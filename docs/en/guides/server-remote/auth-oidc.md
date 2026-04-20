# OIDC Authentication

An auth mode where senko accepts OAuth 2.0 / OIDC JWTs as Bearer tokens. Designed for use behind an internal SSO / Google / Cognito / Keycloak / Auth0 / etc.

> **Recommended for production.** Humans obtain JWTs via **OAuth Authorization Code + PKCE**; CI / bots / service accounts obtain them via **OAuth Client Credentials (M2M)**. Both send their JWTs to senko, and senko only does JWT verification — a single `[server.auth.oidc]` config handles both paths. API key auth should be kept strictly to evaluation scenarios.

## How It Works

```
CLI ── senko auth login ──┐
                          ├── Browser redirects to the IdP
                          ├── User logs in → PKCE exchange
                          └── senko receives the JWT, issues an internal
                              API key, and stores it in the OS keychain

Subsequent senko commands pull the key from the keychain and send it as Bearer.
```

The CLI doesn't keep sending the JWT — **JWT verification happens exactly once, and from then on senko uses its internally issued API key**.

## Server Config

```toml
[server.auth.oidc]
issuer_url = "https://accounts.example.com"
client_id  = "senko-cli"
scopes     = ["openid", "profile", "email"]
# username_claim = "preferred_username"   # falls back to sub when unset
# groups_claim   = "groups"               # claim used for master_group check (default "groups")
# master_group   = "senko-admins"         # JWTs belonging to this group get is_master=true
# required_claims = { email_verified = "true" }
callback_ports = ["8400", "9000-9010"]    # local callback port candidates opened during CLI login

[server.auth.oidc.session]
ttl          = "30d"    # absolute TTL
inactive_ttl = "7d"     # inactivity timeout
max_per_user = 10       # per-user session cap
```

- `issuer_url` must serve `.well-known/openid-configuration`.
- Register `client_id` on the IdP as a "Public client / PKCE" (no secret needed).
- `callback_ports` are **ports opened on the CLI user's machine**. Specify individual ports or a range.

> **Mutually exclusive with other modes**: `[server.auth.api_key]` (`master_key`) / `[server.auth.oidc]` / `[server.auth.trusted_headers]` cannot be enabled simultaneously. If you picked OIDC, don't set `master_key`.

## JIT User Registration

In OIDC mode, **users are auto-created on first authentication**. You don't pre-issue users.

- JWT's `sub` → `users.sub`
- The claim named by `username_claim` becomes `username` (default priority: `preferred_username` → `email` → `sub`)
- `name` / `email` claims also populate `display_name` / `email` when present

A first-time login is not a member of any project yet, so all the newly provisioned user can do is:

- Read its own profile (`/auth/me`)
- **Create a new project** (`POST /api/v1/projects`; the creator becomes owner)
- If its JWT carries `master_group`, manage anything everywhere (below)

This gives OIDC **a self-bootstrap path** — the first user logs in, creates their project, invites others as members. No master key required.

## Master Privilege: `master_group`

In OIDC mode, master privilege is granted via a **group claim** (a different mechanism from the `master_key` used by API key mode):

```toml
[server.auth.oidc]
groups_claim = "groups"              # which claim to inspect (default "groups")
master_group = "senko-admins"        # users in this group get is_master=true
```

- `groups_claim` names a **string-array** claim in the JWT. For Cognito it's `cognito:groups`; for Auth0 you map a claim to `groups`.
- If the array contains `master_group`, the caller is `is_master=true`.
- `is_master=true` users **bypass membership checks on every project** and can call `POST /api/v1/users` (user CRUD).

OIDC works fine without `master_group`. In that case there's no master; operation scales per project-owner and — for many teams — that's plenty.

## IdP Setup

### For human users (PKCE)

Register a public OAuth client:

- **Grant**: authorization_code (PKCE)
- **Redirect URIs**: `http://127.0.0.1:<port>/callback` (matching `callback_ports`)
- **Scopes**: `openid profile email`
- Client secret: not needed

### For bots / service accounts (Client Credentials / M2M)

Register a separate confidential (machine-to-machine) client:

- **Grant**: client_credentials
- **Audience** (Auth0 etc.): the senko server URL
- **Scopes**: optional (senko itself doesn't inspect scopes, but you can still use them for IdP-side access control)
- **client_id + client_secret**: keep in the bot's secret store (CI secrets / Secrets Manager)

A single `[server.auth.oidc]` on the senko side handles both paths — match the issuer / audience / required_claims. Just pick `username_claim` / `required_claims` that **also hold for M2M tokens** (see below).

## Client Side (CLI)

```toml
# .senko/config.toml
[cli.remote]
url = "https://senko.example.com"
# Do NOT set token — it comes from the keychain
```

First login:

```bash
senko auth login [--device-name "alice-laptop"]
```

What happens:

1. A browser opens (or, with `[cli] browser = false`, a URL is printed to stdout).
2. User authenticates with the IdP.
3. CLI receives the callback and exchanges the code via PKCE for a JWT.
4. senko verifies the JWT and returns a newly issued internal API key.
5. CLI stores that API key in the OS keychain.

Afterwards:

```bash
senko auth status     # current login info
senko auth sessions   # list issued sessions (= internal API keys)
senko auth logout     # revoke the current session + remove the key from the keychain
senko auth revoke <id>        # revoke another device's session
senko auth revoke --all       # revoke everything
```

## What's in the Keychain

- macOS: Keychain Access → service `senko`
- Linux: libsecret / gnome-keyring entry `senko`
- Windows: Credential Manager `senko`

## CI / Bots (OAuth Client Credentials / M2M)

`senko auth login` is an interactive flow and can't be used in CI / headless environments. Fetch a JWT directly from the IdP using Client Credentials and inject it as `SENKO_CLI_REMOTE_TOKEN`.

```bash
# Example: fetch a JWT from Auth0
JWT=$(curl -s https://accounts.example.com/oauth/token \
  -H "Content-Type: application/json" \
  -d '{
    "client_id":     "senko-bot",
    "client_secret": "'"$SENKO_BOT_CLIENT_SECRET"'",
    "audience":      "https://senko.example.com",
    "grant_type":    "client_credentials"
  }' | jq -r '.access_token')

# Send to senko
export SENKO_CLI_REMOTE_URL="https://senko.example.com"
export SENKO_CLI_REMOTE_TOKEN="$JWT"
senko task list
```

On GitHub Actions, store `client_secret` in a secret and run the step above at the start of the job.

### Claim design gotchas

Human JWTs and M2M JWTs have different claims, so the `username_claim` / `required_claims` you pick must hold for both:

| Aspect | Human JWT | M2M JWT |
|---|---|---|
| `sub` | IdP-specific user ID | client_id |
| `email` / `email_verified` | present | **absent** |
| `preferred_username` / `name` | present | absent (depends on IdP) |
| Custom claims (`username`, `service`, etc.) | depends on IdP mapping | depends on IdP mapping |

- `username_claim = "sub"` is the simplest choice. For M2M, the username ends up being the client_id (e.g. `senko-bot`).
- Don't add human-assuming `required_claims` like `{ email_verified = "true" }` — they'll reject M2M tokens.
- If you need to distinguish humans from bots, map a custom IdP claim (e.g. `"type": "service"`) and adjust project roles after JIT registration.

### Short-lived JWT issue

Client Credentials access tokens typically expire in ~1 hour. Long-running jobs must re-fetch:

- Re-fetch at each job step (fine for short jobs).
- For multi-hour jobs, write a small bash helper that inspects the `exp` claim and re-fetches when less than 5 minutes remain.
- If you truly need long-lived tokens, consider limited use of [API Key Authentication](auth-api-key.md).

## Session Management

Server-side, senko distinguishes OIDC-derived API keys as "sessions":

- Expire when `[server.auth.oidc.session] ttl` passes (login required).
- Expire after `inactive_ttl` from last use.
- The oldest sessions are revoked when `max_per_user` is hit.

## Cannot Combine with Trusted Headers

`[server.auth.oidc]` and `[server.auth.trusted_headers]` cannot coexist. For an API-Gateway-terminated OIDC setup, use `trusted_headers` instead ([Trusted Headers Authentication](auth-trusted-headers.md)).

## Troubleshooting

| Symptom | What to check |
|---|---|
| `senko auth login` doesn't open a browser | For headless machines, set `[cli] browser = false` and copy the URL manually |
| Connection refused on the callback | Is the `callback_ports` range blocked by your firewall? |
| Login succeeds but API returns 401 | Does `username_claim` match what the IdP puts in the JWT? |
| Forced to re-login constantly | `[server.auth.oidc.session] ttl` / `inactive_ttl` too short? |
| You want to map IdP groups/roles to senko permissions | No built-in mapping. Add members by hand or use `required_claims` to scope access |

## Next Steps

- Terminate OIDC in an API Gateway (e.g. Cognito) and have senko receive trusted headers → [Trusted Headers Authentication](auth-trusted-headers.md) and [AWS Deployment (API Gateway + Cognito + Lambda Web Adapter)](aws-deployment.md)
