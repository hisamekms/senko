# AWS デプロイ (API Gateway + Cognito + Lambda Web Adapter)

API Gateway HTTP API で JWT 検証を終端し、Lambda 上で動く senko に信頼ヘッダで identity を渡す構成。

## アーキテクチャ

```
Client ──[Authorization: Bearer <Cognito JWT>]──┐
                                                 ▼
                                        API Gateway HTTP API
                                            │
                                            ├─ Cognito JWT Authorizer (JWT 検証)
                                            ├─ Parameter Mapping (JWT claim → x-senko-* ヘッダ)
                                            ▼
                                        Lambda (Web Adapter)
                                            │
                                            │  x-senko-user-sub: <sub>
                                            │  x-senko-user-name: <name>
                                            │  x-senko-user-email: <email>
                                            │  x-senko-user-groups: <groups>
                                            ▼
                                        senko serve (trusted_headers モード)
                                            │
                                            └─ Backend: Aurora / RDS PostgreSQL
```

- **API Gateway**: TLS 終端 + JWT 検証 + ヘッダ変換
- **Lambda Web Adapter**: Lambda 実行環境で普通の HTTP サーバ (`senko serve`) を動かせるようにする adapter
- **senko**: `trusted_headers` モードで動き、ヘッダだけを見て identity を決定
- **DB**: RDS / Aurora PostgreSQL (別 VPC で管理推奨)

## 前提

- AWS アカウント
- Cognito User Pool
- API Gateway HTTP API (REST ではなく HTTP API を使う)
- [Lambda Web Adapter](https://github.com/awslabs/aws-lambda-web-adapter) レイヤ
- `senko` バイナリ (postgres + aws-secrets feature)

## Step 1. Cognito User Pool

ユーザプールを作成。メモしておく値:

- **User Pool ID**: `ap-northeast-1_XXXXXXXXX`
- **Issuer URL**: `https://cognito-idp.{region}.amazonaws.com/{user-pool-id}`

App client を作成:

- Public client (client secret なし)
- PKCE を有効化
- Callback URL: `http://127.0.0.1:<port>/callback` (CLI login で使うローカル callback ポート)

## Step 2. Lambda 関数 (senko)

### パッケージ方法

- Rust binary を `aarch64-unknown-linux-musl` or `x86_64-unknown-linux-musl` でビルド (postgres + aws-secrets feature)
- Lambda Web Adapter 共通レイヤを追加
- ハンドラは `bootstrap` 相当で `senko serve --host 127.0.0.1 --port 8080` を実行するラッパースクリプトを置く
- タイムアウトは 30 秒 〜 15 分で適切に (長時間接続は考慮しない)

### 環境変数

```
SENKO_POSTGRES_URL                = (set by rds_secrets_arn instead)
SENKO_AUTH_API_KEY_MASTER_KEY_ARN = arn:aws:secretsmanager:...:secret:senko/master-key
PORT                              = 8080    # Lambda Web Adapter が見るポート
```

### IAM

- `secretsmanager:GetSecretValue` (master key + RDS secret)
- VPC 内の Lambda にして RDS にアクセスするなら VPC 設定
- CloudWatch Logs 書き込み権限

### 設定ファイル

Lambda 内の `.senko/config.toml` に:

```toml
[backend.postgres]
# 推奨: RDS Proxy のエンドポイントを指す secret。Proxy が接続プールを集約するので
# Lambda からは短命接続で済む。Proxy なしで直接 RDS に繋ぐ場合は max_connections を
# 1〜3 程度に絞り、同時実行数を別途制限すること。
rds_secrets_arn = "arn:aws:secretsmanager:...:secret:rds-proxy/senko"
sslrootcert     = "/opt/rds-ca-bundle.pem"
max_connections = 5

[server.auth.trusted_headers]
subject_header      = "x-senko-user-sub"
name_header         = "x-senko-user-name"
email_header        = "x-senko-user-email"
groups_header       = "x-senko-user-groups"
oidc_issuer_url     = "https://cognito-idp.ap-northeast-1.amazonaws.com/ap-northeast-1_XXXXXXXXX"
oidc_client_id      = "your-app-client-id"

[server.auth.api_key]
master_key_arn = "arn:aws:secretsmanager:...:secret:senko/master-key"

[log]
format = "json"
level  = "info"
```

## Step 3. API Gateway HTTP API

### Authorizer

- Type: **JWT Authorizer**
- Issuer URL: Cognito の issuer URL
- Audience: app client ID
- Identity source: `$request.header.Authorization`

### Routes

1 つの `$default` route で全 path を Lambda に流す:

```
ANY / {proxy+}   →   Lambda integration (HTTP API)
```

Authorizer を route に適用。

### Parameter Mapping (Authorizer → ヘッダ)

以下を `Overwrite request headers` で追加:

| ヘッダ | Value |
|---|---|
| `x-senko-user-sub` | `$context.authorizer.claims.sub` |
| `x-senko-user-name` | `$context.authorizer.claims.cognito:username` |
| `x-senko-user-email` | `$context.authorizer.claims.email` |
| `x-senko-user-groups` | `$context.authorizer.claims.cognito:groups` |

**注**: クライアントが勝手に `x-senko-*` を送ってきた場合に上書きされるよう、Parameter Mapping の mode は "Overwrite" にすること。"Append" は危険。

## Step 4. VPC / RDS

- senko Lambda は private subnet に配置
- RDS (Aurora PostgreSQL) も同 VPC、senko Lambda からのみ接続可能に
- `secretsmanager:*` には VPC endpoint が必要 (NAT Gateway 経由でも可だが endpoint 推奨)

## Step 5. CLI 側

チームメンバーの CLI 設定:

```toml
[cli.remote]
url = "https://senko.example.com"   # API Gateway のカスタムドメイン
```

ログイン:

```bash
senko auth login
```

挙動: senko は `GET /auth/config` でサーバ (= API Gateway → Lambda → senko) から `oidc_issuer_url` / `oidc_client_id` を取得し、Cognito でログインし、取得した JWT を Bearer で送る。API Gateway が JWT を検証・ヘッダ変換して senko に渡す。

## 運用 Tips

- **Cold start**: 初回 Lambda 起動で 1-3 秒 + DB 接続コスト。ProvisionedConcurrency を検討
- **Connection pool**: Lambda は同時実行数分だけ新しいインスタンスが立ち上がるため、各 Lambda が直接 RDS に `max_connections` 分プールすると **RDS 側が瞬時に枯渇** しうる。**RDS Proxy を間に挟むのが推奨構成** (Proxy が接続プールを共有し、Lambda 側はそこに短命接続で繋ぐ)。Proxy を使わない場合は `[backend.postgres] max_connections` を 1〜3 程度に抑え、同時実行を ProvisionedConcurrency / reserved concurrency で制限する
- **Migration**: 新バージョンの Lambda がデプロイされると自動で未適用マイグレーションが走る。canary デプロイで前バージョンとの共存時間を最小化。Proxy を挟んでいれば複数バージョンの Lambda が共存しても接続枯渇は緩和される
- **master key ローテーション**: Secrets Manager 側でローテートすれば Lambda は次回 cold start 時に新値を取得
- **監査ログ**: `[server.remote.*.hooks.audit]` で Lambda から CloudWatch Metrics や EventBridge に流す

## トラブルシューティング

| 症状 | 対処 |
|---|---|
| 401 Unauthorized | API Gateway の Authorizer 設定を確認 (JWT audience / issuer) |
| senko が `user unknown` エラー | Parameter Mapping が効いていない。API Gateway ログで x-senko-* が付いているか確認 |
| RDS に繋がらない | Lambda の Security Group / Subnet / RDS (or RDS Proxy) の SG を確認 |
| `secretsmanager:GetSecretValue` denied | Lambda の IAM role と VPC endpoint ポリシーを確認 |
| RDS の `max_connections` 枯渇 | **RDS Proxy** を間に挟むのが第一選択。未導入なら `[backend.postgres] max_connections` を絞り、ProvisionedConcurrency / reserved concurrency で同時実行数を制限 |
| cold start が重い | ProvisionedConcurrency を検討 |

## 次のステップ

- trusted_headers の詳細 → [auth-trusted-headers.md](auth-trusted-headers.md)
- hook で監査ログを CloudWatch に → [hooks.md](hooks.md)
