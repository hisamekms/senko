# CLI → Relay → Remote → PostgreSQL (AI Sandbox)

Use senko from inside an AI-agent sandbox while keeping the upstream senko server's credentials **entirely out of the sandbox**.

→ How the three pillars play out in this setup: [Core Concept](../explanation/core-concept.md).

```
  Sandbox-only network                                  Outbound
┌──────────────────────────────────────┐      ┌──────────────────────────┐
│  AI sandbox container                │      │  Relay container         │
│  (no upstream secret)                │      │  (holds alice's session  │
│                                      │      │   API key; acts as alice)│
│  senko CLI                           │──┐   │                          │
│   SENKO_CLI_REMOTE_URL=http://relay  │  │   │  senko serve (relay mode)│
│                                      │  └──►│                          │──┐
│  (sandbox can only egress to relay)  │      │  SENKO_SERVER_RELAY_URL  │  │
│                                      │      │  SENKO_SERVER_RELAY_TOKEN│  │
└──────────────────────────────────────┘      │   = alice's session key  │  │
                                              └──────────────────────────┘  │
                                                                            │
                                                                            ▼
                                              ┌────────────────────┐
                                              │  senko serve       │
                                              │  (OIDC direct)     │
                                              │  → logged as alice │
                                              └────────┬───────────┘
                                                       │
                                                       ▼
                                              ┌────────────────────┐
                                              │  PostgreSQL        │
                                              └────────────────────┘
```

## When to Choose This

- The AI agent runs **outside your trust boundary** (assume prompt injection)
- You still want to let the agent perform some senko operations
- You **cannot** put the upstream senko credential inside the sandbox
- You want "who did what" auditing at the relay layer

Conversely, if the CLI runs only in a **trusted developer's own hands**, there's no reason to add a relay — [CLI → Remote → PostgreSQL](cli-remote-postgres.md) is simpler.

## What "secretless" Means Here

**Inside the CLI (sandbox container)**, all you hold is:
- The relay URL (reachable only within the sandbox network)
- Non-sensitive settings like the project name

**Deliberately kept out of the CLI side**:
- Credentials for the upstream (OIDC JWT, M2M client_secret, API key, DB credentials…)
- Connection info for the IdP (token endpoint, client_id, client_secret)
- The upstream URL itself (the relay knows it; the sandbox doesn't need to)

Even if the AI leaks everything inside the sandbox, **there's no credential that escapes the sandbox**. The security boundary is two-layered:

1. **Network isolation**: the sandbox container can only egress to the relay (compose network isolation / iptables / outbound deny).
2. **Relay one-way flow**: the relay holds the M2M JWT in `[server.relay] token` and **owns the authentication to the upstream**. The sandbox's identity never reaches the upstream.

> **Relay does no inbound authentication.** When `[server.relay] url` is set, `senko serve` starts with `auth_mode: None` and forwards incoming requests to the upstream without checking credentials (the relay's own `[server.auth.*]` is ignored in relay mode). That means anything that can reach the relay can call the upstream through it, so **network isolation is your only defense**.

## What the Relay Holds (the "secret-full" side)

The real credential lives in the relay (and **never in the sandbox**):
- A **Bearer token** accepted by the upstream senko — the current recommendation is **a human user's session API key obtained via `senko auth login`** (detailed below).
- (Depending on deployment) Access to Secrets Manager / podman secret / `.env`.

Pass that Bearer token to the relay as `SENKO_SERVER_RELAY_TOKEN`, and the relay rewrites the `Authorization` header of incoming requests with it before forwarding to the upstream (substitution mode).

## A Current Limitation: 1 Relay = 1 User

senko's relay **has no way to forward the caller's identity (from the sandbox's CLI) to the upstream**. There's no `on-behalf-of` header; the relay sends exactly one `Authorization`. The relay therefore **stands in for a single senko user**.

Operational implications:

- **Deploy the relay for one specific person (e.g. alice)** — embed alice's session API key in the relay.
- Upstream logs/audit will all attribute actions to alice.
- Anything the sandboxed agent does is recorded as "alice acted via the relay." A natural fit for individual usage where you want one person's agent activity aggregated under their own log.
- Sharing one relay across a team mixes everyone's actions under a single name, so **for team use you run one sandbox (= one relay) per person**.

Future work could lift this limit — caller identity forwarding, OAuth Token Exchange (RFC 8693), per-sandbox bots — but none of that is implemented today.

## Components

| Layer | Role | Where it runs | Secrets |
|---|---|---|---|
| CLI | Client the AI agent calls | Inside the sandbox (a podman compose container) | None (reaches the relay with no auth) |
| Relay | Auth substitution + auditing from sandbox → upstream | Outside the trust boundary (another container in the same compose, or a separate host) | OIDC M2M client_secret (entrypoint exchanges it for a JWT) |
| Remote | The actual `senko serve` that holds the data | Separate host (or same VPC) | PostgreSQL credentials (+ `master_group` / OIDC IdP integration) |
| PostgreSQL | Persistence layer | RDS / Aurora / self-hosted | DB connection info |

> For a local minimum, the easiest pattern is **one podman compose with `sandbox` and `relay` side by side** — separate containers, separate env / secret scopes (see the Step 2 example).

## Setup

### Prerequisites

You must already have [CLI → Remote → PostgreSQL](cli-remote-postgres.md) set up (PostgreSQL + OIDC-authenticated `senko serve` + a project created).

> **This guide assumes the upstream runs in OIDC mode.** If the upstream runs in `trusted_headers` mode (behind an API Gateway, etc.), the token to put in the relay and its TTL behavior are different — see [The `trusted_headers` Upstream Case](#the-trusted_headers-upstream-case) below.

### Step 1: Get a session API key from the upstream

1. **Set a long session TTL on the upstream** (`[server.auth.oidc.session] ttl = "30d"`, for example) so the token embedded in the relay doesn't expire often.
2. **Log in via PKCE from your own PC (outside the sandbox)**:

   ```bash
   senko auth login --device-name "relay-for-sandbox"
   ```

   The session API key lands in your OS keychain.
3. **Extract the session API key**:

   ```bash
   senko auth token
   # => sk_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
   ```

   Use that value as the relay's `SENKO_SERVER_RELAY_TOKEN`.

> What `senko auth token` returns is **a session API key (`sk_xxx`) issued by the senko server after OIDC authentication**. The CLI sends the IdP JWT (obtained via PKCE) exactly once to the upstream's `POST /auth/token`; the upstream verifies it, creates a new API key internally (stored in `api_keys`), and returns it to the keychain. It is **not** the IdP JWT itself. TTL is governed by `[server.auth.oidc.session]` and individual sessions can be revoked with `senko auth revoke`. While the TTL holds, the relay needs no refresh (no restart either).
>
> When the TTL expires, do this manually: re-run `senko auth login` → `senko auth token` → update the relay env → restart the relay. Pick a long TTL (e.g. 30d) to keep this rare.

### Step 1.5 (Optional): Separate relay tokens per person

If each person runs their own relay, use a distinct `--device-name` so you can later list and revoke them individually with `senko auth sessions`:

```bash
# For human use (if it already exists, skip)
senko auth login --device-name "alice-laptop"

# For the relay (a separate session)
senko auth login --device-name "alice-relay-sandbox"
senko auth token > /tmp/relay-token.txt   # → paste into the relay's .env
```

When the sandbox is decommissioned, `senko auth revoke <id>` cancels just the relay session.

### Step 2: Deploy the relay with podman compose

`.env` (never baked into the sandbox image; add to `.gitignore`):

```
SENKO_RELAY_TOKEN=sk_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx  # the session API key from Step 1
```

`compose.yaml`:

```yaml
services:
  relay:
    image: senko:latest
    command: ["serve", "--host", "0.0.0.0", "--port", "3142"]
    volumes:
      - ./relay-config.toml:/etc/senko/config.toml:ro
      - ./audit:/var/log/senko-relay
    environment:
      SENKO_CONFIG:              "/etc/senko/config.toml"
      SENKO_SERVER_RELAY_URL:    "https://senko-upstream.example.com"
      SENKO_SERVER_RELAY_TOKEN:  "${SENKO_RELAY_TOKEN}"  # injected via .env
      SENKO_USER:                "alice"                 # name recorded in the audit envelope
    networks: [sandbox-net]
    restart: unless-stopped

  sandbox:
    image: my-ai-sandbox:latest
    depends_on: [relay]
    environment:
      SENKO_CLI_REMOTE_URL: "http://relay:3142"
      SENKO_PROJECT:        "backend-team"
    networks: [sandbox-net]

networks:
  sandbox-net: {}          # isolated from the default bridge; apply egress restrictions separately
```

> Notice `SENKO_CLI_REMOTE_TOKEN` is absent from the sandbox side. The relay doesn't check inbound auth, so a token there would be meaningless — and "don't put credentials in the sandbox" is the simpler rule.

Run it:

```bash
podman compose up -d
```

The relay and the sandbox come up. As long as the upstream's `[server.auth.oidc.session] ttl` holds, the relay doesn't need to be restarted.

#### `relay-config.toml`

```toml
[server]
host = "0.0.0.0"       # reachable from the compose network
port = 3142

# Upstream senko server; token is injected via SENKO_SERVER_RELAY_TOKEN
[server.relay]
url = "https://senko-upstream.example.com"

# Audit who passed through
# In relay mode there's no auth layer; envelope.user comes from the relay's [user] / SENKO_USER.
[server.relay.task_add.hooks.audit]
command = '''
jq -c "{
  ts: .event.timestamp,
  via: .user.name,
  action: \"task_add\",
  task: .event.task.id,
  title: .event.task.title
}" >> /var/log/senko-relay/audit.jsonl
'''
mode = "async"

[server.relay.task_complete.hooks.audit]
command = 'jq -c ". | {ts: .event.timestamp, via: .user.name, task: .event.task.id}" >> /var/log/senko-relay/audit.jsonl'
mode = "async"

[server.relay.task_cancel.hooks.audit]
command = 'jq -c ". | {ts: .event.timestamp, via: .user.name, task: .event.task.id, reason: .event.task.cancel_reason}" >> /var/log/senko-relay/audit.jsonl'
mode = "async"

[log]
format = "json"
level  = "info"
```

> **In relay mode `[server.auth.*]` and `[backend.*]` are ignored.** Writing them is not an error, but it's confusing — keep the relay config to the bare minimum shown above.

#### Rotating the session token

When the upstream's `[server.auth.oidc.session] ttl` passes, the relay's `SENKO_SERVER_RELAY_TOKEN` becomes invalid and the upstream starts returning 401. To recover:

```bash
# On your PC
senko auth login --device-name "alice-relay-sandbox"
senko auth token                             # prints the new session API key

# Update .env and restart the relay
vim .env                                      # replace SENKO_RELAY_TOKEN
podman compose up -d --force-recreate relay   # re-read env and bring relay back up
```

With a long TTL (e.g. 30 days), this is a monthly operation at most.

### Step 3: Verify the sandbox-side CLI

This was configured via the `sandbox` service in the Step 2 compose.yaml — just confirm:

```yaml
environment:
  SENKO_CLI_REMOTE_URL: "http://relay:3142"   # reach the relay container across the compose network
  SENKO_PROJECT:        "backend-team"
```

When the agent (or developer) inside the sandbox runs `senko`:

```bash
senko task list                            # fetched from the upstream via the relay
senko task next                            # state transition relay → upstream
senko task complete 42                     # same
```

- The sandbox reaches the relay **without any credential** (relay ignores inbound auth).
- The relay passes the request through but **rewrites the Authorization header** with `SENKO_SERVER_RELAY_TOKEN` (= alice's session API key) before sending to the upstream.
- The upstream validates that session API key and records the action **as alice**.

### Step 4: Tracking who actually ran the action

Upstream logs only see the relay's standing identity, so **the relay-side audit log is the source of truth**.

Ship the relay's `/var/log/senko-relay-audit.jsonl` straight to CloudWatch Logs / Loki / etc.:

```bash
# Fluent Bit example
[INPUT]
    Name tail
    Path /var/log/senko-relay/audit.jsonl
    Parser json

[OUTPUT]
    Name cloudwatch_logs
    Match *
    region ap-northeast-1
    log_group_name /senko/relay-audit
    log_stream_name relay-$(hostname)
    auto_create_group On
```

- Always map the sandbox ID or agent name into `user.name` or `project.name`.
- Ship logs **outside the sandbox** so they can't be deleted from within it.

### Step 6: Identity separation for multiple sandboxes

Since relay mode does no inbound authentication, **the relay instance's `[user] name` / `[project] name` determines the envelope actor**. A single sandbox + single relay is fine with Step 2 as-is.

To run multiple sandboxes simultaneously or share this pattern across multiple people, **split the relays by person** — each relay carries that person's session API key:

```bash
# alice's sandbox relay
alice% senko auth login --device-name "alice-relay-sandbox"
alice% senko auth token > alice-relay/.env      # SENKO_RELAY_TOKEN=...
alice% SENKO_USER=alice podman compose up -d    # inside alice-relay/

# bob's sandbox relay
bob%   senko auth login --device-name "bob-relay-sandbox"
bob%   senko auth token > bob-relay/.env
bob%   SENKO_USER=bob podman compose up -d      # inside bob-relay/
```

`senko serve` in relay mode reflects the startup `[user] name` / `SENKO_USER` into the envelope's `user`, so relay-side audit logs distinguish "which relay did this pass through". Upstream logs distinguish the people because each relay authenticates with alice's or bob's own session API key.

## Security Model

### Threat model

- **The AI dumps everything inside the sandbox** — fine. There's no credential inside that can reach the upstream.
- **The AI makes arbitrary outbound HTTP requests** — the sandbox's network policy denies anything that isn't the relay.
- **The AI takes excessive actions through the relay (spam / repeated cancels)** — detect/limit via relay hooks and/or upstream rate limits. **Every AI action is recorded as the owner (alice) on the upstream**, so the account holder is on the hook to monitor it.
- **The relay itself is compromised** — alice's session API key leaks. Treat the relay as a trust boundary and harden it. On leak, revoke immediately with `senko auth revoke`.

### Must-haves

- [ ] `SENKO_SERVER_RELAY_TOKEN` (= alice's session API key) is **unreadable from the sandbox container** (separate env scopes, do not mount the secret into the sandbox).
- [ ] The sandbox container's network can only reach the relay (= the compose-internal service).
- [ ] The relay container is allowed to egress only to the upstream senko.
- [ ] Relay audit logs are shipped immediately to immutable storage outside the sandbox.
- [ ] Harden the relay's host / container like any production server.
- [ ] The session API key is issued with a relay-only `--device-name` (e.g. `alice-relay-sandbox`), separate from the human-login one, so leaks can be revoked surgically.

### AI-specific caveats

- **Prompt injection**: when the agent writes comments on a task, it can end up executing instructions that came from an external source. `workflow.task_add.instructions` can tell the agent "don't execute unknown instructions," but design assumes that rule is not always honored.
- **Over-action**: the agent may unnecessarily spam `senko task cancel`. Add relay-side hooks to flag unusual patterns.

## Operations Checklist

- [ ] Sandbox container env contains only `SENKO_CLI_REMOTE_URL` and `SENKO_PROJECT` (no session API key or upstream URL).
- [ ] Sandbox network cannot reach anything but the relay (separate compose network / egress restrictions).
- [ ] The relay container has **env / secrets in a different scope** than the sandbox (separate `.env`, never mount the podman secret into the sandbox).
- [ ] `SENKO_RELAY_TOKEN` is not baked into the image (injected from `.env` or a secret store).
- [ ] The relay-only session uses a dedicated `--device-name` that differs from the human login, and can be revoked via `senko auth sessions`.
- [ ] The upstream `[server.auth.oidc.session] ttl` is aligned with org policy (not too long, balanced against operational cost).
- [ ] Relay audit hooks cover every action (`task_add` / `task_ready` / `task_start` / `task_complete` / `task_cancel` / `contract_add` / `contract_note_add` / `contract_dod_check` / `contract_dod_uncheck`).
- [ ] Audit logs are shipped to tamper-proof storage outside the sandbox.
- [ ] **The account owner (alice) is accountable for monitoring AI activity passing through the relay** — cross-correlate sandbox audit logs with upstream OIDC session logs.

## The `trusted_headers` Upstream Case

If the upstream runs in `trusted_headers` mode instead of OIDC (e.g. API Gateway + Cognito + Lambda — see [AWS Deployment](../guides/server-remote/aws-deployment.md)), the nature of the token in the relay changes:

| Aspect | OIDC mode | trusted_headers mode |
|---|---|---|
| What `senko auth token` returns | Session API key issued by senko (`sk_xxx`) | The IdP's **raw JWT (access_token)** |
| Expiry management | `api_keys` table + `[server.auth.oidc.session] ttl` | senko is not involved. Follows the IdP's `access_token_lifetime` |
| Typical TTL | Configurable (e.g. 30 days) | IdP default (usually ~1 hour; up to 24 hours on Cognito) |
| `senko auth revoke` | Per-session revocation | Not available (no session in senko's DB) |
| Refresh | Not needed while the TTL holds; re-run `senko auth login` on expiry | Short-lived; frequent refresh required |

### Operational impact

- **The relay's `SENKO_SERVER_RELAY_TOKEN` has to be rotated frequently**. A sandbox running across the JWT's expiry window will start getting 401s.
- Automation options: if the IdP issues a **refresh_token**, wire up a relay-side startup script that refreshes the token on a timer. The current senko CLI doesn't handle refresh_tokens, so this is custom work.
- Alternatively, register an **OAuth Client Credentials (M2M) client** on the IdP and have the relay's entrypoint fetch a JWT with `client_credentials` (the old relay pattern). The upstream records the relay as an M2M service account rather than as a human session.
- **The M2M account must exist on the upstream**: after JIT registration, have the owner add it as a member.

Bottom line: a `trusted_headers` upstream + relay means you hand session management off to the IdP / your own scripts, so ops costs are higher than with OIDC direct mode. When possible, either switch the upstream to OIDC mode or — if the sandbox doesn't strictly need to be isolated from the IdP — avoid this pattern.

## Variants

### Variant A: One relay per sandbox session

Step 2 sized for one sandbox + one relay. To run several sandboxes at once, **duplicate the sandbox+relay pair**:

- Generate `sandbox-N` + `relay-N` (separate network / separate project) from the compose template.
- Seed each relay with a **session API key issued under its own `--device-name`** → you can separate sandboxes using the upstream's `api_keys.device_name`.
- Tear down both when the session ends (compose ephemeral).

On Kubernetes, putting `sandbox` and `relay` in the same Pod as sidecars and binding both to the Pod lifecycle is the natural shape.

### Variant B: Routing other clients (PR bots / CI) through the relay

Since the relay has no inbound auth, naive exposure would let anything reach the upstream through it. If non-sandbox clients also need to go through the relay, either **add authorization in front of the relay** (mTLS / IP allowlist at nginx / a separate API Gateway), or — more simply — have those clients connect directly to the upstream.

## Troubleshooting

| Symptom | What to try |
|---|---|
| 502 from the sandbox | Relay → upstream connectivity broken / upstream down |
| 401 after a while | Session API key TTL expired. Re-run `senko auth login` → update `.env` → `podman compose up -d --force-recreate relay` |
| Actions recorded under the wrong user upstream | `SENKO_SERVER_RELAY_TOKEN` is a different person's session key. Cross-check `senko auth sessions` against the relay env |
| Upstream logs show the action but audit log doesn't | Relay hook may be `sync` and failing. `senko hooks log -f` to investigate |
| The sandbox knows the upstream URL directly | Sandbox env accidentally contains the upstream URL. Make sure `SENKO_CLI_REMOTE_URL` points at the relay |

## See Also

- Relay overall → [Deploy relay `senko serve`](../guides/server-relay/deploy.md)
- Token relay patterns → [Token Relay Pattern](../guides/server-relay/token-relay.md)
- Relay hook examples → [`[server.relay.*]` Hook Examples](../guides/server-relay/hooks.md)
- Choosing a runtime → [Choosing a Runtime](../explanation/runtimes.md)
