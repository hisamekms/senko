# `WEB_DEV_AUTH_BYPASS` is read once at server start

`WEB_DEV_AUTH_BYPASS` controls two server-side checks in the web app:

1. `web/src/routes/__root.tsx` — `fetchSession()` returns a fake
   `{ user: { name: 'dev-user' }, … }` session (so the `_authed` page gate
   lets protected pages render without an OIDC sign-in).
2. `web/src/routes/api/senko/$.ts` — the BFF proxy skips `getSession()` and
   forwards the request without an `Authorization` header (so the bypassed
   senko `--dev-no-auth` accepts it).

Both checks read `process.env.WEB_DEV_AUTH_BYPASS === 'true'` at request
time, but `process.env` is **populated once when the Vite dev server boots**
and is not influenced by the requesting browser. There is no per-request
cookie, header, or query param that toggles the bypass.

## Implication for E2E

Testing the unauthenticated → `/login` redirect (DoD scenario 8 of task
#400) cannot be done against the bypass-on instance — every request is
"authenticated" with the fake session.

The Playwright suite spins up a *second* Vite dev server on port `3001`
inside `web/tests/e2e/specs/08-auth-redirect.spec.ts` (`beforeAll` /
`afterAll`) with `WEB_DEV_AUTH_BYPASS=false` and dummy `AUTH_*` env vars,
runs the unauthed-redirect assertion against it, and tears it down. The
other 7 specs continue to run against the bypass-on stack on port `3000`
booted by `mise run web:dev`.

## Why not toggle the env var between tests?

Restarting Vite mid-run is slow (cold-start can take 5 s+) and would
serialise the entire suite. A second always-on instance via Playwright's
`webServer` array would also pay the cost on every run, even when
`08-auth-redirect.spec.ts` is filtered out. On-demand spawn keeps the
secondary instance scoped to the one spec that needs it.

## Don't forget the dummy `AUTH_*` env

Auth.js (`@auth/core` via `start-authjs`) crashes if `AUTH_SECRET` is
missing, even on a route that never signs anyone in. The 08 spec sets:

- `AUTH_SECRET` (any 32-byte string)
- `AUTH_URL`
- `AUTH_OIDC_ISSUER` / `AUTH_OIDC_CLIENT_ID` / `AUTH_OIDC_CLIENT_SECRET`

OIDC discovery is lazy, so the unreachable issuer URL is fine — we never
actually trigger a sign-in in the test.
