# API Key Authentication

Simple Bearer token authentication.

> **Positioning**: API key authentication is **for evaluation / smoke tests only**. For production, use [OIDC Authentication](auth-oidc.md) for both humans (OAuth Authorization Code + PKCE) and bots (OAuth Client Credentials = M2M).
>
> - The three auth modes (`api_key` / `oidc` / `trusted_headers`) can be enabled **only one at a time** — configuring more than one is a startup error.
> - So `master_key` is a concept **only for API key mode**. To grant master privileges under OIDC or trusted_headers, use `master_group` (a group claim) instead (see [OIDC Authentication](auth-oidc.md)).

## Setup

### 1. Generate a master key

```bash
MASTER_KEY=$(openssl rand -base64 32)
export SENKO_AUTH_API_KEY_MASTER_KEY="$MASTER_KEY"
senko serve --host 0.0.0.0 --port 3142
```

Or via config:

```toml
[server.auth.api_key]
master_key = "..."
# Or:
# master_key_arn = "arn:aws:secretsmanager:..."
```

**What a master key is**: a privileged key not tied to any User. It's used for bootstrap operations like user creation (`POST /api/v1/users`). **Don't use it for normal API traffic.**

### 2. Create a user with the master key

```bash
curl -s -X POST https://senko.example.com/api/v1/users \
  -H "Authorization: Bearer $MASTER_KEY" \
  -H "Content-Type: application/json" \
  -d '{"username":"alice"}' | jq .
# {"id": 2, "username": "alice", ...}
```

### 3. Issue an API key for that user

```bash
curl -s -X POST https://senko.example.com/api/v1/users/2/api-keys \
  -H "Authorization: Bearer $MASTER_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name":"default"}' | jq .
# {"id": 3, "key": "sk_abc123...", "key_prefix": "sk_ab", ...}
```

**`key` is returned only once at issue time**. If you lose it, reissue is the only recovery — store it safely.

### 4. Configure the client

Use the issued API key:

```bash
export SENKO_CLI_REMOTE_URL="https://senko.example.com"
export SENKO_CLI_REMOTE_TOKEN="sk_abc123..."
senko task list
```

Or put it in `.senko/config.local.toml` (gitignored):

```toml
[cli.remote]
url = "https://senko.example.com"
token = "sk_abc123..."
```

## Managing the Master Key

- **Don't expose it to the Internet.** Inject it from Secrets Manager at issue time.
- **Rotation**: with `master_key_arn`, rotate on the Secrets Manager side → restart the server.
- **No revocation mechanism**: a master key itself can't be revoked. On leak, replace it with a new value and redistribute. Unlike normal API keys, it isn't stored in the DB.

## Revoking API Keys

```bash
# List a user's API keys
curl -s -H "Authorization: Bearer $MASTER_KEY" \
  https://senko.example.com/api/v1/users/2/api-keys | jq .

# Delete a specific key
curl -s -X DELETE -H "Authorization: Bearer $MASTER_KEY" \
  https://senko.example.com/api/v1/users/2/api-keys/3
```

Or use `senko auth revoke <id>` for your own keys.

## Master Key vs. Normal API Key

| | Master key | API key |
|---|---|---|
| Bound to a user | No | Yes |
| Stored in DB | No (config / env only) | Yes (as a hash in `api_keys`) |
| `POST /api/v1/users` | Allowed | **Denied** |
| Project membership checks | Bypasses them | Follows role |
| Revocation | Not directly (replace the value) | Delete from DB |

## Operational Tips

- **Issue per device**: each developer gets a separate API key per machine (`name = "alice-laptop"`, `"alice-ci"`, etc.). Narrows blast radius on loss.
- **Treat the master key as "startup-only"**: once you've created the first user and their API key, stop using the master key.
- **Leak prevention**: make sure `Authorization: Bearer ...` doesn't end up in logs.

## Troubleshooting

| Symptom | Cause | What to do |
|---|---|---|
| 401 Unauthorized | Token invalid / revoked | Check `senko auth status` or list the keys |
| 403 Forbidden | Authenticated but not a member of the target project | Add as member or have the owner invite you |
| `[server.auth.api_key]` not activating | Neither `master_key` nor `master_key_arn` is set, or env typo | Run `senko config` to confirm |

## Next Steps

- Move to OIDC → [OIDC Authentication](auth-oidc.md)
- Use behind an API Gateway → [Trusted Headers Authentication](auth-trusted-headers.md)
