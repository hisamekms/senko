# Dev Bypass (`dev_bypass`) 認証 — 開発専用

> **開発専用です。localhost の外から到達できる場所で絶対に有効化しないでください**。
>
> `SENKO_ENV=production` でブートすると **起動を拒否** します。このモードは認証を完全に無効化し、すべてのリクエストに **master 権限** を与えます。目的はローカル開発と Playwright E2E から IdP / API キーの摩擦を取り除くこと、それだけです。

## いつ使うか

- フロントエンド開発で、Cognito / Auth0 / Keycloak を立てずに senko remote API を叩きたいとき
- 認証済みセッションは必要だが本物の identity はどうでもいい E2E
- `[server.auth.api_key]` / OIDC / Trusted Headers をいじらずに senko ビルドを smoke test したいとき

ローカル以外のデプロイには、必ず以下のいずれかを使ってください:

- [API Key 認証](auth-api-key.md) — 評価・smoke test 向け
- [OIDC 認証](auth-oidc.md) — production
- [信頼ヘッダ (trusted_headers) 認証](auth-trusted-headers.md) — production を API Gateway 経由で

## 有効化のしかた

CLI フラグ:

```bash
senko serve --dev-no-auth
```

または `config.toml`:

```toml
[server.auth.dev_bypass]
enabled = true
```

CLI フラグが優先されます。`--dev-no-auth` を渡すと、それより低優先度の設定で off に戻すことはできません。

## 何が起こるか

すべてのリクエストが、**同じ固定ダミーユーザ** に解決されます:

- `id = 1`（`DEFAULT_USER_ID`。`sync_config_defaults` によって初回起動時に必ず作られる行と一致するので、外部キー制約に引っかかりません）
- `username = "dev-bypass"`
- `is_master = true` — プロジェクトメンバーシップ判定と master 専用エンドポイントの両方を素通り

`AuthUser` extractor は `Authorization` / `x-senko-*` を一切見ません。Bearer も session も存在しません。

`GET /auth/config` は `auth_mode = "dev_bypass"` を返すので、フロントが「DEV MODE」バナーを描画できます。

## やらないこと（重要）

- **`POST /auth/token`** は `501 Not Implemented` を返します。JWT → API キー交換を許すと、ダミーユーザが本物の DB 行として永続化され、本物の session token が払い出されてしまうため、bypass モードでは明示的に拒否します
- **`/auth/me`** に紐づく session はありません（`session: null`）
- **`[server.relay]` と併用不可**。両方設定してブートすると即座にエラー終了します
- **他の認証モード（`api_key` / `oidc` / `trusted_headers`）と併用不可**。4 モードは互いに排他です

## Production ガード

`validate_serve_auth` は relay 以外のブート前に必ず走り、`dev_bypass.enabled = true` と `SENKO_ENV=production`（大文字小文字無視、前後空白トリム）の組み合わせを拒否します。

```bash
$ SENKO_ENV=production senko serve --dev-no-auth
Error: dev auth bypass cannot be enabled with SENKO_ENV=production. Unset SENKO_ENV or remove [server.auth.dev_bypass] / --dev-no-auth.
```

bypass モードでブートするたびに `WARN` ログが出ます:

```
WARN dev auth bypass enabled — DO NOT USE IN PRODUCTION
```

config 解決時と "Listening on …" の直前の **2 回** 出るので、起動ログを冒頭からテーリングしている運用者にも見落としようがありません。

## 動作確認

```bash
# 警告を吐いて起動成功
senko serve --dev-no-auth

# 別シェルで — Authorization ヘッダなしで通る
curl -sf http://127.0.0.1:3142/auth/config
# → {"auth_mode":"dev_bypass","oidc":null}

curl -sf http://127.0.0.1:3142/auth/me
# → {"user":{"id":1,"username":"dev-bypass",...},"session":null}

# /auth/token は 501
curl -i -X POST http://127.0.0.1:3142/auth/token -H 'Content-Type: application/json' -d '{}'
# HTTP/1.1 501 Not Implemented
```
