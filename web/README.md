# senko Web (skeleton)

The Web frontend for senko. This is a TanStack Start application that talks to
the senko remote API. The directory is intentionally placed **outside the Cargo
workspace** so it has no impact on Rust builds or `mise test` / `mise run e2e`.

> Status: Auth.js OIDC BFF and the typed senko API client are wired up. The
> real feature pages (dashboard, task views, contracts, graph) are delivered
> by follow-up sub-tasks of Contract 10.

## Stack

| Concern         | Choice                                  |
| --------------- | --------------------------------------- |
| Framework       | [TanStack Start](https://tanstack.com/start) (Vite-based plugin) |
| Routing         | [TanStack Router](https://tanstack.com/router) (file-based)      |
| Styling         | [Panda CSS](https://panda-css.com/)     |
| UI primitives   | [Ark UI](https://ark-ui.com/)           |
| i18n            | [react-i18next](https://react.i18next.com/) (canonical for this contract) |
| Bundler         | Vite                                    |

## Requirements

- **Node.js**: `>=20` (the devcontainer ships a recent Node — `node --version`).
- **npm**: shipped with Node.

## Setup

The fastest way to bring up the full local stack (senko API + Vite dev
server + sample data) is the bundled mise launcher — see [Combined dev
workflow](#combined-dev-workflow-recommended) below.

For the bare web-only setup (e.g. when you'll point at an existing senko
instance):

```bash
cd web
npm install
npm run dev
```

The dev server starts on **http://localhost:3000** by default. Open it in a
browser; you should see the skeleton page with a working theme toggle and
language switcher.

## Combined dev workflow (recommended)

`mise run web:dev` boots `senko serve --dev-no-auth` and the Vite dev server
together against an isolated dev DB seeded with the standard fixture, so the
dashboard, task views, contract views, and graph all render real data on
first run — no IdP, no manual seeding, no `.env` setup.

```bash
mise run web:dev          # → open http://localhost:3000
mise run web:dev:stop     # tear down both processes
mise run web:dev:reset    # wipe + reseed the dev DB, then start
mise run web:dev:status   # show live/dead state
```

What it does:

1. Builds `senko` with `--features dev-tools` (idempotent).
2. Runs `senko dev seed append` against an isolated dev DB at
   `.local/web-dev/senko.db`. Fresh DB → loads 60 tasks / 5 contracts /
   notes / dependencies / DoD. Already-seeded DB → noop.
3. Starts `senko --db-path … serve --dev-no-auth` on `127.0.0.1:3142`.
   PID + log under `.local/web-dev/`.
4. Starts `vite dev` from `web/` with `WEB_DEV_AUTH_BYPASS=true` and
   `SENKO_API_BASE_URL=http://127.0.0.1:3142` exported, so the BFF proxy
   skips Auth.js and the root route injects a fake dev session.

The dev DB lives at `.local/web-dev/senko.db` — **outside** `.senko/senko.db`,
so `web:dev:reset` cannot wipe real project tasks.

Override knobs:

| Env var          | Default | Effect |
| ---------------- | ------- | ------ |
| `WEB_DEV_PORT`   | `3000`  | Vite dev server port. |
| `SENKO_DEV_PORT` | `3142`  | senko serve port (also rewrites the BFF target). |

```bash
WEB_DEV_PORT=4000 mise run web:dev
SENKO_DEV_PORT=3200 mise run web:dev
```

`mise run web:dev:stop` is idempotent. `web:dev` refuses to start if either
PID file is alive — run `web:dev:stop` first. Logs are tailable at
`.local/web-dev/senko.log` and `.local/web-dev/vite.log`.

## Available scripts

| Script                | Purpose                                              |
| --------------------- | ---------------------------------------------------- |
| `npm run dev`         | Run Panda codegen, then start the Vite dev server.   |
| `npm run build`       | Run Panda codegen, then build for production.        |
| `npm run start`       | Run the production build.                            |
| `npm run typecheck`   | Run `tsc --noEmit`.                                  |
| `npm run panda:codegen` | Run Panda codegen explicitly (writes `styled-system/`). |
| `npm run gen:api`     | Regenerate `src/api/types.gen.ts` from `../docs/openapi/openapi.json`. Run after the senko OpenAPI spec changes. |

## Environment variables

Copy `web/.env.example` to `web/.env` and fill in real values. The TanStack
Start server reads them at runtime.

| Variable                | Default | Description                                              |
| ----------------------- | ------- | -------------------------------------------------------- |
| `WEB_DEV_PORT`          | `3000`  | Port for the Vite dev server (and `vite preview`).        |
| `AUTH_SECRET`           | —       | Auth.js cookie + JWT secret (≥32 bytes; `openssl rand -base64 32`). |
| `AUTH_URL`              | —       | Auth.js base URL including the auth path (e.g. `http://localhost:3000/api/auth`). |
| `AUTH_OIDC_ISSUER`      | —       | OIDC issuer URL of your IdP. Must match the senko backend's IdP. |
| `AUTH_OIDC_CLIENT_ID`   | —       | OIDC client ID for the web app. |
| `AUTH_OIDC_CLIENT_SECRET` | —     | OIDC client secret. |
| `SENKO_API_BASE_URL`    | —       | Origin of `senko serve` (e.g. `http://localhost:8080`). The BFF proxy forwards to it. |
| `WEB_DEV_AUTH_BYPASS`   | `false` | When `true`, both the BFF proxy at `/api/senko/*` and the `_authed` page gate are bypassed: the proxy forwards without an `Authorization` header, and the root route injects a fake dev session so protected pages render without OIDC. Pair with `senko serve --dev-no-auth`. **Local development only — never enable in production.** |

## Authentication & BFF

This app uses [Auth.js](https://authjs.dev/) (`@auth/core` + `start-authjs`,
the official TanStack Start wrapper) with a generic OIDC provider so it can
point at any OIDC IdP (Keycloak, Authentik, Auth0, …) configured via the
`AUTH_OIDC_*` env vars.

**Flow:**

1. The browser visits any protected route — anything under the pathless
   `/_authed` layout (`web/src/routes/_authed/`).
2. The root route's `beforeLoad` calls `getSession()` from `start-authjs` and
   exposes the session through the router context.
3. `_authed` redirects to `/login` when there is no session.
4. The login form POSTs `csrfToken` + `callbackUrl` to
   `/api/auth/signin/oidc`; Auth.js drives the OIDC PKCE round-trip and lands
   the user back on `/`.
5. The OAuth `access_token` is bridged from the OAuth account into the JWT
   and exposed on the session via Auth.js callbacks (`web/src/utils/auth.ts`).
6. Sign-out is the standard Auth.js GET handler at `/api/auth/signout`.

**BFF proxy:** `/api/senko/*` (`web/src/routes/api/senko/$.ts`)

- Reads the session via `getSession(request, authConfig)`. Returns `401` if
  no session or no `access_token`.
- Strips the `/api/senko` prefix and forwards the remaining path + query to
  `${SENKO_API_BASE_URL}`.
- Sets `Authorization: Bearer ${access_token}` and drops the inbound `cookie`
  header so browser cookies are never leaked upstream.
- Streams the upstream response body back unchanged (minus hop-by-hop
  headers).
- When `WEB_DEV_AUTH_BYPASS=true`, the session check is skipped and no
  `Authorization` header is attached — pair with `senko serve --dev-no-auth`
  for a no-login local round-trip. In this mode the root route also injects a
  fake `{ user: { name: 'dev-user' }, expires }` session so the `_authed`
  gate lets the dashboard render without an IdP.

So a browser fetch to `/api/senko/api/v1/projects` ends up at
`${SENKO_API_BASE_URL}/api/v1/projects` with the Bearer attached.

## senko API client (typed)

`src/api/` exposes a TypeScript client over the BFF proxy:

```ts
import { apiClient, paginate, collectAll } from '#/api'

const { data, error } = await apiClient.GET('/api/v1/projects', {
  params: { query: { limit: 20 } },
})

// Async generator over pages — yields each page's items as the cursor advances.
for await (const tasks of paginate<Task>(async (cursor) => {
  const r = await apiClient.GET('/api/v1/projects/{id}/tasks', {
    params: {
      path: { id: projectId },
      query: { after: cursor ?? undefined, limit: 50 },
    },
  })
  if (r.error || !r.data) throw r.error ?? new Error('list_tasks failed')
  return r.data
})) {
  // …consume tasks
}

// Or eagerly collect all pages into one array.
const all = await collectAll<Task>(async (cursor) => /* same shape */)
```

| Module / export                   | Purpose                                              |
| --------------------------------- | ---------------------------------------------------- |
| `apiClient`                       | Default singleton; `baseUrl` is `/api/senko` (the BFF). |
| `createApiClient(options)`        | Factory for custom configs (test fetch, alt baseUrl, custom 401 handler). |
| `paginate(fetchPage)`             | Async generator over `{ items, next_cursor }` pages. |
| `collectAll(fetchPage)`           | Eager helper — flattens all pages into one array.    |
| `paths`, `components`, `operations` | Generated OpenAPI types.                           |

A 401 from the API triggers a redirect to `/login` (browser only); supply
`onUnauthorized` to `createApiClient` to override.

### Generating the API client

The TypeScript types are generated from the senko OpenAPI spec emitted by
sub-task 388 at `docs/openapi/openapi.json` (committed to the repo).

```bash
cd web
npm run gen:api
```

This rewrites `src/api/types.gen.ts` (committed to git so downstream
sub-tasks 392–395 import it without re-running the generator). Re-run
whenever the spec changes — for example after `cargo run -- openapi`. The
runtime helpers (`client.ts`, `pagination.ts`, `index.ts`) are
hand-written and stable across regenerations.

### Local smoke test (no OIDC required)

`mise run web:dev` is the one-command equivalent of the recipe below — see
[Combined dev workflow](#combined-dev-workflow-recommended). Use the manual
three-terminal version when you want to drive the senko binary or the Vite
dev server with non-default flags:

```bash
# Terminal A — senko API with bypass mode (default port 3142)
cargo run --bin senko -- serve --dev-no-auth

# Terminal B — web dev server with BFF bypass
cd web
WEB_DEV_AUTH_BYPASS=true \
SENKO_API_BASE_URL=http://localhost:3142 \
  npm run dev

# Terminal C
curl -fsS http://localhost:3000/api/senko/api/v1/projects | jq '.items, .next_cursor'
```

Expect HTTP `200` and a JSON body shaped `{ items: [...], next_cursor: ... }`.

## Local OIDC setup (Keycloak)

Any OIDC IdP works, but Keycloak in Docker is a quick way to get going:

```bash
# Start Keycloak in dev mode on :8081
docker run --rm -p 8081:8080 \
  -e KEYCLOAK_ADMIN=admin -e KEYCLOAK_ADMIN_PASSWORD=admin \
  quay.io/keycloak/keycloak:latest start-dev
```

Then in the admin console (`http://localhost:8081`):

1. Create a realm `senko`.
2. In `Clients`, create `senko-web` (OpenID Connect, confidential).
3. Set **Valid redirect URIs** to `http://localhost:3000/api/auth/callback/oidc`.
4. In `Credentials`, copy the client secret.
5. In `Client Scopes` → `senko-web`, ensure `offline_access` is listed under
   **Default** or **Optional Client Scopes** (Keycloak ships it as a default
   realm-level scope, but new clients sometimes need it added explicitly).
   This is required for refresh-token rotation — see
   [IdP requirements](#idp-requirements) below.
6. Create a user, set a password, log in once at the account console to
   prime it.

Use these values in `web/.env`:

```
AUTH_OIDC_ISSUER=http://localhost:8081/realms/senko
AUTH_OIDC_CLIENT_ID=senko-web
AUTH_OIDC_CLIENT_SECRET=<from Keycloak credentials>
```

Configure `senko serve` to trust the same issuer (see top-level
`docs/...`).

### IdP requirements

The web app asks the IdP for a few capabilities at signin time. If the IdP is
not configured to grant them, the corresponding flow falls back gracefully but
some features stop working.

- **`offline_access` scope** — required for **refresh-token rotation**
  (`web/src/utils/auth/refresh.ts`). The web app requests scope
  `openid profile email offline_access` from the authorization endpoint. When
  the IdP grants `offline_access`, the OIDC provider returns a `refresh_token`
  alongside the `access_token`, and the Auth.js `jwt` callback transparently
  refreshes the access token via the IdP's `token_endpoint` once the previous
  one is within 60 seconds of expiry. Without this scope the app **still
  works**, but every short-lived `access_token` expiry forces the user back
  to `/login`.
  - **Keycloak**: enabled by default at the realm level. Make sure the client's
    `Client Scopes` tab lists `offline_access` under Default or Optional.
  - **Auth0**: in the application settings, enable **Allow Offline Access**
    (under APIs → your API), and tick **Allow Offline Access** for the
    application's grant types.
  - **Authentik**: add `offline_access` to the provider's `Scopes` list.

  When refresh fails (e.g. the IdP revoked the refresh token), the web app
  surfaces this as `session.error = 'RefreshAccessTokenError'`. The protected
  routes redirect to `/login` and the BFF (`/api/senko/*`) returns
  `401 {error: "RefreshAccessTokenError"}`.

- **`post_logout_redirect_uri`** — required for **RP-Initiated Logout**
  (`web/src/routes/api/auth/$.ts`). Register `http://localhost:3000/login` (or
  your production equivalent) as a Valid post-logout redirect URI on the IdP
  client. Without it, sign-out still clears the local session cookie but the
  browser stays on the IdP-side logout page instead of returning to `/login`.

### End-to-end smoke test

1. Start the IdP and `senko serve` (configured against the same issuer).
2. `cp web/.env.example web/.env` and fill in real values.
3. `npm run dev` (from `web/`).
4. Visit `http://localhost:3000/` — you should be redirected to `/login`.
5. Click **Sign in with OIDC** → IdP login → land back on `/`.
6. Open devtools and run:
   ```js
   await fetch('/api/auth/session').then(r => r.json())
   ```
   Confirms a session JSON is returned. Without a session it returns `{}`.
7. Hit a senko endpoint via the BFF, e.g.
   ```js
   await fetch('/api/senko/api/v1/projects').then(r => r.status)
   ```
   `200` confirms the Bearer made it through.
8. Click **Sign out** → redirected; `/api/auth/session` again returns `{}`,
   `/` redirects back to `/login`.

## Project layout

```
web/
├── src/
│   ├── api/                      # Typed senko API client
│   │   ├── client.ts             # createApiClient + 401 → /login middleware
│   │   ├── pagination.ts         # paginate / collectAll over { items, next_cursor }
│   │   ├── types.gen.ts          # Generated by `npm run gen:api`; committed
│   │   └── index.ts              # Public re-exports
│   ├── routes/                   # File-based TanStack Router routes
│   │   ├── __root.tsx            # Shell + session beforeLoad
│   │   ├── _authed.tsx           # Pathless auth gate (redirects to /login)
│   │   ├── _authed/index.tsx     # Authenticated home
│   │   ├── login.tsx             # OIDC sign-in form
│   │   └── api/
│   │       ├── auth/$.ts         # Auth.js handlers (/api/auth/*)
│   │       └── senko/$.ts        # BFF proxy to senko (Bearer-attached)
│   ├── components/               # Reusable UI components (theme/lang switchers)
│   ├── hooks/                    # Reusable hooks (e.g. useTheme)
│   ├── i18n/                     # react-i18next setup; canonical entrypoint
│   │   ├── index.ts
│   │   └── locales/{en,ja}.json
│   ├── utils/auth.ts             # Auth.js (StartAuthJSConfig) + OIDC provider
│   ├── router.tsx                # TanStack Router instance factory
│   └── styles.css                # Panda CSS @layers entry point
├── .env.example                  # Sample environment variables
├── panda.config.ts               # Panda CSS preset, tokens, dark-mode condition
├── postcss.config.cjs            # Wires Panda's PostCSS plugin
├── vite.config.ts                # TanStack Start + WEB_DEV_PORT integration
└── tsconfig.json
```

`styled-system/` is generated by `panda codegen` and is gitignored; it must
exist before `vite dev` / `vite build` (the `dev` and `build` scripts run
codegen automatically).

## Internationalization

i18n is initialized in `src/i18n/index.ts` and is the **canonical entrypoint**
for all sub-tasks of Contract 10. Add new strings to `src/i18n/locales/en.json`
and `src/i18n/locales/ja.json`. Use `useTranslation()` from `react-i18next` in
components.

The default language is `en`. The browser language detector reads
`localStorage` (key `senko.web.lng`) and falls back to the navigator language.

## Dark mode

The theme is held on `document.documentElement.dataset.theme` (`"light"` or
`"dark"`). Panda CSS targets it via the `_dark` condition (defined in
`panda.config.ts` as `[data-theme="dark"] &`). User preference is persisted
in `localStorage` (key `senko.web.theme`). When no preference is stored, the
`prefers-color-scheme` media query is followed.

A small inline script in the document `<head>` (defined in
`src/routes/__root.tsx`) sets `data-theme` before hydration to avoid a flash of
unstyled / wrong-themed content. Sub-tasks adding new components should style
them using Panda's `_dark` condition.

## End-to-end tests (Playwright)

Browser-driven E2E coverage for the v1 surface lives under `web/tests/e2e/`.
The suite exercises the dashboard, project switcher, task list/detail,
contract list/detail/notes, dependency graph, i18n switching, dark mode
toggle, and authenticated/unauthenticated redirect behavior.

### One-time setup

```bash
mise run web:e2e:install        # downloads chromium browser binaries (~150 MB)
```

This is also what CI does (cached between runs).

### Running the suite

```bash
mise run web:e2e                # headless run, html report at web/playwright-report/
mise run web:e2e:ui             # headed UI mode (interactive)
mise run web:e2e:report         # open the most recent HTML report
```

The bundled `globalSetup` boots `mise run web:dev` (senko serve `--dev-no-auth`
on :3142 + Vite on :3000 with `WEB_DEV_AUTH_BYPASS=true`) against the same
isolated dev DB at `.local/web-dev/senko.db` that manual exploration uses,
so **don't run a manual `mise run web:dev` session in parallel** — the suite
will refuse to start (port 3000 / 3142 already in use) or worse, share a DB
that you might be inspecting.

`globalTeardown` stops both processes when the run ends.

### Knobs

| Env var       | Default | Effect                                                                                         |
| ------------- | ------- | ---------------------------------------------------------------------------------------------- |
| `E2E_FRESH`   | unset   | Force `web:dev:reset` even if the stack is already running. CI sets this to start from scratch. |
| `E2E_KEEP`    | unset   | Skip `globalTeardown`; useful for poking at the stack with `mise run web:dev:status` after a run. |

### Auth-redirect spec — second Vite

`08-auth-redirect.spec.ts` cannot toggle `WEB_DEV_AUTH_BYPASS` per-request
(it is read once at server start) so it spawns its own Vite on port `3001`
with bypass off in `beforeAll`, kills it in `afterAll`, and runs against
that. The other 7 specs pay no startup cost for this.

### Failure artifacts

On a failed run:

- `web/playwright-report/` — HTML report (`mise run web:e2e:report` opens it).
- `web/test-results/` — per-test trace, screenshot on failure, and video on
  retry. CI uploads both as the `playwright-artifacts` workflow artifact.

### Why the Playwright suite is independent of the Rust e2e umbrella

`mise run e2e` runs the bash + senko CLI driven Rust e2e tests. The
Playwright suite has different runtime requirements (Node + chromium) and
gates a different surface (the web app). Keep them separate; CI runs them
in parallel jobs.
