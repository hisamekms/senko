# Trusted Headers (`trusted_headers`) Authentication

A setup where an API Gateway or reverse proxy has **already handled authentication/authorization and injects the user identity via headers**. senko itself does no token verification and **trusts the header values unconditionally**.

## ⚠️ Read This First: Security Warning

**Do NOT expose a senko running in `trusted_headers` mode directly to the Internet.**

- senko trusts `x-senko-user-sub` and friends without verification.
- If a client can send those headers directly, **they can impersonate any user**.
- The API Gateway / reverse proxy must be the **sole entry point**, and it must strip any client-supplied `x-senko-*` before forwarding.

## Basic Shape (AWS API Gateway + Cognito)

```
Client ──[Bearer JWT]──> API Gateway (HTTP API)
                            │
                            ├─ Cognito JWT Authorizer verifies the JWT
                            ├─ Parameter Mapping turns JWT claims into x-senko-* headers
                            ▼
                          senko serve (trusted_headers mode)
```

Full walkthrough: [AWS Deployment (API Gateway + Cognito + Lambda Web Adapter)](aws-deployment.md).

## Server Config

Setting `subject_header` **enables** `trusted_headers` mode:

```toml
[server.auth.trusted_headers]
subject_header      = "x-senko-user-sub"       # required; carries the sub
name_header         = "x-senko-user-name"
display_name_header = "x-senko-user-display-name"
email_header        = "x-senko-user-email"
groups_header       = "x-senko-user-groups"
scope_header        = "x-senko-user-scope"

# Fallback for CLI login (returned from GET /auth/config)
oidc_issuer_url     = "https://cognito-idp.ap-northeast-1.amazonaws.com/ap-northeast-1_XXXXX"
oidc_client_id      = "xxxxxxxx"
```

- Only `subject_header` is required; the rest are optional.
- If `name_header` is absent but `display_name_header` is present, the latter is used as fallback.
- `oidc_*` is what `GET /auth/config` returns when the CLI asks for login metadata. Set these when you want users to authenticate via the CLI's OIDC flow.

## Client Side

Ordinary clients (the CLI, tools) **authenticate via OIDC in front of the API Gateway** and attach the JWT as Bearer when calling the Gateway. They don't talk to senko directly.

`senko auth login` fetches the OIDC metadata from `GET /auth/config`, redirects to the IdP, and sends the obtained JWT as Bearer to the Gateway (Gateway verifies → injects headers → senko receives them).

## Which User Is It?

1. On each request, senko reads the `subject_header` value (e.g. the JWT's `sub`).
2. Looks it up in the DB as `users.sub`.
3. If there's a matching user, authentication is done.
4. Otherwise, senko **JIT-provisions** a user:
   - `username` = `name_header` (or `sub` as fallback)
   - `display_name` = `display_name_header` / `name_header`
   - `email` = `email_header`

## Authorization (membership / role)

A user authenticated via `trusted_headers` **cannot operate on resources without being a project member**. Any authenticated user can, however, create a new project with `POST /api/v1/projects` (the creator becomes owner), so — like OIDC mode — this supports **self-bootstrap**.

For "super-admin" privileges across all projects, use `master_group`:

```toml
[server.auth.trusted_headers]
subject_header = "x-senko-user-sub"
groups_header  = "x-senko-user-groups"     # comma-separated group names
master_group   = "senko-admins"            # members of this group get is_master=true
```

- senko parses `groups_header` as **comma-separated** values; if one matches `master_group`, the caller is `is_master=true`.
- `is_master=true` users **bypass project membership checks everywhere** and can call master-only endpoints like `POST /api/v1/users`.

Configure the API Gateway's Parameter Mapping so JWT claims (`cognito:groups`, `roles`, etc.) flow into `x-senko-user-groups`.

## Mutually Exclusive With Other Auth Modes

**Only one of the three auth modes (`api_key` / `oidc` / `trusted_headers`) can be enabled at a time** — senko checks the mutual exclusion at startup and bails if violated. Combining `trusted_headers` with `[server.auth.api_key] master_key` is a **startup error**. To grant master privileges under `trusted_headers`, use `master_group` as shown above.

## Troubleshooting

| Symptom | What to do |
|---|---|
| Every request returns 401 | Confirm the API Gateway is injecting `x-senko-*` — inspect the Parameter Mapping |
| Users being impersonated | Make sure there's no path to senko other than via the API Gateway (security groups, etc.) |
| JIT registration doesn't happen | `subject_header` might be empty — check the API Gateway logs for the header value |
| `senko auth login` doesn't work | `oidc_issuer_url` / `oidc_client_id` unset, or PKCE disabled on the IdP side |

## Next Steps

- Full AWS walkthrough → [AWS Deployment (API Gateway + Cognito + Lambda Web Adapter)](aws-deployment.md)
