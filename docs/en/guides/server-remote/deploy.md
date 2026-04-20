# Deploying `senko serve`

How to stand up a team-shared senko server. Pick the production auth mode first, then come back here:

- **[OIDC Authentication](auth-oidc.md)** — recommended for production. Humans use OAuth Authorization Code + PKCE; bots use OAuth Client Credentials (M2M); both terminate on the same `[server.auth.oidc]`.
- **[Trusted Headers Authentication](auth-trusted-headers.md)** — put an API Gateway / reverse proxy in front to handle auth and forward identity headers to senko.
- **[AWS Deployment (API Gateway + Cognito + Lambda Web Adapter)](aws-deployment.md)** — a concrete instance of the trusted-headers pattern.
- [API Key Authentication](auth-api-key.md) — for smoke tests and bootstrapping only; not meant for production.

This page covers everything **outside** auth — the DB, process management, TLS, containerization.

## Requirements

- The `senko` binary (build with the `postgres` feature if you'll use PostgreSQL)
- DB: SQLite (smoke test) or PostgreSQL (production)
- **One of the three auth modes** configured — `senko serve` refuses to start without any auth configured
- A reverse proxy for TLS termination — required for production

## PostgreSQL

Recommended for production:

```bash
export SENKO_POSTGRES_URL="postgres://senko:****@db.internal:5432/senko?sslmode=require"
# Configure auth separately — see [OIDC Authentication](auth-oidc.md), etc.
senko serve --host 0.0.0.0 --port 3142
```

Unapplied migrations run on first start.

Connection-pool tuning:

```toml
[backend.postgres]
url = "postgres://..."
max_connections = 20
```

## Injecting Credentials from AWS Secrets Manager

With an `aws-secrets` feature build you can reference the RDS credential by ARN:

```toml
[backend.postgres]
rds_secrets_arn = "arn:aws:secretsmanager:ap-northeast-1:123456789:secret:rds/senko"
```

The ARN is resolved at startup and the resolved value is held only in memory.

For the API-key bootstrap path, `master_key_arn` works the same way (see [API Key Authentication](auth-api-key.md)). In OIDC mode no secret is required for IdP communication, so you typically don't need this setting.

## systemd Unit Example

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

# Hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ReadWritePaths=/var/lib/senko

[Install]
WantedBy=multi-user.target
```

`/etc/senko/env` (OIDC mode example):

```
SENKO_POSTGRES_URL=postgres://senko:****@db.internal:5432/senko?sslmode=require
# Put the OIDC settings in config.toml's [server.auth.oidc] (env overrides also work)
```

Enable it:

```bash
sudo systemctl enable --now senko
sudo journalctl -u senko -f
```

## TLS / Reverse Proxy

senko doesn't do TLS itself; put nginx / Caddy / an API Gateway in front:

```nginx
# nginx example
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

## Running in Docker

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
# In practice you'd mount a config.toml containing [server.auth.oidc],
# or terminate auth in an API Gateway using trusted_headers.
```

## Operations

- **Health check**: `GET /api/v1/health` (no auth, returns 200)
- **Logs**: JSON on stdout — collect with journald / Fluentd / etc.
- **Metrics**: no built-in metrics in v1; wire them via hooks to your own system
- **Backups**: SQLite — snapshot the explicit `[backend.sqlite] db_path` (or `$XDG_DATA_HOME/senko/projects/<dir>/data.db` by default). PostgreSQL — `pg_dump`.
- **Upgrades**: drop in the new binary and restart. Migrations run automatically. Validate against a separate DB in production before rolling out.

## Next Steps

- Enable auth → [OIDC Authentication](auth-oidc.md) (recommended) / [Trusted Headers Authentication](auth-trusted-headers.md)
- Just smoke-test → [API Key Authentication](auth-api-key.md)
- Hooks → [`[server.remote.*]` Hook Examples](hooks.md)
- AWS setup → [AWS Deployment (API Gateway + Cognito + Lambda Web Adapter)](aws-deployment.md)
