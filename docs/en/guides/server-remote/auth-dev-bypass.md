# Dev Bypass (`dev_bypass`) — DEV-ONLY

> **DEVELOPMENT-ONLY. DO NOT DEPLOY ANYWHERE REACHABLE FROM OUTSIDE LOCALHOST.**
>
> Booting with `SENKO_ENV=production` will **refuse to start**. This mode disables all authentication and grants every request **master privileges**. Its only purpose is removing IdP / API-key friction from local dev and Playwright E2E.

## When to use it

- Spinning up the senko remote API for a frontend dev session without standing up a Cognito / Auth0 / Keycloak.
- Running E2E tests that need an authenticated session but don't care about real identity.
- Smoke-testing a senko build without touching `[server.auth.api_key]` / OIDC / Trusted Headers.

For any deployment beyond your laptop, use one of:

- [API Key Authentication](auth-api-key.md) — evaluation / smoke tests
- [OIDC Authentication](auth-oidc.md) — production
- [Trusted Headers Authentication](auth-trusted-headers.md) — production behind an API Gateway

## How to enable

Either via CLI flag:

```bash
senko serve --dev-no-auth
```

Or via `config.toml`:

```toml
[server.auth.dev_bypass]
enabled = true
```

The CLI flag wins — once `--dev-no-auth` is passed, lower-priority sources cannot turn the flag off.

## What it does

When enabled, every inbound request resolves to the **same fixed synthetic user**:

- `id = 1` (matches `DEFAULT_USER_ID` — the row sync_config_defaults guarantees on first boot, so foreign-key writes succeed)
- `username = "dev-bypass"`
- `is_master = true` — bypasses every project-membership check and every master-only endpoint guard

The `AuthUser` extractor never inspects `Authorization` or `x-senko-*` headers. There is no Bearer to send and no session to manage.

`GET /auth/config` returns `auth_mode = "dev_bypass"` so a frontend can render a "DEV MODE" banner.

## What it explicitly does NOT do

- **`POST /auth/token`** returns `501 Not Implemented`. The JWT → API-key exchange would otherwise persist the synthetic user as a real DB row and hand out real session tokens — neither is appropriate for bypass.
- **`/auth/me`** has no associated session (`session: null`).
- **Cannot be combined with `[server.relay]`.** Booting with both set fails fast with a clear error.
- **Cannot be combined with another auth mode** (`api_key` / `oidc` / `trusted_headers`). All four modes are pairwise exclusive.

## Production guard

`validate_serve_auth` runs before every non-relay boot and rejects the combination `dev_bypass.enabled = true` + `SENKO_ENV=production` (case-insensitive, whitespace-trimmed).

```bash
$ SENKO_ENV=production senko serve --dev-no-auth
Error: dev auth bypass cannot be enabled with SENKO_ENV=production. Unset SENKO_ENV or remove [server.auth.dev_bypass] / --dev-no-auth.
```

You will also see a `WARN` log on every boot in bypass mode:

```
WARN dev auth bypass enabled — DO NOT USE IN PRODUCTION
```

It is emitted twice — once at config resolution, once just before "Listening on …" — so operators tailing the log from boot cannot miss it.

## Quick check

```bash
# Boots successfully — note the warning in the log
senko serve --dev-no-auth

# In another shell — no Authorization header needed
curl -sf http://127.0.0.1:3142/auth/config
# → {"auth_mode":"dev_bypass","oidc":null}

curl -sf http://127.0.0.1:3142/auth/me
# → {"user":{"id":1,"username":"dev-bypass",...},"session":null}

# /auth/token is disabled
curl -i -X POST http://127.0.0.1:3142/auth/token -H 'Content-Type: application/json' -d '{}'
# HTTP/1.1 501 Not Implemented
```
