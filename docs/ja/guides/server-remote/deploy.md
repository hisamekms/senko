# `senko serve` をデプロイする

チーム共有の senko サーバを立ち上げる手順です。認証の詳細は別ページを参照:

- [API キー認証](auth-api-key.md)
- [OIDC 認証](auth-oidc.md)
- [信頼ヘッダ (trusted_headers) 認証](auth-trusted-headers.md) — API Gateway 配下
- [AWS デプロイ (API Gateway + Cognito + Lambda Web Adapter)](aws-deployment.md)

## 必須要件

- `senko` バイナリ (PostgreSQL を使うなら `postgres` feature 有効ビルド)
- DB: SQLite (試用) or PostgreSQL (本番推奨)
- リバースプロキシ (TLS 終端、API キー配信等) — 本番では必ず用意

## 試用構成 (SQLite + API キー)

> **試用・初期動作確認用の最小構成です**。本番の人間ユーザ認証には [OIDC 認証](auth-oidc.md) を使ってください。

```bash
# 1. 任意のディレクトリで DB を持つ
mkdir -p /var/lib/senko && cd /var/lib/senko

# 2. master key を生成
export SENKO_AUTH_API_KEY_MASTER_KEY="$(openssl rand -base64 32)"

# 3. 起動
senko serve --host 0.0.0.0 --port 3142
```

起動確認:

```bash
curl http://127.0.0.1:3142/api/v1/health
# {"status":"ok"}
```

## PostgreSQL

```bash
export SENKO_POSTGRES_URL="postgres://senko:****@db.internal:5432/senko?sslmode=require"
export SENKO_AUTH_API_KEY_MASTER_KEY="$(openssl rand -base64 32)"
senko serve --host 0.0.0.0 --port 3142
```

初回起動で未適用マイグレーションが自動適用されます。

接続プールチューニング:

```toml
[backend.postgres]
url = "postgres://..."
max_connections = 20
```

## AWS Secrets Manager 経由で credential を注入

`aws-secrets` feature 有効ビルドで:

```toml
[backend.postgres]
rds_secrets_arn = "arn:aws:secretsmanager:ap-northeast-1:123456789:secret:rds/senko"

[server.auth.api_key]
master_key_arn = "arn:aws:secretsmanager:ap-northeast-1:123456789:secret:senko/master-key"
```

起動時に ARN が解決され、メモリ上でだけ値を保持します。

## systemd ユニット例

```ini
# /etc/systemd/system/senko.service
[Unit]
Description=senko server
After=network.target

[Service]
Type=simple
User=senko
Group=senko
WorkingDirectory=/var/lib/senko
EnvironmentFile=/etc/senko/env
ExecStart=/usr/local/bin/senko serve --host 0.0.0.0 --port 3142
Restart=on-failure
RestartSec=5s

# セキュリティハードニング
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ReadWritePaths=/var/lib/senko

[Install]
WantedBy=multi-user.target
```

`/etc/senko/env` 例:

```
SENKO_POSTGRES_URL=postgres://senko:****@db.internal:5432/senko?sslmode=require
SENKO_AUTH_API_KEY_MASTER_KEY=***
```

有効化:

```bash
sudo systemctl enable --now senko
sudo journalctl -u senko -f
```

## TLS / リバースプロキシ

senko サーバ自体は TLS 終端しません。nginx / Caddy / API Gateway 等を前に置いてください:

```nginx
# nginx 例
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

## Docker で動かす

```dockerfile
FROM debian:bookworm-slim
ARG SENKO_VERSION=1.0.0
ARG TARGETARCH
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/* \
 && case "$TARGETARCH" in \
      amd64) T=x86_64-unknown-linux-musl ;; \
      arm64) T=aarch64-unknown-linux-musl ;; \
    esac \
 && curl -fsSL "https://github.com/hisamekms/senko/releases/download/v${SENKO_VERSION}/senko-v${SENKO_VERSION}-${T}.tar.gz" \
  | tar xz -C /usr/local/bin senko
WORKDIR /data
ENTRYPOINT ["senko"]
CMD ["serve", "--host", "0.0.0.0", "--port", "3142"]
```

```bash
docker run --rm -p 3142:3142 \
  -v senko-data:/data/.senko \
  -e SENKO_POSTGRES_URL=postgres://... \
  -e SENKO_AUTH_API_KEY_MASTER_KEY=*** \
  senko
```

## 運用観点

- **ヘルスチェック**: `GET /api/v1/health` (認証不要、200)
- **ログ**: `stdout` に JSON で出力。journald / Fluentd で収集
- **メトリクス**: v1 時点で組み込みなし → hook + 外部基盤
- **バックアップ**: SQLite なら `[backend.sqlite] db_path` で明示指定したパスの snapshot (未指定なら `$XDG_DATA_HOME/senko/projects/<dir>/data.db`)、PostgreSQL は `pg_dump`
- **アップグレード**: 新バイナリを配置 → サービス再起動。マイグレーションは自動。本番では事前に別 DB で検証を

## 次のステップ

- 認証有効化 → [API キー認証](auth-api-key.md) or [OIDC 認証](auth-oidc.md)
- hook を仕込む → [`[server.remote.*]` hook の実例](hooks.md)
- AWS 構成 → [AWS デプロイ (API Gateway + Cognito + Lambda Web Adapter)](aws-deployment.md)
