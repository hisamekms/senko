# Cognito User Pool does not accept the `offline_access` standard scope

## Problem

senko-web's OIDC provider hard-coded the scope set
`openid profile email offline_access` for both the authorization request
(`web/src/utils/auth/index.ts`) and the refresh-token request
(`web/src/utils/auth/refresh.ts`). On any Cognito User Pool + Hosted UI
deployment, sign-in callback fails immediately:

```
GET /api/auth/callback/oidc?error=invalid_request&error_description=invalid_scope
```

Cognito Hosted UI rejects the authorize request at the OAuth-authorize step
before redirecting back, so the failure happens before Auth.js can do
anything about it.

## Root cause

Cognito User Pool's OAuth scope set is a closed list. Per AWS docs the
acceptable values are:

- The built-in scopes `openid`, `profile`, `email`, `phone`, and
  `aws.cognito.signin.user.admin`.
- Custom scopes defined on a Cognito Resource server, written in the
  fully-qualified `<resource-id>/<scope-name>` form.

The bare `offline_access` standard scope from RFC 6749 / OIDC Core is
**not** in either list. Cognito treats it as an unknown scope and returns
`invalid_scope`. This is a documented Cognito limitation, not a senko-web
or Auth.js bug.

## Why dropping `offline_access` is safe on Cognito

Cognito issues a refresh token by default for any app client that has the
`refresh_token` OAuth grant flow enabled — independent of whether
`offline_access` was requested. The refresh-token TTL is set on the app
client itself (default 30 days), not via scope. Therefore senko-web's
refresh-token rotation in `web/src/utils/auth/refresh.ts` continues to
work after dropping `offline_access`.

This is a Cognito-specific behavior. Other OIDC IdPs (Keycloak,
Authentik, Auth0, Entra ID) follow the OIDC spec and require
`offline_access` to be requested for the IdP to mint a refresh token —
those deployments must keep `offline_access` in the scope set.

## Workaround

senko-web exposes `AUTH_OIDC_SCOPES` (added in #429). The env value, if
set, replaces the default scope string in both authorization and refresh
requests. The same value is used in both places to satisfy RFC 6749 §6
(refresh request scope cannot exceed the originally granted set).

```bash
# Cognito + Hosted UI deployments
AUTH_OIDC_SCOPES="openid profile email"
```

When `AUTH_OIDC_SCOPES` is unset, senko-web preserves the historical
default `openid profile email offline_access`, so Keycloak / Authentik /
Auth0 deployments are unaffected.

## References

- [Cognito User Pool — Defining resource servers and custom scopes](https://docs.aws.amazon.com/cognito/latest/developerguide/cognito-user-pools-define-resource-servers.html)
- [Cognito User Pool — App client refresh tokens](https://docs.aws.amazon.com/cognito/latest/developerguide/amazon-cognito-user-pools-using-tokens-with-identity-providers.html#amazon-cognito-user-pools-using-the-refresh-token)
- RFC 6749 §6 — refresh request scope must be a subset of the original grant
- senko-web env reference: [docs/en/guides/web/README.md](../en/guides/web/README.md) / [docs/ja/guides/web/README.md](../ja/guides/web/README.md)
