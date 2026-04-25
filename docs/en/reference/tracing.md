# Tracing Reference

The senko Remote and Relay emit observability data on two parallel channels:

1. **W3C Trace Context + Baggage**: the CLI propagates arbitrary attributes to the Remote on every request, and the Remote promotes them to `baggage.<key>` span attributes.
2. **Business event `LogRecord`s** (each carrying an `event.name` attribute): the application layer of the Remote and Relay emits one OTel `LogRecord` per domain state transition. Caller-supplied baggage from external systems (e.g. `--attr aviary.session.id=…`) rides on these records as common attributes, **without any prefix rewrite**.

Both channels are produced through the standard OTel SDK, so the same environment variables that drive other OTel-aware tools (e.g. Claude Code) drive senko too. For day-to-day operation see the [OTel Tracing Operations Guide](../guides/tracing.md).

> **The CLI in local mode (sqlite / postgres backends) does not initialize the OTel SDK.** Business and cross-cutting events fire only on the Remote (`senko serve`) and the Relay (`senko serve --proxy`).

## Business Events at a Glance

Business events are emitted via `tracing::event!` with a fixed `target: "senko_business"`. The `opentelemetry-appender-tracing` `OpenTelemetryTracingBridge` layer reads `Metadata::name()` and forwards it as the OTel `LogRecord::set_event_name`.

| Field | Value |
|---|---|
| `target` | `senko_business` (constant) |
| `Level` | `INFO` (most events) / `WARN` (`senko.hook.failed`) / `ERROR` (`senko.api.error`) |
| Sinks | The same record flows to **both** the fmt layer (stdout JSON) and the OTel Logs exporter |
| Common attributes | Resource / actor / target / `senko.operation.id` are auto-attached by the SDK and the `BusinessAttributesProcessor` |
| Emit layer | Application layer (`LocalXxxOperations` / `RemoteXxxOperations` / `XxxService`) plus a few middlewares (`presentation/api/telemetry.rs`, `infra/hook/mod.rs`) |
| `RUST_LOG` | `RUST_LOG=senko_business=info` filters business events independently of infra tracing |

The `BusinessAttributesProcessor` is registered as an OTel `LogProcessor` and only enriches records whose `target == "senko_business"`. It pulls two tokio task-locals on each emit:

- `RESOLVED_USER` (set by the auth middleware): supplies `enduser.id` and `enduser.name`.
- `INBOUND_BAGGAGE` (set by `propagate_trace_context`): supplies `senko.operation.id` plus any caller-supplied attributes (e.g. `aviary.session.id`). **Keys keep their original names — there is no `baggage.` prefix on the LogRecord side.**

Infra-level records (`info!("Listening on …")`, etc.) keep their default module-path `target`, so the processor leaves them alone — they only carry Resource attributes.

## `event.name` Catalog (33 events total)

Twenty-nine business events plus four cross-cutting events. Caller-supplied baggage like `aviary.*=…` rides on **every** one of them as common attributes.

### Task (11)

| `event.name` | When | Required attributes (in addition to the common ones) |
|---|---|---|
| `senko.task.created` | `task add` succeeds | `senko.task.id`, `senko.project.id` |
| `senko.task.updated` | `task edit` succeeds (title / description / priority / plan / tags / metadata / …) | `senko.task.id`, `senko.project.id`, `changed_fields` (JSON array) |
| `senko.task.published` | `task publish` succeeds | `senko.task.id`, `senko.project.id`, `from_status`, `to_status` |
| `senko.task.started` | `task start` succeeds | `senko.task.id`, `senko.project.id`, `from_status`, `to_status` |
| `senko.task.completed` | `task complete` succeeds | `senko.task.id`, `senko.project.id`, `from_status`, `to_status` |
| `senko.task.canceled` | `task cancel` succeeds | `senko.task.id`, `senko.project.id`, `from_status`, `to_status`, `cancel_reason` |
| `senko.task.dependency_added` | `deps add` succeeds | `senko.task.id`, `senko.project.id`, `dep_id` |
| `senko.task.dependency_removed` | `deps remove` succeeds | `senko.task.id`, `senko.project.id`, `dep_id` |
| `senko.task.dependencies_set` | `deps set` succeeds | `senko.task.id`, `senko.project.id`, `deps` (JSON array) |
| `senko.task.dod_checked` | `dod check` succeeds | `senko.task.id`, `senko.project.id`, `dod_index` |
| `senko.task.dod_unchecked` | `dod uncheck` succeeds | `senko.task.id`, `senko.project.id`, `dod_index` |

### Contract (6)

| `event.name` | When | Required attributes |
|---|---|---|
| `senko.contract.created` | `contract create` succeeds | `senko.contract.id`, `senko.project.id` |
| `senko.contract.updated` | `contract edit` succeeds | `senko.contract.id`, `senko.project.id`, `changed_fields` |
| `senko.contract.deleted` | `contract delete` succeeds | `senko.contract.id`, `senko.project.id` |
| `senko.contract.dod_checked` | `contract dod check` succeeds | `senko.contract.id`, `senko.project.id`, `dod_index` |
| `senko.contract.dod_unchecked` | `contract dod uncheck` succeeds | `senko.contract.id`, `senko.project.id`, `dod_index` |
| `senko.contract.note_added` | `contract note add` succeeds | `senko.contract.id`, `senko.project.id` |

### Project (5)

| `event.name` | When | Required attributes |
|---|---|---|
| `senko.project.created` | `project create` succeeds | `senko.project.id` |
| `senko.project.updated` | `project edit` succeeds | `senko.project.id`, `changed_fields` |
| `senko.project.member_added` | member added | `senko.project.id`, `senko.user.id` (target), `role` |
| `senko.project.member_removed` | member removed | `senko.project.id`, `senko.user.id` (target) |
| `senko.project.member_role_changed` | role changed | `senko.project.id`, `senko.user.id` (target), `from_role`, `to_role` |

### User (5)

| `event.name` | When | Required attributes |
|---|---|---|
| `senko.user.created` | `user create` or auto-provisioning succeeds | `senko.user.id` (target), `source` (`manual` / `oidc_provisioning` / `trusted_headers_provisioning`) |
| `senko.user.updated` | `user edit` succeeds | `senko.user.id` (target), `changed_fields` |
| `senko.user.api_key_issued` | API key issued | `senko.user.id` (target) |
| `senko.user.api_key_revoked` | API key revoked | `senko.user.id` (target) |
| `senko.user.session_revoked` | session revoke succeeds (single / all) | `senko.user.id` (target), `session.id`, `scope` (`Single` / `All`) |

When `scope=All`, **one LogRecord per affected session is emitted** — not a single rolled-up record.

### MetadataField (2)

| `event.name` | When | Required attributes |
|---|---|---|
| `senko.metadata_field.defined` | `metadata-field define` succeeds | `senko.project.id`, `senko.metadata_field.name`, `senko.metadata_field.type` |
| `senko.metadata_field.removed` | `metadata-field remove` succeeds | `senko.project.id`, `senko.metadata_field.name`, `senko.metadata_field.type` (the value before removal) |

### Cross-cutting (4)

Emitted by middleware / cross-cutting layers, not tied to a domain aggregate.

| `event.name` | When | Emit site | Required attributes |
|---|---|---|---|
| `senko.api.call` | Request finishes (one record per request, both 2xx and 5xx) | `propagate_trace_context` middleware | `http.method`, `http.route`, `http.status_code`, `latency_ms`, [`senko.project.id`] |
| `senko.api.error` | `ApiError` response | `IntoResponse for ApiError` | `http.status_code`, `error.type`, `error.message` |
| `senko.hook.fired` | Hook completes successfully (exit 0) | `ShellHookExecutor` | `hook.name`, `hook.trigger`, `exit_status=0`, `duration_ms` |
| `senko.hook.failed` | Hook fails (`timeout` / `spawn_error` / `non_zero_exit` / `stdin_error` / `wait_error`) | `ShellHookExecutor` | `hook.name`, `hook.trigger`, `failure.reason`, `duration_ms`, [`exit_status`], [`stderr_excerpt`], [`error.message`] |

`http.route` is the axum **`MatchedPath` template** — query strings are not included (e.g. `/api/projects/{project_id}/tasks`). When no route matches (404), it falls back to `uri.path()` (the raw path, still without the query).

`error.type` is one of `not_found` / `bad_request` / `unauthorized` / `forbidden` / `conflict` / `internal` / `not_implemented`. For `ApiError::Internal`, `error.message` carries an internal `log_message` (the Display-formatted anyhow chain) while the response body always returns the static string `"internal server error"` — so internal details (file paths, connection strings, stack traces) never reach the client.

`hook.trigger` is the hook config key (`task_complete`, `task_update`, `project_member_added`, `user_api_key_issued`, …). `stderr_excerpt` is at most 1024 bytes (UTF-8 lossy).

> **Known gap (revisit in Phase E1 / V1)**: hooks running in `mode = async` execute on a `std::thread::spawn` worker, so the tokio task-locals (`RESOLVED_USER`, `INBOUND_BAGGAGE`) do not propagate. Async-mode `senko.hook.fired` / `senko.hook.failed` therefore lack `enduser.*` and `senko.operation.id`. Sync mode is unaffected.

## Common Attribute Schema

Every `target=senko_business` record receives the attributes below automatically. The cross-cutting events sit on top of the same machinery.

### Resource attributes (fixed at startup, attached by the SDK)

| Attribute | Default | Env override |
|---|---|---|
| `service.name` | `senko-server` (Remote) / `senko-relay` (Relay) | Override via `OTEL_SERVICE_NAME` or `OTEL_RESOURCE_ATTRIBUTES=service.name=…` |
| `service.version` | Baked-in `CARGO_PKG_VERSION` at build time | Override via `OTEL_RESOURCE_ATTRIBUTES=service.version=…` |
| `senko.version` | Baked-in `CARGO_PKG_VERSION` at build time | **Not overridable.** The senko binary always self-reports this so telemetry provenance stays honest |

`senko.version` is required by the Aviary integration contract — operators should always see the actual version that emitted the data, so no env override is provided.

### actor — who performed the action (OTel semantic conventions)

| Attribute | Value |
|---|---|
| `enduser.id` | The authenticated user's `username` (attached by the Remote's auth middleware via the `RESOLVED_USER` task-local) |
| `enduser.name` | The user's display name (`display_name` if set, otherwise `username`) |

Unauthenticated requests (e.g. `/healthz` and other public endpoints) carry no `enduser.*`.

### target — what the action is operating on (senko-specific)

A LogRecord may carry both an actor and a target when the operator acts on someone else (e.g. adding a different user as a project member, revoking another user's session).

| Attribute | Carried by |
|---|---|
| `senko.task.id` | All `senko.task.*` |
| `senko.contract.id` | All `senko.contract.*` |
| `senko.project.id` | All `senko.{task,contract,project,metadata_field}.*`, plus `senko.api.call` when the route contains `{project_id}` |
| `senko.user.id` | All `senko.user.*` (= the target user's id), and `senko.project.member_*` (= the user being added / removed / promoted) |
| `senko.metadata_field.name` / `senko.metadata_field.type` | All `senko.metadata_field.*` |

### Common (caller-supplied baggage, attached by `BusinessAttributesProcessor`)

| Attribute | Value |
|---|---|
| `senko.operation.id` | UUIDv4 minted by the CLI process (correlates every span / LogRecord across one user-level operation) |
| Arbitrary caller-supplied attributes | Whatever the CLI passed via `--attr foo=bar` (e.g. `aviary.session.id`, `aviary.nest.id`, `aviary.task.id`). **Keys keep their original names — no `baggage.` prefix on LogRecords** |

Caller-supplied attributes flow through the reserved-namespace filter first; only the post-filter form lands on the record (see [Reserved Namespaces](#reserved-namespaces)).

### HTTP attributes (only on `senko.api.call` / `senko.api.error`)

| Attribute | Value |
|---|---|
| `http.method` | The request method (`GET`, `POST`, …) |
| `http.route` | The axum `MatchedPath` template, e.g. `/api/projects/{project_id}/tasks/{id}` — no query string |
| `http.status_code` | Integer response status code |
| `latency_ms` | Request handling time in integer milliseconds |

> **Important**: the legacy `http.target` attribute (the raw URL = path + query) has been **removed** in favor of `http.route` (the template). This keeps query strings and sensitive path segments (`/api/users/{user_id}/api-keys/{key_id}` and similar) out of long-lived telemetry.

## Existing Tracing → New Event Mapping

The bare tracing calls below have been **replaced** by the new business events; no double output is produced.

| Legacy output | New event | Notes |
|---|---|---|
| `tracing::warn!("api_error", …)` (`presentation/api/mod.rs`) | `senko.api.error` | `error.type` / `error.message` are now structured |
| `tracing::error!("unclassified internal error", ?e)` | `senko.api.error` (`error.type=internal`) | `?e` (Debug) → `%e` (Display); the anyhow chain is flattened with `format!("{e:#}")` |
| `tracing::info!("auto-provisioning user from OIDC claims")` (`infra/auth.rs`) | `senko.user.created` (`source=oidc_provisioning`) | — |
| `tracing::info!("auto-provisioning user from trusted headers")` | `senko.user.created` (`source=trusted_headers_provisioning`) | — |
| `tracing::info!("response", …)` / `error!("request failed", …)` (`presentation/api/telemetry.rs`) | `senko.api.call` (one per request, 2xx or 5xx) / `senko.api.error` | — |
| Hook `tracing::warn!`s (`infra/hook/mod.rs`) | `senko.hook.failed` | `failure.reason` / `stderr_excerpt` are structured |

## Bare Tracing That Stays (not replaced)

Operations-observability output that does not describe a domain transition is left as plain `tracing::info!` / `warn!`:

- `info!("Listening on {addr}")` (startup)
- `info!("OTel telemetry initialized")` / `info!("OTel telemetry disabled (OTEL_SDK_DISABLED=true)")` (bootstrap)
- `info!("shutdown signal received")` (graceful shutdown)
- `warn!("baggage value truncated", …)` / `warn!("baggage drops excess key", …)` / `warn!("baggage total size exceeded", …)` (incoming sanitization)
- `warn!("OIDC discovery failed")` / `warn!("JWKS fetch failed")` (transient auth bootstrap failures)
- `validate_hook_def` and `warn_about_mismatched_runtime_sections` startup warnings (config validation)
- Various `tracing::debug!` calls (e.g. auth claim mismatch)

These belong to operations observability, not business observability.

## HTTP Headers the CLI Sends

Every CLI → Remote request carries:

| Header | When added | Contents |
|---|---|---|
| `traceparent` | **Always** | `version-trace_id-parent_id-flags` (W3C Trace Context). A fresh 128-bit `trace_id` + 64-bit `span_id` is generated per request. |
| `baggage` | **Only when the merged attribute map is non-empty** | `key1=value1,key2=value2` (W3C Baggage). Keys and values are both percent-encoded with the `NON_ALPHANUMERIC` class, so `.` `=` `,` and spaces all become `%..` escapes. |

If no attributes are configured, `baggage` is omitted and only `traceparent` is sent.

## The Four Attribute Sources and Precedence

The CLI merges four sources to decide what ends up in `baggage`:

| Source | Format | Malformed entry | Reserved-namespace filter |
|---|---|---|---|
| `--attr KEY=VALUE` (global CLI flag, repeatable) | One pair per use | **Hard error** (`invalid --attr …`) | Not applied |
| `SENKO_TRACE_ATTRIBUTES` (env var) | `K=V,K=V,…` | Silently skipped (per OTel spec) | Not applied |
| `OTEL_RESOURCE_ATTRIBUTES` (env var) | `K=V,K=V,…` | Silently skipped | **Applied** |
| Auto-populated (`senko.operation.id`) | UUIDv4 generated once per CLI process | — | Not applied (internal value) |

### Precedence

When the same key appears in multiple sources, the higher-priority one wins: **`--attr` > `SENKO_TRACE_ATTRIBUTES` > `OTEL_RESOURCE_ATTRIBUTES` > auto**. Any of the upper three sources can override an auto-populated value.

### Auto-Populated Attributes

| Key | Value | When it's generated |
|---|---|---|
| `senko.operation.id` | UUIDv4 string | Minted once per CLI process on the first trace-attribute resolution, then reused for the rest of the invocation |

`senko.operation.id` is the correlation ID that ties together every HTTP request, hook invocation, and status change within a single `senko …` invocation. Because the same value rides in every baggage header the CLI sends during that process, filtering on `senko.operation.id` on the Remote gives you all spans and logs for one user-level operation. To force a specific ID (testing, replay, cross-CLI correlation), pass `--attr senko.operation.id=<own-id>` or set `SENKO_TRACE_ATTRIBUTES=senko.operation.id=<own-id>` — the explicit value wins.

### Using `--attr`

`--attr` is a **global flag** — place it before the subcommand.

```bash
senko --attr run.id=abc123 --attr session.id=xyz task complete 42
```

Malformed values fail loudly instead of being dropped (the one place senko deviates from OTel env-var semantics):

| Input | Error |
|---|---|
| `--attr foo` (no `=`) | `invalid --attr 'foo': expected KEY=VALUE` |
| `--attr =bar` (empty key) | `invalid --attr '=bar': key must not be empty` |
| `--attr foo=` (empty value) | `invalid --attr 'foo=': value must not be empty` |

#### Caller-supplied attributes (Aviary and friends)

For external-system integrations, pass system-specific correlation IDs as multiple `--attr` flags. They are **attached verbatim to the business event LogRecords** (no `baggage.` prefix on the LogRecord side):

```bash
senko \
  --attr aviary.session.id=sess-abc \
  --attr aviary.nest.id=nest-42 \
  --attr aviary.task.id=at-99 \
  task complete 42
```

The Remote's `senko.task.completed` LogRecord then carries:

- Domain: `senko.task.id=42`, `senko.project.id=…`, `from_status=in_progress`, `to_status=completed`
- Actor: `enduser.id=…`, `enduser.name=…`
- Common: `senko.operation.id=<UUID>`, `aviary.session.id=sess-abc`, `aviary.nest.id=nest-42`, `aviary.task.id=at-99`
- Resource: `service.name=senko-server`, `service.version=…`, `senko.version=…`

Aviary can then group every senko operation in one session by filtering on `aviary.session.id` in Jaeger / Tempo / its logging backend.

### How `SENKO_TRACE_ATTRIBUTES` / `OTEL_RESOURCE_ATTRIBUTES` Are Parsed

- `K=V` pairs separated by `,` (matches the OTel Resource Attributes spec).
- Whitespace around keys is trimmed.
- **Whitespace around values is preserved** (`foo= bar` → value is ` bar`).
- Malformed entries (no `=`, empty key, empty value) are **silently skipped** — no log is emitted.
- An empty string is treated as zero attributes.

## Reserved Namespaces

To avoid clobbering OTel-defined Resource attributes, keys with any of the following prefixes are **automatically excluded from `OTEL_RESOURCE_ATTRIBUTES`**. **The check is case-insensitive** — `SERVICE.NAME`, `Service.Name`, and `service.name` are all filtered (Contract #8 / Phase F2 closed the mixed-case bypass):

```
service.  host.  os.  process.  telemetry.  deployment.  cloud.  k8s.  container.
```

The filter applies to **`OTEL_RESOURCE_ATTRIBUTES` only**. Anything you set explicitly via `--attr` or `SENKO_TRACE_ATTRIBUTES` passes through untouched (explicit user intent wins). The Remote (`propagate_trace_context`) re-applies the same filter defensively on the receiving side.

## Baggage Limits (Receiver Side)

When the Remote (or Relay) receives baggage, it normalizes the map exactly once via `apply_baggage_limits` before promoting it to span attributes or forwarding it. In Relay mode, the same normalized map is forwarded upstream — preventing the Relay from acting as a DoS amplifier.

| Limit | Value | Behavior on overflow |
|---|---|---|
| Per-value length | **256 bytes** (truncated on a UTF-8 boundary) | `tracing::warn!("baggage value truncated", …)` is emitted; the truncated value is kept |
| Number of keys | **32 keys** | `tracing::warn!("baggage drops excess key", …)`; **the 33rd-onward keys are dropped in alphabetical order** (head retain) |
| Total size | **8 KB (8 × 1024 bytes)** | `tracing::warn!("baggage total size exceeded", …)`; **keys are dropped from the tail (reverse alphabetical)** until the total is ≤ 8 KB |

The fixed normalization order is (1) cap key count to 32, (2) truncate each value to 256 B, (3) drop trailing keys until total ≤ 8 KB. The CLI itself does not truncate — the receiver owns the limit (uniform server-side enforcement).

## OTel Environment Variables Honored by `senko serve`

The Remote reads the standard OTel environment variables at startup to initialize the SDK:

| Variable | Values | Default | Behavior |
|---|---|---|---|
| `OTEL_SDK_DISABLED` | `true` / `false` | `false` | `true` disables every OTel layer (only the fmt log layer and the W3C propagator remain). |
| `OTEL_TRACES_EXPORTER` | `otlp` / `console` / `none` | `none` (see note) | Traces destination. |
| `OTEL_LOGS_EXPORTER` | `otlp` / `console` / `none` | `none` (see note) | Logs destination — business event LogRecords flow here. |
| `OTEL_SERVICE_NAME` | string | `senko-server` (Remote) / `senko-relay` (Relay) | Value of the `service.name` Resource attribute. |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | URL | — | OTLP collector endpoint. When set, the default exporter is promoted to `otlp`. |
| `OTEL_RESOURCE_ATTRIBUTES` | `K=V,K=V,…` | — | Resource attributes (read directly by the SDK). Including `service.version=…` here overrides the Remote's default (baked-in `CARGO_PKG_VERSION`). |

> **Note on defaults**: the OTel spec says the default exporter is `otlp`, but senko Remote defaults to **`none` when no OTel env is set** — this keeps local development quiet and avoids unintended OTLP connections. Setting `OTEL_EXPORTER_OTLP_ENDPOINT` promotes the default to `otlp`. Unknown exporter names log a warning and fall back to `none`.

## Baggage → Span Attribute Promotion

The Remote middleware (`propagate_trace_context` in `presentation/api/telemetry.rs`) takes each entry from the incoming `baggage` header and attaches it to the request span as **`baggage.<key>`**.

- This is a separate channel from business event LogRecords (`target=senko_business`). Spans flow on the OTLP traces side; business events flow on the OTLP logs side. Both carry the same `trace_id`, so they can be joined in any backend.
- Reserved-namespace keys are filtered defensively (see [Reserved Namespaces](#reserved-namespaces)).
- Values are truncated at **256 bytes** on a UTF-8 boundary.
- A `tracing::warn!` is emitted when truncation happens.

A baggage entry `run.id=xyz` becomes `baggage.run.id = "xyz"` in Jaeger / Tempo / any OTel backend, searchable like any other span attribute. On the **business event LogRecord** side the same value lands as `run.id=xyz` (no prefix), so log filters use the original key.

## Proxy Mode

`senko serve --proxy` issues a **fresh `traceparent`** when forwarding to the upstream Remote (re-emitted, not passed through). The inbound `baggage` is extracted and **re-emitted** on the forwarded request, so `baggage.<key>` entries set by the originating CLI appear on the upstream Remote's spans as-is.

- The Relay does **not re-filter reserved namespaces** — the CLI already filters, and a second filter here would silently drop keys the user opted into via `--attr` or `SENKO_TRACE_ATTRIBUTES`.
- The upstream Remote's `propagate_trace_context` still applies defensive reserved-namespace filtering when promoting received baggage into `baggage.<key>` span attributes (unchanged).

### Enduser Resolution for the Relay's Own Telemetry

The Relay's own LogRecords (`senko.api.call`, `senko.api.error`, `senko.hook.*`, etc. emitted on the Relay itself) also need `enduser.*` populated. To do that, the Relay calls the upstream Remote's `/auth/me` and caches the result in an **LRU cache with a 5-minute TTL**, then injects it into the `RESOLVED_USER` task-local for the request's duration.

The cache key is computed in three steps (first match wins):

1. Bearer JWT → extract the `sub` claim and use `jwt:<sub>` (signature is **not** verified — the `sub` is decoded with a manual base64 split).
2. Opaque (non-JWT, or JWT without `sub`) token → `tok:<sha256_hex_of_token>`.
3. Trusted-headers mode → use the `subject_header` value as `thv:<value>`.

On failure (network error, non-2xx upstream, parse failure, missing required fields) the Relay does **not** populate the cache and continues without `enduser.*` (graceful degrade). The fetch timeout is 5 seconds.

## Graceful Shutdown

On `SIGINT` (Ctrl-C) or `SIGTERM`, the Remote / Relay drains in-flight axum requests, then flushes the OTel tracer and logger providers before exiting. Because telemetry is dropped **after** an explicit flush, even short-lived processes manage to ship their final spans and business event LogRecords.

## See Also

- [`--attr` global flag](cli.md#global-options)
- [OTel Tracing Operations Guide](../guides/tracing.md) — Aviary integration, `event.name` queries, audit filters, Jaeger / Tempo / console-exporter verification, security considerations.
