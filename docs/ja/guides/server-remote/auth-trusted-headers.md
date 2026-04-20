# 信頼ヘッダ (trusted_headers) 認証

API Gateway や reverse proxy が **既に認証/認可を済ませた後に、ユーザ identity をヘッダとして注入してくる** 構成。senko 自身は token 検証を行わず、ヘッダ値を **無条件で信頼** します。

## ⚠️ 必ず読むべきセキュリティ注意

**trusted_headers モードの senko を直接インターネットに出してはいけません**。

- senko は `x-senko-user-sub` 等を検証なしに信用します
- もしクライアントがそのヘッダを直接送れる経路があると、**任意のユーザになりすまし可能**
- API Gateway / reverse proxy が **唯一の入口** で、かつ proxy が必ずクライアント由来の `x-senko-*` を剥ぎ取る構成になっていること

## 基本形 (AWS API Gateway + Cognito の場合)

```
Client ──[Bearer JWT]──> API Gateway (HTTP API)
                            │
                            ├─ Cognito JWT Authorizer で JWT 検証
                            ├─ Parameter Mapping で JWT claim → x-senko-* ヘッダに変換
                            ▼
                          senko serve (trusted_headers モード)
```

このパターンの詳細手順は [AWS デプロイ (API Gateway + Cognito + Lambda Web Adapter)](aws-deployment.md) にあります。

## サーバ側の設定

`subject_header` を設定すると trusted_headers モードが **有効化** されます:

```toml
[server.auth.trusted_headers]
subject_header      = "x-senko-user-sub"       # 必須。sub を運ぶヘッダ
name_header         = "x-senko-user-name"
display_name_header = "x-senko-user-display-name"
email_header        = "x-senko-user-email"
groups_header       = "x-senko-user-groups"
scope_header        = "x-senko-user-scope"

# CLI ログインのフォールバック (GET /auth/config で返される)
oidc_issuer_url     = "https://cognito-idp.ap-northeast-1.amazonaws.com/ap-northeast-1_XXXXX"
oidc_client_id      = "xxxxxxxx"
```

- `subject_header` 以外はすべてオプション
- `name_header` が無く `display_name_header` があれば、そちらがフォールバックとして使われる
- `oidc_*` は「CLI ログインしたい」と CLI が `GET /auth/config` を叩いた時に返す値。ユーザには CLI 側の OIDC ログインフローを使わせたい場合に設定

## クライアント側

通常のクライアント (CLI・ツール) は **API Gateway の前で OIDC 認証して JWT を取得** → API Gateway に Bearer 付けるだけ。senko 自体に直接 credential を送る必要はありません。

CLI 側の `senko auth login` は、サーバから `GET /auth/config` で取得した OIDC メタデータを使って IdP にリダイレクトし、得た JWT をそのまま Bearer で送るという動きをします (→ API Gateway の Authorizer が検証 → ヘッダ注入 → senko に届く)。

## どのユーザとして扱われる?

1. リクエストが届くと `subject_header` の値 (= JWT の `sub` など) を取り出す
2. その `sub` を DB の `users.sub` と突き合わせる
3. 既存ユーザがいればそれで認証完了
4. いなければ **自動でユーザ登録** (JIT provisioning):
   - `username` = `name_header` の値 (なければ `sub`)
   - `display_name` = `display_name_header` / `name_header`
   - `email` = `email_header`

## 認可 (member / role)

trusted_headers で認証したユーザは、**プロジェクトメンバーでないとリソース操作できません**。ただし任意の認証済みユーザは `POST /api/v1/projects` で新規プロジェクトを作成できる (作成者が owner として自動登録される) ので、OIDC モードと同じく **self-bootstrap** が可能です。

全体運用で "super-admin" 的な権限を持たせたい場合は `master_group` を使います:

```toml
[server.auth.trusted_headers]
subject_header = "x-senko-user-sub"
groups_header  = "x-senko-user-groups"     # カンマ区切りのグループ名を受ける
master_group   = "senko-admins"            # このグループに属するユーザは is_master=true
```

- `groups_header` が運んでくる値を **カンマ区切り** でパースし、`master_group` と一致するものがあれば `is_master=true`
- `is_master=true` のユーザは **全プロジェクトのメンバーシップ検査を bypass**、`POST /api/v1/users` 等の master 限定エンドポイントも使える

API Gateway 側の Parameter Mapping で JWT の `cognito:groups` / `roles` などを `x-senko-user-groups` に流し込む設定を入れてください。

## 他の認証モードとの排他性

**3 つの認証モード (`api_key` / `oidc` / `trusted_headers`) は同時に 1 つだけ** しか有効化できません (起動時に mutual exclusive を検査、違反すると bail)。したがって trusted_headers モードで `[server.auth.api_key] master_key` を併記すると **起動エラー** になります。trusted_headers で master 権限を与えたい場合は上記の `master_group` を使ってください。

## トラブルシューティング

| 症状 | 対処 |
|---|---|
| 全リクエストが 401 | API Gateway が `x-senko-*` を注入しているか Parameter Mapping を確認 |
| ユーザがなりすまされる | API Gateway 以外から senko に到達できる経路が開いていないか確認。セキュリティグループ等 |
| JIT 登録されない | `subject_header` が空 / 値が取れていない可能性。API Gateway ログで値を確認 |
| `senko auth login` が動かない | `oidc_issuer_url` / `oidc_client_id` が未設定。または IdP 側で PKCE が無効 |

## 次のステップ

- AWS の具体構築手順 → [AWS デプロイ (API Gateway + Cognito + Lambda Web Adapter)](aws-deployment.md)
