# senko Web (skeleton)

The Web frontend for senko. This is a TanStack Start application that talks to
the senko remote API. The directory is intentionally placed **outside the Cargo
workspace** so it has no impact on Rust builds or `mise test` / `mise run e2e`.

> Status: Auth.js OIDC BFF is wired up. The senko API client (typed) and the
> real feature pages are delivered by follow-up sub-tasks of Contract 10.

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

```bash
cd web
npm install
npm run dev
```

The dev server starts on **http://localhost:3000** by default. Open it in a
browser; you should see the skeleton page with a working theme toggle and
language switcher.

## Available scripts

| Script                | Purpose                                              |
| --------------------- | ---------------------------------------------------- |
| `npm run dev`         | Run Panda codegen, then start the Vite dev server.   |
| `npm run build`       | Run Panda codegen, then build for production.        |
| `npm run start`       | Run the production build.                            |
| `npm run typecheck`   | Run `tsc --noEmit`.                                  |
| `npm run panda:codegen` | Run Panda codegen explicitly (writes `styled-system/`). |

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

`WEB_DEV_AUTH_BYPASS` is reserved for the dev-bypass path introduced in a
separate sub-task (see Contract 10) and is not consumed here.

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

So a browser fetch to `/api/senko/api/v1/projects` ends up at
`${SENKO_API_BASE_URL}/api/v1/projects` with the Bearer attached.

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
5. Create a user, set a password, log in once at the account console to
   prime it.

Use these values in `web/.env`:

```
AUTH_OIDC_ISSUER=http://localhost:8081/realms/senko
AUTH_OIDC_CLIENT_ID=senko-web
AUTH_OIDC_CLIENT_SECRET=<from Keycloak credentials>
```

Configure `senko serve` to trust the same issuer (see top-level
`docs/...`).

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

## What is NOT here yet

Still deferred to follow-up sub-tasks of Contract 10:

- Typed senko API client (generated from OpenAPI; the BFF proxy itself is in
  place — sub-task 391 generates the typed wrappers that consume it)
- Real screens (dashboard, tasks, contracts, graph): sub-tasks 392–395
- Combined dev command (`senko serve` + web + seeder): sub-task 399
- Playwright E2E suite: sub-task 400
