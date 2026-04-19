# CLI → Remote → PostgreSQL

チーム共有の senko サーバを立て、開発者の CLI から接続する構成。多くのチーム運用で標準的な形。

→ この構成で 3 つの柱がどう動くかは [コアコンセプト](../explanation/core-concept.md) 参照。

```
┌──────────────────┐        HTTPS (Bearer auth)
│  Developer CLI   │ ───────────────────────────┐
│  (SENKO_CLI_REMOTE_URL)                       │
└──────────────────┘                            │
                                                ▼
                                    ┌──────────────────────┐
                                    │  senko serve         │
                                    │  (direct mode)       │
                                    │                      │
                                    │  [server.auth.*]     │
                                    └──────────┬───────────┘
                                               │
                                               ▼
                                    ┌──────────────────────┐
                                    │  PostgreSQL (RDS/    │
                                    │  Aurora/self-hosted) │
                                    └──────────────────────┘
```

## いつ選ぶか

- **複数開発者** が同じ task DB を共有したい
- **CI/CD** や bot からも senko を叩きたい
- **SSO 配下** でアクセス制御したい
- **監査ログ** を一元化したい
- データの永続性・バックアップを DB 層で管理したい

## 構成要素

| コンポーネント | 役割 | secrets の所在 |
|---|---|---|
| senko CLI | 開発者が日常使うクライアント | OIDC セッション or 個人 API キー |
| senko serve (direct) | 中央の senko サーバ | PostgreSQL credential / master key |
| PostgreSQL | データ永続層 | (DB 内部) |
| OIDC IdP (任意) | SSO 認証 | (IdP 側) |

認証方式 (ユーザ用):

| 方式 | 位置づけ | 詳細 |
|---|---|---|
| **OIDC (OAuth PKCE)** | **本番の人間ユーザ認証の推奨方式** | [guides/server-remote/auth-oidc.md](../guides/server-remote/auth-oidc.md) |
| API キー | CI / bot / 試用用。人間のログインには OIDC を推奨 | [guides/server-remote/auth-api-key.md](../guides/server-remote/auth-api-key.md) |
| 信頼ヘッダ | API Gateway 配下 | [guides/server-remote/auth-trusted-headers.md](../guides/server-remote/auth-trusted-headers.md) |

## セットアップ手順 (OIDC 構成の例)

OIDC が最も現場で使いやすいため、ここでは OIDC 構成を主例にします。API キーのみで始めたい場合は [auth-api-key.md](../guides/server-remote/auth-api-key.md) へ。

### Step 1: PostgreSQL を用意

別サーバ (RDS / Aurora / 自前 Postgres) に DB / ユーザを作成:

```sql
CREATE DATABASE senko;
CREATE USER senko WITH PASSWORD '****';
GRANT ALL PRIVILEGES ON DATABASE senko TO senko;
```

senko サーバ側で DB URL を決定:

```
postgres://senko:****@db.internal:5432/senko?sslmode=require
```

(マイグレーションは初回起動時に自動適用される。事前作業不要)

### Step 2: OIDC IdP を設定

Google / Cognito / Keycloak / Auth0 等で **Public OAuth Client** を登録:

- Grant: authorization_code (PKCE)
- Redirect URIs: `http://127.0.0.1:<port>/callback` (`callback_ports` と合わせる)
- Scopes: `openid profile email`
- client secret: 不要

メモしておく値:
- Issuer URL
- Client ID

### Step 3: senko サーバを起動

サーバ (trusted host) で `senko` バイナリ (`postgres` feature 有効ビルド) を配置。

`/var/lib/senko/.senko/config.toml`:

```toml
[server]
host = "0.0.0.0"
port = 3142

[backend.postgres]
url = "postgres://senko:****@db.internal:5432/senko?sslmode=require"
max_connections = 10

[server.auth.oidc]
issuer_url     = "https://accounts.example.com"
client_id      = "senko-cli"
scopes         = ["openid", "profile", "email"]
callback_ports = ["8400", "9000-9010"]

[server.auth.oidc.session]
ttl          = "30d"
inactive_ttl = "7d"
max_per_user = 10

# ユーザ作成用の bootstrap 鍵 (通常の API 操作には使わない)
[server.auth.api_key]
master_key_arn = "arn:aws:secretsmanager:ap-northeast-1:123:secret:senko/master-key"

[log]
format = "json"
level  = "info"

# 監査ログを syslog に
[server.remote.task_add.hooks.audit]
command = "logger -t senko-audit 'task_add'"
mode = "async"
[server.remote.task_complete.hooks.audit]
command = "logger -t senko-audit 'task_complete'"
mode = "async"
```

> **注**: `api_key` と `oidc` は **同時有効可**。`api_key` は master key (bootstrap 専用) 用途で、通常の認証は OIDC が担う。

systemd で常駐:

```ini
# /etc/systemd/system/senko.service
[Service]
User=senko
WorkingDirectory=/var/lib/senko
EnvironmentFile=/etc/senko/env
ExecStart=/usr/local/bin/senko serve --host 0.0.0.0 --port 3142
Restart=on-failure
```

```bash
sudo systemctl enable --now senko
curl http://127.0.0.1:3142/api/v1/health
# {"status":"ok"}
```

### Step 4: TLS 終端

senko 自身は TLS しないので nginx / Caddy 等を前段に:

```nginx
server {
  listen 443 ssl http2;
  server_name senko.example.com;
  ssl_certificate     /etc/ssl/senko.crt;
  ssl_certificate_key /etc/ssl/senko.key;
  location / {
    proxy_pass http://127.0.0.1:3142;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $remote_addr;
  }
}
```

### Step 5: プロジェクトメンバーを準備

master key で最初のユーザ + プロジェクトメンバーを追加:

```bash
export MASTER_KEY="$(aws secretsmanager get-secret-value --secret-id senko/master-key --query SecretString --output text)"

# ユーザ (OIDC login すれば自動 JIT 登録もされるが、明示的な作成も可)
curl -s -X POST https://senko.example.com/api/v1/users \
  -H "Authorization: Bearer $MASTER_KEY" \
  -H "Content-Type: application/json" \
  -d '{"username":"alice","sub":"alice@example.com"}'

# プロジェクトを作って
curl -s -X POST https://senko.example.com/api/v1/projects \
  -H "Authorization: Bearer $MASTER_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name":"backend-team"}'

# alice を member に
curl -s -X POST https://senko.example.com/api/v1/projects/2/members \
  -H "Authorization: Bearer $MASTER_KEY" \
  -H "Content-Type: application/json" \
  -d '{"user_id":2,"role":"member"}'
```

### Step 6: 開発者の CLI を設定

開発者は各自のマシンで:

```bash
# senko をインストール
curl -fsSL https://raw.githubusercontent.com/hisamekms/senko/main/install.sh | sh

# プロジェクトで
cd your-project
senko skill-install

# ログイン (keychain にセッション token が保存される)
senko auth login
```

`.senko/config.toml` (git commit 可):

```toml
[cli.remote]
url = "https://senko.example.com"
# token は書かない — keychain 経由で取得される

[project]
name = "backend-team"
```

動作確認:

```bash
senko auth status         # 誰としてログインしているか
senko task list           # リモート DB からタスク取得
```

### Step 7: CI / bot のセットアップ (任意)

CI 環境は OIDC を使わず API キーで:

```bash
# master key で CI 用のユーザと API キーを発行
curl -X POST https://senko.example.com/api/v1/users \
  -H "Authorization: Bearer $MASTER_KEY" \
  -d '{"username":"ci-bot"}'
curl -X POST https://senko.example.com/api/v1/projects/2/members \
  -H "Authorization: Bearer $MASTER_KEY" \
  -d '{"user_id":3,"role":"member"}'
curl -X POST https://senko.example.com/api/v1/users/3/api-keys \
  -H "Authorization: Bearer $MASTER_KEY" \
  -d '{"name":"ci-bot"}'
# => key を CI secret に保存
```

CI ジョブでは:

```yaml
env:
  SENKO_CLI_REMOTE_URL: https://senko.example.com
  SENKO_CLI_REMOTE_TOKEN: ${{ secrets.SENKO_CI_TOKEN }}
steps:
  - run: senko task list --status todo --output json
```

## セキュリティチェックリスト

- [ ] TLS 終端 (nginx / ALB / Cloudflare など) が前段に居る
- [ ] PostgreSQL の credential は Secrets Manager or EnvironmentFile で注入、ログに出ていない
- [ ] master key は **通常運用で使わない**。ユーザ発行 bootstrap 専用
- [ ] OIDC セッション TTL (`ttl` / `inactive_ttl`) が組織ポリシーに沿っている
- [ ] `[server.remote.*]` で監査 hook が仕込まれている
- [ ] DB バックアップが取れている (`pg_dump` / RDS snapshot)

## よくあるトラブル

| 症状 | 対処 |
|---|---|
| `senko auth login` でコールバック失敗 | `callback_ports` のポートがファイアウォールで塞がれていないか |
| 401 Unauthorized | セッション TTL 切れ → 再 `senko auth login` |
| 403 Forbidden | プロジェクト member 登録が漏れている |
| 初回起動でマイグレーション失敗 | DB ユーザに CREATE TABLE 権限があるか |
| 複数 Lambda インスタンスから繋いで DB 接続枯渇 | `[backend.postgres] max_connections` を絞る |

## AWS にデプロイする場合

API Gateway + Cognito + Lambda Web Adapter を使った構成の詳細は [guides/server-remote/aws-deployment.md](../guides/server-remote/aws-deployment.md)。

## 参考

- サーバ起動詳細 → [guides/server-remote/deploy.md](../guides/server-remote/deploy.md)
- 認証モード各種 → [guides/server-remote/auth-api-key.md](../guides/server-remote/auth-api-key.md) / [auth-oidc.md](../guides/server-remote/auth-oidc.md)
- サーバ側 hook 実例 → [guides/server-remote/hooks.md](../guides/server-remote/hooks.md)
