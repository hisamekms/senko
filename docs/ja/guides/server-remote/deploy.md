# `senko serve` をデプロイする

チーム共有の senko サーバを立ち上げる手順です。本番運用で選択する認証方式を先に決めてから読み進めてください:

- **[OIDC 認証](auth-oidc.md)** — 本番の推奨方式。人間は OAuth Authorization Code + PKCE、bot は OAuth Client Credentials (M2M) で同じ `[server.auth.oidc]` 設定を使う
- **[信頼ヘッダ (trusted_headers) 認証](auth-trusted-headers.md)** — API Gateway / reverse proxy 前段で認証を終端し、senko にはヘッダで identity を渡す構成
- **[AWS デプロイ (API Gateway + Cognito + Lambda Web Adapter)](aws-deployment.md)** — 上記 trusted_headers パターンの具体例
- [API キー認証](auth-api-key.md) — 動作確認・ブートストラップ用。本番で積極採用する場面はない

本ページは DB / プロセス管理 / TLS / コンテナ化など **認証以外の共通事項** を扱います。

## 必須要件

- `senko` バイナリ (PostgreSQL を使うなら `postgres` feature 有効ビルド)
- DB: SQLite (動作確認) or PostgreSQL (本番推奨)
- **3 つの認証モードのうちいずれか 1 つ** の設定 — `senko serve` は認証設定が全く無い状態では起動しません
- リバースプロキシ (TLS 終端) — 本番では必ず用意

## PostgreSQL

本番では PostgreSQL を推奨:

```bash
export SENKO_POSTGRES_URL="postgres://senko:****@db.internal:5432/senko?sslmode=require"
# 認証設定は別途 [OIDC 認証](auth-oidc.md) 等を参照
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

`aws-secrets` feature 有効ビルドで RDS 資格情報を ARN 参照にできます:

```toml
[backend.postgres]
rds_secrets_arn = "arn:aws:secretsmanager:ap-northeast-1:123456789:secret:rds/senko"
```

起動時に ARN が解決され、メモリ上でだけ値を保持します。

API キーを使うブートストラップ構成なら `master_key_arn` も同様に参照可能です (→ [API キー認証](auth-api-key.md))。OIDC モードでは IdP との通信に secret が不要なので、通常はこの設定は使いません。

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

`/etc/senko/env` 例 (OIDC モードの場合):

```
SENKO_POSTGRES_URL=postgres://senko:****@db.internal:5432/senko?sslmode=require
# OIDC 設定は config.toml の [server.auth.oidc] 側に書く (env override も可)
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
  senko
# 実際には [server.auth.oidc] を含む config.toml を volume mount する、
# または trusted_headers を API Gateway 側で終端する構成が一般的
```

## 運用観点

- **ヘルスチェック**: `GET /api/v1/health` (認証不要、200)
- **ログ**: `stdout` に JSON で出力。journald / Fluentd で収集
- **メトリクス**: v1 時点で組み込みなし → hook + 外部基盤
- **バックアップ**: SQLite なら `[backend.sqlite] db_path` で明示指定したパスの snapshot (未指定なら `$XDG_DATA_HOME/senko/projects/<dir>/data.db`)、PostgreSQL は `pg_dump`
- **アップグレード**: 新バイナリを配置 → サービス再起動。マイグレーションは自動。本番では事前に別 DB で検証を

## 次のステップ

- 認証有効化 → [OIDC 認証](auth-oidc.md) (推奨) / [信頼ヘッダ (trusted_headers) 認証](auth-trusted-headers.md)
- 動作確認だけしたい → [API キー認証](auth-api-key.md)
- hook を仕込む → [`[server.remote.*]` hook の実例](hooks.md)
- AWS 構成 → [AWS デプロイ (API Gateway + Cognito + Lambda Web Adapter)](aws-deployment.md)
