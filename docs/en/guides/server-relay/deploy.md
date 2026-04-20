# Deploying `senko serve` in Relay Mode

A relay server has no DB; it's a thin HTTP forwarder to an upstream direct server. There's no dedicated flag to start in relay mode — starting `senko serve` with `[server.relay] url` set (env: `SENKO_SERVER_RELAY_URL`) automatically enters relay mode.

> **Key prerequisite: the relay does no inbound authentication.**
>
> `senko serve` in relay mode starts with `auth_mode: None` hard-coded; `[server.auth.*]` is **not read and is ignored**. Incoming requests skip authentication and are forwarded to the upstream. The relay must therefore run on a **closed network (sandbox-only network / VPN / loopback, etc.)**, and limiting reachability is what effectively authorizes callers.
>
> Never run the relay on a public network. If you need to expose it, put a reverse proxy (nginx with IP allowlist / mTLS / a separate API Gateway) in front to handle authorization.

When to use each runtime: [Choosing a Runtime](../../explanation/runtimes.md).

## Typical Use Cases

1. **Concentrate upstream access for an AI sandbox**
   - The sandbox has egress restrictions and can only reach the relay.
   - The relay holds the upstream credential (session API key or M2M JWT) and forwards on behalf of the sandbox.
2. **Closed-network client → external upstream**
   - A bridge from an internal network to a senko server acting as external SaaS.
   - The relay concentrates audit logs and egress governance.
3. **Single point of upstream authentication**
   - Several small clients avoid holding individual credentials; the relay handles it.
   - The relay itself doesn't authenticate — keep the entrance locked down via network boundary or an external proxy.

Not good fits:

- Public-internet-facing inbound (no auth mechanism, instant breach).
- Multi-tenant auth separation (a single relay can't distinguish inbound tenants).

## Minimum Setup

```bash
# Upstream server and the token it accepts
export SENKO_SERVER_RELAY_URL="https://senko-upstream.example.com"
export SENKO_SERVER_RELAY_TOKEN="<Bearer value accepted by the upstream>"

# Start the relay (listen inside the closed network)
#   Since SENKO_SERVER_RELAY_URL is set, relay mode kicks in.
senko serve --host 127.0.0.1 --port 3142
```

Via config file:

```toml
[server]
host = "127.0.0.1"     # reachable only from within the closed network
port = 3142

[server.relay]
url   = "https://senko-upstream.example.com"
token = "<Bearer value accepted by the upstream>"
```

> Writing `[server.auth.*]` or `[backend.*]` is ignored in relay mode. It's not a startup error, but keep the relay config minimal to avoid confusion.

## Behavior

The relay processes each incoming HTTP request as follows:

1. **No auth check** — `auth_mode` is None, so the request passes unconditionally.
2. Decide what Authorization header to send upstream:
   - `[server.relay] token` set → **rewrite with this token** (substitution mode).
   - Unset → **passthrough the client's Authorization header as-is** (passthrough mode).
3. Forward the upstream response to the client.
4. Fire any `[server.relay.<action>.hooks.<name>]` after the upstream call succeeds.

## Substitution Mode (`token` is set)

```toml
[server.relay]
url   = "https://senko-upstream.example.com"
token = "<Bearer value accepted by the upstream>"
```

What `token` should contain depends on the upstream's auth mode:

- Upstream is OIDC: **a session API key issued by senko** (obtained via `senko auth login` + `senko auth token`; TTL governed by `[server.auth.oidc.session]`) or **an M2M JWT fetched directly from the IdP** (expires per the IdP's access_token_lifetime).
- Upstream is trusted_headers: **a JWT accepted by the API Gateway** (the IdP's access_token passed through).
- Upstream is API key: **a normal API key issued via master_key** (long-lived, evaluation-grade).

Behavior:

- **Client credentials never reach the upstream** (the relay drops them).
- From the upstream's perspective, the relay sends every request as one identity.
- Upstream logs don't retain individual client info, so **audit logging must live on the relay side** ([`[server.relay.*]` Hook Examples](hooks.md)).

For picking, obtaining, and refreshing the token, see [Token Relay Pattern](token-relay.md).

## Passthrough Mode (`token` unset)

```toml
[server.relay]
url = "https://senko-upstream.example.com"
# No token
```

- The relay **doesn't touch the Authorization header** — it passes through.
- Enable `[server.auth.oidc]` etc. on the upstream; it verifies the client's JWT / API key.

**Note**: the relay itself still doesn't authenticate. If a client sends no credential, the relay forwards an empty Authorization, and the upstream responds 401 (the relay is just passing that 401 back).

## Health Check

```
GET /api/v1/health
```

No auth required, doesn't hit the upstream, returns 200 immediately. Safe for load balancers.

## Operations Tips

- **The relay is stateless**: easy to scale out horizontally. Just note that hook-based aggregation happens per instance.
- **Don't skip TLS certificate verification on the upstream connection.**
- **Verify the network boundary**: use firewall / network namespace / compose network to limit reachability. Validate the design can't accidentally be exposed before starting.
- **Keep relay hooks audit-only** (heavy work belongs on the upstream or an external system).
- **Periodically rotate short-lived upstream tokens** (e.g. IdP-issued M2M JWTs) — see [Token Relay Pattern](token-relay.md).

## Troubleshooting

| Symptom | What to do |
|---|---|
| 502 Bad Gateway | Upstream down / network broken / DNS failure relay → upstream |
| 401 returned (substitution mode) | The `[server.relay] token` isn't accepted by the upstream — expired JWT / under-privileged API key / audience mismatch |
| 401 returned (passthrough mode) | Client sent no credential, or upstream rejected a weak credential |
| Hooks don't fire | `[server.relay.*]` hooks accidentally placed under `[server.remote.*]` or `[cli.*]`? Check the runtime warning |
| Unauthenticated clients reach the relay | By design. **Run on a closed network.** To expose, put a reverse proxy / API Gateway in front with authorization |

## Next Steps

- Picking and rotating tokens → [Token Relay Pattern](token-relay.md)
- Hook examples → [`[server.relay.*]` Hook Examples](hooks.md)
- AI sandbox end-to-end → [CLI → Relay → Remote → PostgreSQL](../../getting-started/cli-relay-remote-postgres.md)
