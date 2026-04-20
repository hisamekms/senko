# CLI → Remote → PostgreSQL

Stand up a shared senko server and connect developer CLIs to it. The standard shape for most team deployments.

→ How the three pillars play out in this setup: [Core Concept](../explanation/core-concept.md).

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

## When to Choose This

- **Multiple developers** need to share the same task DB
- You want **CI/CD or bots** to call senko
- You want **SSO-backed** access control
- You want **centralized audit logs**
- You want DB-layer persistence and backups

## Components

| Component | Role | Where secrets live |
|---|---|---|
| senko CLI | Day-to-day client for developers | OS keychain (OIDC session) |
| senko serve (direct) | Central senko server | PostgreSQL credentials |
| PostgreSQL | Persistence layer | (inside the DB) |
| OIDC IdP | SSO authentication | (on the IdP side) |

Authentication options (for user access):

| Mode | Positioning | Details |
|---|---|---|
| **OIDC** | **Recommended for production.** Humans use PKCE; bots use Client Credentials (M2M); both terminate at the same `[server.auth.oidc]` | [OIDC Authentication](../guides/server-remote/auth-oidc.md) |
| Trusted headers | Run behind an API Gateway that verifies the JWT on your behalf | [Trusted Headers Authentication](../guides/server-remote/auth-trusted-headers.md) |
| API key | Smoke tests and early evaluation only | [API Key Authentication](../guides/server-remote/auth-api-key.md) |

## Setup (OIDC Configuration)

A single OIDC config handles both humans (PKCE) and bots (M2M). For the API-key-only evaluation path, see [API Key Authentication](../guides/server-remote/auth-api-key.md).

### Step 1: Provision PostgreSQL

Create the database and user on a separate server (RDS / Aurora / self-hosted):

```sql
CREATE DATABASE senko;
CREATE USER senko WITH PASSWORD '****';
GRANT ALL PRIVILEGES ON DATABASE senko TO senko;
```

Decide on the DB URL for the senko server:

```
postgres://senko:****@db.internal:5432/senko?sslmode=require
```

(Migrations are applied automatically on first start — no prep work required.)

### Step 2: Configure the OIDC IdP

Register **two OAuth clients** on your IdP (Google / Cognito / Keycloak / Auth0, etc.):

**Human users (Public / PKCE)**
- Grant: authorization_code (PKCE)
- Redirect URIs: `http://127.0.0.1:<port>/callback` (must match `callback_ports`)
- Scopes: `openid profile email`
- No client secret

**Bots / service accounts (Confidential / Client Credentials)**
- Grant: client_credentials
- Audience: the senko server URL (e.g. `https://senko.example.com`)
- client_id + client_secret: store in your secret store (CI secrets / Secrets Manager)

Values to keep handy:
- Issuer URL
- Human Client ID
- Bot Client ID (+ put the secret in your secret store)

### Step 3: Start the senko server

On a trusted host, install the `senko` binary (built with the `postgres` feature).

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
client_id      = "senko-cli"                       # human-facing public client used by CLI login
scopes         = ["openid", "profile", "email"]
callback_ports = ["8400", "9000-9010"]
# Setting username_claim to "sub" lets humans and M2M share the same config
username_claim = "sub"

[server.auth.oidc.session]
ttl          = "30d"
inactive_ttl = "7d"
max_per_user = 10

# Optional: grant super-admin via an IdP group claim
# groups_claim = "groups"
# master_group = "senko-admins"

[log]
format = "json"
level  = "info"

# Pipe audit logs to syslog
[server.remote.task_add.hooks.audit]
command = "logger -t senko-audit 'task_add'"
mode = "async"
[server.remote.task_complete.hooks.audit]
command = "logger -t senko-audit 'task_complete'"
mode = "async"
```

> **Note**: only **one** of the three auth modes (`api_key` / `oidc` / `trusted_headers`) can be enabled at a time. Since you picked OIDC, do not configure `[server.auth.api_key]` — the server will refuse to start. If you need super-admin privileges, add `master_group` alongside OIDC.

Run it under systemd:

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

### Step 4: Terminate TLS

senko itself doesn't do TLS; front it with nginx / Caddy / an ALB, etc.:

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

### Step 5: Create a project (self-bootstrap)

In OIDC mode, **the first login JIT-provisions the user**. No master key is required. Two flows:

**Pattern A: small team (no `master_group`)**

1. The designated admin (alice) runs `senko auth login` → a user is JIT-created.
2. alice runs `senko project create --name backend-team` → alice becomes owner of the new project.
3. Other members (bob, carol) each run `senko auth login` to JIT-register themselves.
4. Each member looks up their own `user_id` with `senko auth status` and shares it with alice.
5. alice adds them with `senko project members add --user-id <id> --role member`.

```bash
# alice (admin)
senko auth login
senko project create --name backend-team       # => id=2, alice is owner
senko project members add --user-id 3 --role member

# bob (first-time)
senko auth login
senko auth status                                # note user.id and share it with alice
```

**Pattern B: with a super-admin (`master_group`)**

Set `master_group` in the Step 3 config, and anyone whose JWT belongs to that group becomes a **master**: they can list all users and manage members across every project.

```bash
# admin has logged in via JIT and belongs to master_group
senko user list                                  # enumerate all users
senko project members add --user-id 3 --role member
```

Pick based on scale. If your policy doesn't require a super-admin, Pattern A is plenty.

### Step 6: Configure developer CLIs

Each developer, on their own machine:

```bash
# Install senko
curl -fsSL https://raw.githubusercontent.com/hisamekms/senko/main/install.sh | sh

# In the project
cd your-project
senko skill-install

# Log in (the session token lands in the OS keychain)
senko auth login
```

`.senko/config.toml` (safe to commit):

```toml
[cli.remote]
url = "https://senko.example.com"
# Do not set token — it's retrieved via the keychain.

[project]
name = "backend-team"
```

Verify:

```bash
senko auth status         # who am I logged in as
senko task list           # fetch tasks from the remote DB
```

### Step 7: CI / bots (optional)

Bots authenticate through the same OIDC config using the Client Credentials flow. Grab a JWT from the IdP and inject it as `SENKO_CLI_REMOTE_TOKEN`.

GitHub Actions:

```yaml
env:
  SENKO_CLI_REMOTE_URL: https://senko.example.com
steps:
  - name: Get OIDC access token (M2M)
    run: |
      TOKEN=$(curl -s https://accounts.example.com/oauth/token \
        -H "Content-Type: application/json" \
        -d '{
          "client_id":     "senko-bot",
          "client_secret": "'"${{ secrets.SENKO_BOT_CLIENT_SECRET }}"'",
          "audience":      "https://senko.example.com",
          "grant_type":    "client_credentials"
        }' | jq -r '.access_token')
      echo "SENKO_CLI_REMOTE_TOKEN=$TOKEN" >> $GITHUB_ENV

  - run: senko task list --status todo --output json
```

Bot users are also **JIT-registered on first M2M access** (the username is the JWT `sub`, which is the client_id). After registration, invite the bot into the project:

```bash
# After the bot has made its first call (triggering JIT registration),
# the project owner (alice) adds it:
senko project members add --user-id <bot_user_id> --role member
```

For claim design and handling short-lived JWTs, see [OIDC Authentication](../guides/server-remote/auth-oidc.md#ci--bot-oauth-client-credentials--m2m).

## Security Checklist

- [ ] TLS terminator (nginx / ALB / Cloudflare, etc.) sits in front
- [ ] PostgreSQL credentials come from Secrets Manager or an EnvironmentFile and never appear in logs
- [ ] Only **one** auth mode is enabled (`api_key` / `oidc` / `trusted_headers` are mutually exclusive)
- [ ] OIDC session TTLs (`ttl` / `inactive_ttl`) align with your organization's policy
- [ ] If you use super-admins, `master_group` matches the IdP group mapping
- [ ] Audit hooks under `[server.remote.*]` are in place
- [ ] You have a DB backup plan (`pg_dump` / RDS snapshot)

## Common Issues

| Symptom | What to check |
|---|---|
| `senko auth login` callback fails | Firewall blocking the `callback_ports` range? |
| 401 Unauthorized | Session TTL expired — run `senko auth login` again |
| 403 Forbidden | User not yet added as a project member |
| Migration fails on first start | Does the DB user have `CREATE TABLE` privileges? |

## Deploying on AWS

For the API Gateway + Cognito + Lambda Web Adapter variant, see [AWS Deployment](../guides/server-remote/aws-deployment.md).

## See Also

- Server startup details → [Deploy](../guides/server-remote/deploy.md)
- Auth modes → [API Key Authentication](../guides/server-remote/auth-api-key.md) / [OIDC Authentication](../guides/server-remote/auth-oidc.md)
- Server-side hook examples → [`[server.remote.*]` Hook Examples](../guides/server-remote/hooks.md)
