# senko-web Deployment Guide

senko-web is a TanStack Start (SSR) web frontend + BFF. It terminates OIDC authentication (via Auth.js) on the web side and proxies authenticated requests to the senko backend (`senko serve`).

This directory collects the **shared concerns required by every deployment target** (env vars / tarball acquisition / prerequisites) and links out to per-target guides.

## v1 Deployment Targets

| Target | Status | Guide |
| --- | --- | --- |
| AWS Lambda + Amazon Cognito | ✅ v1 supported | [./aws-lambda-cognito.md](./aws-lambda-cognito.md) |
| Container (Docker) | 🚧 Planned | — |
| Vercel / Netlify | 🚧 Planned | — |
| Self-hosted Node (EC2 / VM) | 🚧 Planned | — |

## Architecture Overview

```
Browser
  │
  ▼
senko-web (TanStack Start SSR + Auth.js BFF)
  │   ├─[OIDC]─► OIDC IdP (e.g. Amazon Cognito User Pool)
  │   │
  │   └─[Authorization: Bearer <ID/Access token>]
  ▼
senko backend (`senko serve`, OpenAPI)
```

- **senko-web**: SSR + auth BFF, distributed as a tarball (`senko-web-${VERSION}.tar.gz`)
- **OIDC IdP**: handles login. The v1 sample uses an Amazon Cognito User Pool
- **senko backend**: an already-deployed `senko serve` instance — outside the scope of this guide (see [Related docs](#related-docs))

## Prerequisites

- senko backend (`senko serve`) deployed and reachable over HTTPS ([deploy guide](../server-remote/deploy.md), [AWS deployment example](../server-remote/aws-deployment.md))
- An OIDC IdP (Cognito User Pool / Auth0 / Google, etc.) provisioned ([OIDC auth guide](../server-remote/auth-oidc.md))
- senko-web tarball and senko backend versions match (= same OpenAPI contract). The release workflow co-publishes both under the same `senko vX.Y.Z` tag, so simply pick the **same vX.Y.Z**

## Environment Variables (read by web Lambda)

Canonical list of the environment variables senko-web reads at startup.

| Name | Required | Example | Description |
| --- | --- | --- | --- |
| `SENKO_API_BASE_URL` | ✅ | `https://api.senko.example.com` | HTTPS endpoint of the senko backend (`senko serve`), e.g. an API Gateway URL |
| `AUTH_SECRET` | ✅ | output of `openssl rand -base64 32` | Auth.js session signing/encryption secret (32+ bytes) |
| `AUTH_URL` | ✅ | `https://app.senko.example.com/api/auth` | senko-web public URL + `/api/auth` |
| `AUTH_OIDC_ISSUER` | ✅ | `https://cognito-idp.<region>.amazonaws.com/<user-pool-id>` | OIDC IdP issuer URL |
| `AUTH_OIDC_CLIENT_ID` | ✅ | `(IdP app client ID)` | OIDC application client ID |
| `AUTH_OIDC_CLIENT_SECRET` | ✅ | `(inject from Secrets Manager etc.)` | OIDC application client secret |

> `AUTH_URL` and `AUTH_OIDC_ISSUER` **must use HTTPS**. Passing `http://` triggers fail-fast at startup.

## Downloading and Verifying the Tarball

Each `senko vX.Y.Z` Release on GitHub attaches `senko-web-${VERSION}.tar.gz` and `senko-web-${VERSION}.tar.gz.sha256`.

```bash
# Same version tag as senko itself (example)
SENKO_VERSION="0.42.0"
REPO="hisamekms/senko"
ASSET="senko-web-${SENKO_VERSION}.tar.gz"
BASE="https://github.com/${REPO}/releases/download/v${SENKO_VERSION}"

# Download
curl -fsSL -o "${ASSET}"        "${BASE}/${ASSET}"
curl -fsSL -o "${ASSET}.sha256" "${BASE}/${ASSET}.sha256"

# Verify (Linux: GNU coreutils)
sha256sum -c "${ASSET}.sha256"
# Verify (macOS)
# shasum -a 256 -c "${ASSET}.sha256"

# Extract
tar -xzf "${ASSET}"
# → ./senko-web-${SENKO_VERSION}/ directory is created
```

Extracted directory layout:

- `aws-lambda-handler.mjs` — AWS Lambda entry point (delegates the SSR fetch handler via `srvx/aws-lambda`'s `toLambdaHandler`)
- `package.json` — minimal metadata: `name` / `version` / `type=module` / `private`
- `dist/server/server-entry.js` — TanStack Start SSR build (exports `{ default: { fetch } }`)
- `dist/client/`, `dist/public/` — client assets
- `node_modules/` — runtime dependencies only (staged with `npm ci --omit=dev`)

Approximate size: ~23 MiB (Contract caps the tarball at 50 MiB).

## Per-Target Guides

- [AWS Lambda + Amazon Cognito](./aws-lambda-cognito.md) — officially supported in v1

Other targets (container, Vercel/Netlify, self-hosted Node) are planned for the future.

## Related Docs

- senko backend deployment: [Deploy](../server-remote/deploy.md), [AWS deployment example](../server-remote/aws-deployment.md)
- OIDC authentication: [OIDC auth guide](../server-remote/auth-oidc.md)
