# Token Relay Pattern

A deep dive into running the relay with `[server.relay] token` set so it rewrites the client Authorization (substitution mode). For relay behavior in general, see [Deploying `senko serve` in Relay Mode](deploy.md).

> **Prerequisite**: the relay does no inbound authentication (`auth_mode: None` is hard-coded), so it can only run on a closed network. This page focuses on the **relay → upstream** auth path.

## Why Token Substitution?

Typical scenario: the agent inside an AI sandbox must not hold a strong production credential.

- You don't want client credentials inside the sandbox (containing the blast radius within the relay boundary on leak).
- The upstream uses ordinary OIDC / API-key authentication.
- → The relay holds the credential and substitutes it on outbound requests.

The client reaches the relay **without any credential**; the only authenticated path is relay → upstream.

## Substitution vs. Passthrough

| Aspect | Substitution (`token` set) | Passthrough (`token` unset) |
|---|---|---|
| Client's Authorization | **Dropped** | Passed through to the upstream |
| Identity arriving upstream | One relay identity (shared across requests) | The client's own credential identity |
| Credential concealment in sandbox | Possible | Not possible (client must hold a credential) |
| Upstream log actor | Fixed to the relay | Per client |
| Auditing | **Must live at the relay** | Upstream auditing suffices |

AI sandboxes suit **substitution**. For a simple relay that just shares an upstream as public SaaS, passthrough is handier.

## Preparing an Upstream Credential for Substitution

The issuance steps and operational cost vary by upstream auth mode.

### When the upstream is OIDC (recommended for production)

Two options.

#### Option 1: Reuse a human user's session API key (simplest)

Run the relay **on behalf of a specific person (e.g. alice)**. One relay = one person's stand-in.

```bash
# On your PC
senko auth login --device-name "alice-relay-sandbox"
senko auth token
# => sk_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

Put that value in `SENKO_SERVER_RELAY_TOKEN` and start the relay.

- TTL follows the upstream's `[server.auth.oidc.session] ttl` (e.g. 30d). No relay restart while the TTL holds.
- On expiry: `senko auth login` again → `senko auth token` for a new value → restart the relay.
- Per-session revocation via `senko auth revoke <session_id>`.
- **Upstream logs record every action as alice** — every AI action is on alice's ledger.

Detailed walkthrough: [CLI → Relay → Remote → PostgreSQL](../../getting-started/cli-relay-remote-postgres.md).

#### Option 2: Register an M2M client on the IdP (separate bot identity)

Use this when you want the relay to run as a **service account separate from any human**. Register an OAuth Client Credentials client on the IdP; the relay fetches JWTs directly.

- Grant: `client_credentials`
- Audience: the upstream URL
- Store `client_id` / `client_secret` in the relay's secret store.

The relay's entrypoint hits the IdP for a JWT, stores it as `SENKO_SERVER_RELAY_TOKEN`, and runs `senko serve`. Because IdP `access_token_lifetime` is typically ~1 hour, you **need to restart the relay on a timer** to refresh the token.

When the relay's first request reaches the upstream, the upstream **JIT-registers** the user (`username` = JWT `sub` = client_id). After registration, the project owner adds it as a member:

```bash
# On the upstream
senko project members add --user-id <relay-bot-id> --role member
```

**Comparing Options 1 and 2**:

| Aspect | Option 1 (session API key) | Option 2 (M2M JWT) |
|---|---|---|
| Operational cost | Low (TTL of days to weeks; manual renewal) | Medium (TTL ~1h; needs periodic restart / refresh plumbing) |
| Upstream log actor | A real human (alice) | A bot identity (e.g. `senko-relay-sandbox`) |
| Accountability | alice personally | Separable per service account |
| IdP-side extra config | None (reuses a normal user login) | Must register an M2M client |
| Relay-side refresh logic | Not needed | Required (entrypoint re-hits the IdP + cron/timer for restart) |
| Leak impact | Scope of alice's membership | Scope of the service account's membership |

For small/medium ops or personal sandbox use, **Option 1** is dramatically easier. If you need strict service-account separation across multiple relays, or compliance requirements (e.g. SOC2) mandate a bot identity, pick **Option 2**.

### When the upstream is trusted_headers

The API Gateway is terminating auth, so the relay needs **a JWT (access_token) accepted by the Gateway**. senko session API keys don't apply (senko isn't issuing any).

In practice this looks like Option 2 (M2M): fetch a JWT via `client_credentials` → set `SENKO_SERVER_RELAY_TOKEN` → rotate frequently because it's short-lived. An Option-1-like human JWT is possible, but it'll still expire per the IdP's `access_token_lifetime`, so operationally it's the same short-lived token story.

### When the upstream is API key mode (evaluation only)

When connecting to an API-key-mode upstream (i.e. an evaluation setup), issue a normal API key using `master_key` and use that as `SENKO_SERVER_RELAY_TOKEN`:

```bash
# On the upstream (running in API-key mode)
curl -s -X POST https://senko-upstream.example.com/api/v1/users \
  -H "Authorization: Bearer $UPSTREAM_MASTER_KEY" \
  -d '{"username":"relay-bot"}'
curl -s -X POST https://senko-upstream.example.com/api/v1/projects/1/members \
  -H "Authorization: Bearer $UPSTREAM_MASTER_KEY" \
  -d '{"user_id":7,"role":"member"}'
curl -s -X POST https://senko-upstream.example.com/api/v1/users/7/api-keys \
  -H "Authorization: Bearer $UPSTREAM_MASTER_KEY" \
  -d '{"name":"relay-bot"}'
# => put the returned key into SENKO_SERVER_RELAY_TOKEN
```

API keys are long-lived, so no periodic relay restart is required. But an API-key-mode upstream is itself evaluation-grade — pick OIDC upstream + Option 1/2 for production.

## How to Bring Auditing Back

In substitution mode, the upstream logs can't identify the real client. **Emit an audit log from relay hooks** — the standard pattern:

```toml
[server.relay.task_add.hooks.audit]
command = '''
jq -c "{
  ts: .event.timestamp,
  runtime: .runtime,
  actor: .user.name,
  project: .project.name,
  action: \"task_add\",
  task: .event.task.id
}" >> /var/log/senko-relay/audit.jsonl
'''
mode = "async"
```

But note: since the relay does no inbound auth, **the envelope's `.user` / `.project` reflect the relay container's own `[user] name` / `[project] name`** (set at startup by env / config). You cannot distinguish per-client.

To distinguish clients through a single relay isn't really possible. The practical solution is **one relay instance per sandbox** (giving each its own `[user]` env).

## Header Rewriting (Dynamic Identity Mapping) Is Not Supported

Inspecting a client's JWT claims and dynamically choosing a different service token per request **isn't supported by the relay alone**. If you need it:

- Put a reverse proxy / API Gateway / Lambda **in front of** the relay to rewrite dynamically.
- Or enable `trusted_headers` on the upstream and have the fronting component inject `x-senko-user-sub` etc.

## Common Mistakes

- **Conflating `[cli.remote]` URL with `[server.relay]`** — the former is how a CLI (human / agent) connects to the relay; the latter is the relay's own upstream setting.
- **Thinking the relay authenticates and setting `[server.auth.api_key]`** — these are not read in relay mode. Authorization comes from the network boundary, period.
- **Expecting substitution + passthrough simultaneously** — if `token` is set, substitution applies uniformly. The client's Authorization is dropped.

## Next Steps

- Overall relay operations → [Deploying `senko serve` in Relay Mode](deploy.md)
- Hook examples → [`[server.relay.*]` Hook Examples](hooks.md)
- AI sandbox end-to-end → [CLI → Relay → Remote → PostgreSQL](../../getting-started/cli-relay-remote-postgres.md)
