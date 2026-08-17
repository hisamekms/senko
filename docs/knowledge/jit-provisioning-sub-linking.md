---
id: jit-provisioning-sub-linking
title: JITプロビジョニングは既存ユーザーへのsub紐付けが必須
description: create_userはsub未指定時にsub=usernameを保存するため、事前登録ユーザーは初回IdPログインで必ずsub不一致になる。username/email一致で既存ユーザーにsubを紐付け直す設計にした。
tags:
  - auth
  - oidc
  - trusted-headers
  - provisioning
  - cognito
created_at: 2026-08-17
updated_at: 2026-08-17
---

## 概要

trusted_headers / oidc モードの JIT 自動プロビジョニングが、事前登録済みユーザーの初回ログインで
`users_username_key`（または email の UNIQUE 制約）違反により失敗し、全 API が 401 になる障害が発生した
（2026-08-17、Entra ID → Cognito SAML フェデレーション環境）。

## 詳細

前提となる非自明な仕様が2つある:

1. **`create_user` は sub 未指定時に `sub = username` を保存する**（sqlite / postgres 両バックエンド）。
   `senko user create` で事前登録したユーザーの sub は NULL ではなく username（通常はメールアドレス）になる。
2. **マイグレーション `20260413000000_add_user_sub.sql` も既存行に `sub = username` をバックフィル**している。

つまり「sub が NULL の既存ユーザー」は実質存在せず、既存ユーザーは全員「sub = username」を持つ。
IdP が発行する sub は UUID 等の別値なので、**事前登録ユーザーと IdP 移行ユーザーは初回ログインで必ず
`get_user_by_sub` がミスし、自動プロビジョニングの INSERT が username/email の UNIQUE 制約で失敗する**。
このエラーは `AuthError::InvalidToken`（401）に潰されていたため、原因追跡が困難だった。

さらに trusted_headers モードの `senko auth login` はキーチェーン保存のみでサーバを叩かないため、
ログインは「成功」と表示され、後続コマンドで初めて「Not logged in」になる（UX 上の別問題）。

## 解決策 / 推奨事項

`UserService::get_or_create_user`（src/application/user_service.rs）に集約して以下の順で解決する:

1. `get_user_by_sub` でヒットすればそのまま返す
2. ミスしたら username → email の順で既存ユーザーを検索し、見つかれば `update_user_sub` で
   sub を新しい値に**常に紐付け直す**（trusted_headers はヘッダ自体が信頼済み、oidc は検証済み claim
   なので上書きは安全。IdP 移行も自動で救済される）
3. どちらも無ければ create。INSERT が失敗したら sub を1回だけ再検索する（並行初回ログインのレース対策）

`TrustedHeadersAuthProvider` / `JwtAuthProvider`（src/infra/auth.rs）は独自に get+create せず、
必ずこの `get_or_create_user` を経由すること。新しい自動プロビジョニング経路を追加する場合も同様。

## 参考

- 障害事例: takuwa.kei@e-grid.co.jp の初回 Entra ログイン（senko.platform.logrise.co.jp、2026-08-17）
