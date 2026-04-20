# OIDC 認証

OAuth 2.0 / OIDC の JWT を Bearer として受け付ける認証モード。社内 SSO や Google / Cognito / Keycloak / Auth0 等の IdP 配下で使う想定。

> **本番運用の推奨方式**。人間ユーザは **OAuth Authorization Code + PKCE** で、CI/bot/サービスアカウントは **OAuth Client Credentials (M2M)** でそれぞれ JWT を取得して senko に送る。senko 側は JWT 検証だけを行うので、同じ `[server.auth.oidc]` 設定でどちらの経路もカバーできます。API キー認証は試用用途のみに留めてください。

## どう動くか

```
CLI ── senko auth login ──┐
                          ├── ブラウザで IdP にリダイレクト
                          ├── ログイン → PKCE exchange
                          └── senko が JWT を受け取り、内部で API キーを発行して keychain に保存

その後の senko コマンドは keychain から token を取り出して Bearer で送る
```

JWT をそのまま送り続けるわけではなく、**初回 1 回だけ JWT 検証を行い、以降は senko 内部の API キーに変換** しているのがポイントです。

## サーバ側の設定

```toml
[server.auth.oidc]
issuer_url = "https://accounts.example.com"
client_id  = "senko-cli"
scopes     = ["openid", "profile", "email"]
# username_claim = "preferred_username"   # 指定しないと sub を使う
# groups_claim   = "groups"               # master_group 判定に使う claim (既定 "groups")
# master_group   = "senko-admins"         # このグループに属する JWT は is_master=true
# required_claims = { email_verified = "true" }
callback_ports = ["8400", "9000-9010"]    # CLI ログイン時にブラウザが開くローカル callback ポート候補

[server.auth.oidc.session]
ttl          = "30d"    # 絶対 TTL
inactive_ttl = "7d"     # 無活動タイムアウト
max_per_user = 10       # 1 ユーザあたりセッション上限
```

- `issuer_url` から `.well-known/openid-configuration` が取得できる必要がある
- `client_id` は IdP 側で "Public client / PKCE" として登録する (secret 不要)
- `callback_ports` は **CLI 側のマシンで開くポート候補**。個別 or range 指定可

> **他モードとの排他**: `[server.auth.api_key]`(`master_key`) / `[server.auth.oidc]` / `[server.auth.trusted_headers]` は **同時に 1 つだけ** しか有効化できません。OIDC モードを選んだなら `master_key` は設定しない。

## ユーザの自動登録 (JIT)

OIDC モードでは **初回認証時にユーザが自動作成** されます。事前にユーザを発行する必要はありません。

- JWT の `sub` → `users.sub` に保存
- `username_claim` で指定した claim が `username` (未指定なら `preferred_username` → `email` → `sub` の順)
- `name` / `email` claim があれば `display_name` / `email` にも入る

初回ログインした人は senko 上でまだ **どのプロジェクトの member でもない** ので、操作できるのは以下のみ:

- 自分のプロフィール取得 (`/auth/me`)
- **新規プロジェクトの作成** (`POST /api/v1/projects`、作成者が自動で owner になる)
- `master_group` のクレームを持っていれば、全プロジェクト・全ユーザを操作可能 (後述)

したがって **OIDC モードは self-bootstrap が可能** — 最初のユーザがログインして自分のプロジェクトを作り、そこに他の member を招待すれば運用開始できます。master_key は不要。

## master 権限: `master_group`

OIDC モードでは **グループクレーム** によって master 権限を与えます (API キーモードの `master_key` とは別機構):

```toml
[server.auth.oidc]
groups_claim = "groups"              # JWT のどの claim を見るか (既定 "groups")
master_group = "senko-admins"        # この group に属するユーザは is_master=true
```

- `groups_claim` は JWT 内で **文字列配列** を持つ claim 名。Cognito なら `cognito:groups`、Auth0 なら mapping で `groups` を出すよう設定
- `master_group` に一致するエントリが配列に含まれると、その JWT で認証した caller は `is_master=true`
- `is_master=true` のユーザは **全プロジェクトのメンバーシップ検査を bypass**、`POST /api/v1/users` (ユーザ CRUD) も使える

master_group を設定しない構成でも OIDC は動きます。その場合 senko 上に master はおらず、運用は各プロジェクト owner 単位で完結します (多くのチームではこれで十分)。

## IdP 側の設定

### 人間ユーザ用 (PKCE)

Public OAuth Client として登録:

- **grant types**: authorization_code (PKCE)
- **redirect URIs**: `http://127.0.0.1:<port>/callback` (callback_ports と一致させる)
- **scopes**: `openid profile email`
- client secret: 不要

### bot / サービスアカウント用 (Client Credentials / M2M)

Confidential (Machine-to-Machine) Client として別途登録:

- **grant types**: client_credentials
- **audience** (Auth0 等): senko サーバの URL
- **scopes**: 権限を絞りたければ (senko 自体は scope を見ないが、IdP 側の access control に使える)
- **client_id + client_secret**: bot の secret store (CI 変数 / Secrets Manager 等) に保管

senko サーバ側の `[server.auth.oidc]` は 1 つで両方を受ける (issuer / audience / required_claims が一致していれば OK)。ただし `username_claim` / `required_claims` は **M2M トークンでも成立する claim** を選ぶこと (後述)。

## クライアント側 (CLI)

```toml
# .senko/config.toml
[cli.remote]
url = "https://senko.example.com"
# token は keychain 経由なのでここには書かない
```

初回ログイン:

```bash
senko auth login [--device-name "alice-laptop"]
```

挙動:

1. ブラウザが立ち上がる (`[cli] browser = false` なら URL が stdout に出るだけ)
2. IdP で認証
3. CLI が callback を受けて PKCE で token 交換
4. サーバ側で JWT 検証 → 内部 API キーを作って返す
5. CLI が OS keychain にその API キーを保存

以降:

```bash
senko auth status     # 今のログイン情報
senko auth sessions   # 発行済みセッション (= 内部 API キー) 一覧
senko auth logout     # 現セッションを revoke + keychain 削除
senko auth revoke <id>        # 他デバイスを revoke
senko auth revoke --all       # 全セッション revoke
```

## keychain の中身

- macOS: Keychain Access → `senko` サービス
- Linux: libsecret / gnome-keyring の `senko` エントリ
- Windows: Credential Manager の `senko`

## CI / bot (OAuth Client Credentials / M2M)

`senko auth login` は対話フローなので CI や headless 環境では使えません。代わりに IdP から **Client Credentials** で JWT を直接取得し、`SENKO_CLI_REMOTE_TOKEN` として注入します。

```bash
# IdP から JWT 取得 (Auth0 の例)
JWT=$(curl -s https://accounts.example.com/oauth/token \
  -H "Content-Type: application/json" \
  -d '{
    "client_id":     "senko-bot",
    "client_secret": "'"$SENKO_BOT_CLIENT_SECRET"'",
    "audience":      "https://senko.example.com",
    "grant_type":    "client_credentials"
  }' | jq -r '.access_token')

# senko に送る
export SENKO_CLI_REMOTE_URL="https://senko.example.com"
export SENKO_CLI_REMOTE_TOKEN="$JWT"
senko task list
```

GitHub Actions 等では secret に client_secret を入れて、ジョブ開始時にこのステップを挟むだけで OK。

### claim 設計の注意

人間の JWT と M2M の JWT で claim が違うので、senko 側の `username_claim` / `required_claims` は両方で成立するものを選ぶ:

| 想定 | 人間 JWT | M2M JWT |
|---|---|---|
| `sub` | ユーザ ID (IdP 固有) | client_id |
| `email` / `email_verified` | あり | **なし** |
| `preferred_username` / `name` | あり | なし (IdP 次第) |
| カスタム claim (`username`, `service` 等) | IdP の mapping 次第 | IdP の mapping 次第 |

- `username_claim = "sub"` が最もシンプル。M2M なら `senko-bot` のような client_id が username として登録される
- `required_claims = { email_verified = "true" }` のような人間前提の制約は M2M を弾くので付けない
- 人間と bot で権限を分けたければ、IdP のカスタム claim (例: `"type": "service"`) + senko 側 JIT 登録後にプロジェクトの role を調整

### JWT の短命問題

Client Credentials で取得した access token は通常 1 時間程度で失効。長時間走るジョブでは取り直しが必要です:

- ジョブステップごとに JWT を取り直す (短いジョブなら十分)
- 数時間かかる場合は bash ヘルパで `exp` claim を見て残り 5 分を切ったら refetch
- どうしても長命 token が必要なら限定的に [API キー認証](auth-api-key.md) を検討

## セッション管理

サーバ側では OIDC ログイン由来の API キーを "session" として区別します:

- `[server.auth.oidc.session] ttl` 経過で失効 (再ログイン必要)
- `inactive_ttl` 経過 (最終使用から) で失効
- `max_per_user` に達すると古いセッションが落とされる

## 信頼ヘッダと併用できない

`[server.auth.oidc]` と `[server.auth.trusted_headers]` は同時有効化できません。API Gateway 配下で OIDC を処理する構成は `trusted_headers` を使ってください ([信頼ヘッダ (trusted_headers) 認証](auth-trusted-headers.md))。

## トラブルシューティング

| 症状 | 確認点 |
|---|---|
| `senko auth login` でブラウザが開かない | ヘッドレスなら `[cli] browser = false` で URL コピー運用 |
| callback で connection refused | `callback_ports` の範囲がファイアウォールで潰れていないか |
| ログインは成功するが API で 401 | `username_claim` が IdP の claim と合っているか |
| 毎回再ログインを求められる | `[server.auth.oidc.session] ttl` / `inactive_ttl` が短すぎないか |
| SSO 側の groups/roles を senko の権限に反映したい | 現状マッピング機能なし。member を手動で追加するか、`required_claims` で絞る |

## 次のステップ

- API Gateway (Cognito) 配下で OIDC を終端させ、senko は信頼ヘッダで受ける構成 → [信頼ヘッダ (trusted_headers) 認証](auth-trusted-headers.md) と [AWS デプロイ (API Gateway + Cognito + Lambda Web Adapter)](aws-deployment.md)
