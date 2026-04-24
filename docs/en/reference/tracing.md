# Tracing Reference

The senko CLI propagates arbitrary attributes to the Remote via **W3C Trace Context + Baggage**, and the Remote emits traces and logs through the **OpenTelemetry SDK**. Because everything is spec-based, the same environment variables used by other OTel-aware tools (e.g. Claude Code) drive senko too.

For day-to-day operation, see the [OTel Tracing Operations Guide](../guides/tracing.md).

## HTTP Headers the CLI Sends

Every CLI → Remote request carries:

| Header | When added | Contents |
|---|---|---|
| `traceparent` | **Always** | `version-trace_id-parent_id-flags` (W3C Trace Context). A fresh 128-bit `trace_id` + 64-bit `span_id` is generated per request. |
| `baggage` | **Only when the merged attribute map is non-empty** | `key1=value1,key2=value2` (W3C Baggage). Keys and values are both percent-encoded with the `NON_ALPHANUMERIC` class, so `.` `=` `,` and spaces all become `%..` escapes. |

If no attributes are configured, `baggage` is omitted and only `traceparent` is sent.

## The Three Attribute Sources and Precedence

The CLI merges three sources to decide what ends up in `baggage`:

| Source | Format | Malformed entry | Reserved-namespace filter |
|---|---|---|---|
| `--attr KEY=VALUE` (global CLI flag, repeatable) | One pair per use | **Hard error** (`invalid --attr …`) | Not applied |
| `SENKO_TRACE_ATTRIBUTES` (env var) | `K=V,K=V,…` | Silently skipped (per OTel spec) | Not applied |
| `OTEL_RESOURCE_ATTRIBUTES` (env var) | `K=V,K=V,…` | Silently skipped | **Applied** |

### Precedence

When the same key appears in multiple sources, the higher-priority one wins: **`--attr` > `SENKO_TRACE_ATTRIBUTES` > `OTEL_RESOURCE_ATTRIBUTES`**.

### Using `--attr`

`--attr` is a **global flag** — place it before the subcommand.

```bash
senko --attr run.id=abc123 --attr session.id=xyz task complete 42
```

Malformed values fail loudly instead of being dropped (this is the one place senko deviates from OTel env-var semantics):

| Input | Error |
|---|---|
| `--attr foo` (no `=`) | `invalid --attr 'foo': expected KEY=VALUE` |
| `--attr =bar` (empty key) | `invalid --attr '=bar': key must not be empty` |
| `--attr foo=` (empty value) | `invalid --attr 'foo=': value must not be empty` |

### How `SENKO_TRACE_ATTRIBUTES` / `OTEL_RESOURCE_ATTRIBUTES` Are Parsed

- `K=V` pairs separated by `,` (matches the OTel Resource Attributes spec).
- Whitespace around keys is trimmed.
- **Whitespace around values is preserved** (`foo= bar` → value is ` bar`).
- Malformed entries (no `=`, empty key, empty value) are **silently skipped** — no log is emitted.
- An empty string is treated as zero attributes.

## Reserved Namespaces

To avoid clobbering OTel-defined Resource attributes, keys with any of the following prefixes are **automatically excluded from `OTEL_RESOURCE_ATTRIBUTES`**:

```
service.  host.  os.  process.  telemetry.  deployment.  cloud.  k8s.  container.
```

The filter applies to **`OTEL_RESOURCE_ATTRIBUTES` only**. Anything you set explicitly via `--attr` or `SENKO_TRACE_ATTRIBUTES` passes through untouched (explicit user intent wins).

## Value Size Limit

When the Remote promotes a baggage value to a span attribute, values **longer than 256 bytes are truncated at a UTF-8 boundary**, and a `tracing::warn!` is emitted. The CLI itself does not truncate.

## OTel Environment Variables Honored by `senko serve`

The Remote reads the standard OTel environment variables at startup to initialize the SDK:

| Variable | Values | Default | Behavior |
|---|---|---|---|
| `OTEL_SDK_DISABLED` | `true` / `false` | `false` | `true` disables every OTel layer (only the fmt log layer and the W3C propagator remain). |
| `OTEL_TRACES_EXPORTER` | `otlp` / `console` / `none` | `none` (see note) | Traces destination. |
| `OTEL_LOGS_EXPORTER` | `otlp` / `console` / `none` | `none` (see note) | Logs destination. |
| `OTEL_SERVICE_NAME` | string | `senko-server` | Value of the `service.name` Resource attribute. |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | URL | — | OTLP collector endpoint. When set, the default exporter is promoted to `otlp`. |
| `OTEL_RESOURCE_ATTRIBUTES` | `K=V,K=V,…` | — | Resource attributes (read directly by the SDK). |

> **Note on defaults**: the OTel spec says the default exporter is `otlp`, but senko Remote defaults to **`none` when no OTel env is set** — this keeps local development quiet and avoids unintended OTLP connections. Setting `OTEL_EXPORTER_OTLP_ENDPOINT` promotes the default to `otlp`. Unknown exporter names log a warning and fall back to `none`.

## Baggage → Span Attribute Promotion

The Remote middleware (`propagate_trace_context` in `presentation/api/telemetry.rs`) takes each entry from the incoming `baggage` header and attaches it to the request span as **`baggage.<key>`**.

- Keys in a **reserved namespace** (`service.*` etc., see the list above) are **defensively filtered out** (the CLI already filters them, but the server filters again).
- Values are truncated at **256 bytes** on a UTF-8 boundary.
- A `tracing::warn!` is emitted when truncation happens.

As a result, a baggage entry `run.id=xyz` becomes `baggage.run.id = "xyz"` in Jaeger / Tempo / any OTel backend, searchable like any other span attribute.

## Proxy Mode

`senko serve --proxy` issues a **fresh `traceparent`** when forwarding to the upstream Remote (re-emitted, not passed through). Forwarding the inbound baggage as outbound attributes is **not yet implemented**.

## Graceful Shutdown

On `SIGINT` (Ctrl-C) or `SIGTERM`, the Remote drains in-flight axum requests, then flushes the OTel tracer and logger providers before exiting. Because telemetry is dropped **after** an explicit flush, even short-lived processes manage to ship their final spans.

## See Also

- [`--attr` global flag](cli.md#global-options)
- [OTel Tracing Operations Guide](../guides/tracing.md) — Claude Code integration, Jaeger / Tempo / console-exporter verification, security considerations
