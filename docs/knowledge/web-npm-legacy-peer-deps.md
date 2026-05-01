# `web/` requires `npm install --legacy-peer-deps`

`web/package.json` declares `typescript: "^6.0.2"`, but
`openapi-typescript@7.13.0` (also a direct dev-dep) declares a peer of
`typescript@^5.x`. npm 7+ enforces strict peer resolution by default, so
plain `npm install` and `npm ci` both fail with:

```
npm error Conflicting peer dependency: typescript@5.9.3
npm error   peer typescript@"^5.x" from openapi-typescript@7.13.0
```

The committed `web/package-lock.json` already resolves both versions via
the legacy algorithm, so the lockfile-honouring command is:

```bash
npm ci --legacy-peer-deps
```

Anything that auto-installs `web/node_modules` (CI, the `mise run web:dev`
launcher in `scripts/bin/web-dev`) must use this flag. A future cleanup
should either downgrade `typescript` to `^5` or wait for an
`openapi-typescript` release that loosens its peer.
