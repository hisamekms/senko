//! HTTP middleware that receives W3C `traceparent` + `baggage` headers from
//! the senko CLI, starts a server-side span under the extracted parent
//! context, and promotes client-supplied baggage values to span attributes so
//! they also ride along on every OTel log record emitted while the request is
//! being handled.
//!
//! In relay mode (`senko serve --proxy`), the extracted baggage is also
//! stashed into the `INBOUND_BAGGAGE` task-local for the duration of the
//! request so the outbound `HttpClient` can re-emit it on the forwarded
//! request to the upstream Remote.
//!
//! The corresponding CLI-side emitter is `src/infra/http/trace_propagation.rs`
//! (sibling task 339); this file is the receiving half.

use std::collections::BTreeMap;
use std::time::Instant;

use axum::extract::{MatchedPath, Request};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;
use opentelemetry::baggage::{Baggage, BaggageExt};
use opentelemetry::propagation::Extractor;
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::infra::http::INBOUND_BAGGAGE;
use crate::infra::http::trace_propagation::is_reserved_namespace;

/// Maximum length (in bytes) of a single Baggage value before it is truncated
/// and a warning is emitted. Matches the Contract #7 spec: "1 値 256 char
/// 超過時に切り詰め". Values are ASCII after percent-decoding in the common
/// case, so bytes ≈ chars; truncation snaps to a UTF-8 boundary regardless.
pub const BAGGAGE_VALUE_MAX_LEN: usize = 256;

/// Maximum number of baggage keys retained per inbound request. Excess keys
/// are dropped in alphabetical order — the head 32 are kept. Defends against
/// a malicious CLI spamming arbitrary keys to inflate per-span attribute
/// count and downstream collector / APM cost.
pub const BAGGAGE_MAX_KEYS: usize = 32;

/// Maximum total UTF-8 byte budget across all retained entries
/// (`Σ key.len() + value.len()`). Once exceeded after per-value truncation,
/// keys are dropped from the tail (reverse-alphabetical) until the running
/// total fits.
pub const BAGGAGE_MAX_TOTAL_BYTES: usize = 8 * 1024;

/// Adapter so `TextMapPropagator::extract` can read from axum's `HeaderMap`.
struct HeaderMapExtractor<'a>(&'a HeaderMap);

impl<'a> Extractor for HeaderMapExtractor<'a> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|k| k.as_str()).collect()
    }
}

/// Axum middleware — extract inbound trace context, open a server span,
/// promote baggage to span attributes, and emit one Contract #8
/// `senko.api.call` LogRecord per request (200 and 5xx alike). Replaces the
/// older `tower_http::trace::TraceLayer`: status + latency are recorded on
/// the span here, and the response/request-failed `tracing` lines have been
/// folded into the single business event.
///
/// `http.route` is the matched axum template (e.g.
/// `/api/v1/projects/{project_id}/tasks/{id}`), not the raw URI — query
/// strings are intentionally never recorded.
pub async fn propagate_trace_context(req: Request, next: Next) -> Response {
    let parent_cx = opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.extract(&HeaderMapExtractor(req.headers()))
    });

    let method = req.method().clone();
    let uri = req.uri().clone();
    let route: String = req
        .extensions()
        .get::<MatchedPath>()
        .map(|mp| mp.as_str().to_string())
        .unwrap_or_else(|| uri.path().to_string());
    let project_id = extract_path_param_i64(&route, uri.path(), "project_id");

    let span = tracing::info_span!(
        "http_request",
        otel.name = %format!("{} {}", method, route),
        otel.kind = "server",
        http.method = %method,
        http.route = %route,
        http.status_code = tracing::field::Empty,
        latency_ms = tracing::field::Empty,
    );
    let _ = span.set_parent(parent_cx.clone());

    let inbound_baggage = apply_baggage_limits(parent_cx.baggage());
    promote_baggage_to_span(&span, &inbound_baggage);

    let start = Instant::now();
    let response = INBOUND_BAGGAGE
        .scope(inbound_baggage, next.run(req).instrument(span.clone()))
        .await;
    let latency = start.elapsed();

    let status = response.status();
    let status_u16 = status.as_u16();
    let latency_ms = latency.as_millis() as u64;
    span.record("http.status_code", status_u16);
    span.record("latency_ms", latency_ms);

    // emit_business_event! expands to `tracing::event!(...);` with a trailing
    // semicolon, so each match arm must be a block expression (otherwise the
    // semicolon lands in expression position and triggers `future_incompatible`
    // / `semicolon_in_expressions_from_macros`).
    let _enter = span.enter();
    match project_id {
        Some(pid) => {
            crate::emit_business_event!(
                "senko.api.call",
                http.method = %method,
                http.route = %route,
                http.status_code = status_u16,
                latency_ms = latency_ms,
                senko.project.id = pid,
            );
        }
        None => {
            crate::emit_business_event!(
                "senko.api.call",
                http.method = %method,
                http.route = %route,
                http.status_code = status_u16,
                latency_ms = latency_ms,
            );
        }
    }
    drop(_enter);

    response
}

/// Extract a `{name}` path-parameter value from `path` using `template` as
/// the matching shape. Returns `None` when:
/// - `template` does not contain a `{name}` segment;
/// - `template` and `path` segment counts disagree (defensive — should not
///   happen when both come from the same matched route);
/// - the captured value does not parse as `i64`.
///
/// Used by `propagate_trace_context` to attach `senko.project.id` to
/// `senko.api.call` records when the route shape includes `{project_id}`.
fn extract_path_param_i64(template: &str, path: &str, name: &str) -> Option<i64> {
    let placeholder = format!("{{{name}}}");
    let template_segs = template.split('/');
    let path_segs = path.split('/');
    template_segs
        .zip(path_segs)
        .find_map(|(t, p)| (t == placeholder).then_some(p))
        .and_then(|v| v.parse::<i64>().ok())
}

/// Normalize inbound baggage with three sequential limits, returning a flat
/// `BTreeMap` suitable for both span-attribute promotion and relay-side
/// re-emission. No reserved-namespace filtering here — the CLI already
/// filters outbound, and relay is a passthrough (re-filtering would drop
/// entries the CLI explicitly permitted via `--attr` / `SENKO_TRACE_ATTRIBUTES`).
///
/// Order matters:
///   1. Drop excess keys past `BAGGAGE_MAX_KEYS` in alphabetical order
///      (head 32 retained).
///   2. Truncate each value to `BAGGAGE_VALUE_MAX_LEN` at a UTF-8 boundary.
///   3. Drop trailing keys (reverse-alphabetical) until total UTF-8 bytes
///      ≤ `BAGGAGE_MAX_TOTAL_BYTES`.
///
/// Each overflow emits a single `tracing::warn!` so DoS / cost-inflation
/// patterns are visible without log spam.
fn apply_baggage_limits(baggage: &Baggage) -> BTreeMap<String, String> {
    let mut map: BTreeMap<String, String> = baggage
        .iter()
        .map(|(key, (value, _metadata))| (key.as_str().to_string(), value.as_str().to_string()))
        .collect();

    if map.len() > BAGGAGE_MAX_KEYS {
        let original_count = map.len();
        let to_drop: Vec<String> = map.keys().skip(BAGGAGE_MAX_KEYS).cloned().collect();
        for k in to_drop {
            map.remove(&k);
        }
        tracing::warn!(
            count = original_count,
            max = BAGGAGE_MAX_KEYS,
            "baggage key count exceeded",
        );
    }

    for (key, value) in map.iter_mut() {
        let original_len = value.len();
        let (truncated, was_truncated) = truncate_baggage_value(value, BAGGAGE_VALUE_MAX_LEN);
        if was_truncated {
            tracing::warn!(
                key = key.as_str(),
                original_len,
                max = BAGGAGE_VALUE_MAX_LEN,
                "baggage value truncated",
            );
            *value = truncated;
        }
    }

    let mut total: usize = map.iter().map(|(k, v)| k.len() + v.len()).sum();
    if total > BAGGAGE_MAX_TOTAL_BYTES {
        let original_total = total;
        let keys_desc: Vec<String> = map.keys().rev().cloned().collect();
        for k in keys_desc {
            if total <= BAGGAGE_MAX_TOTAL_BYTES {
                break;
            }
            if let Some(v) = map.remove(&k) {
                total -= k.len() + v.len();
            }
        }
        tracing::warn!(
            total_bytes = original_total,
            max = BAGGAGE_MAX_TOTAL_BYTES,
            "baggage total length exceeded",
        );
    }

    map
}

/// Attach each non-reserved baggage entry as a `baggage.<key>` span attribute.
/// Reserved keys (`service.*`, `host.*`, etc. — see `trace_propagation`) are
/// dropped defensively: the CLI already filters them, but a mis-configured
/// client or proxy could still send them, and we never want them to overwrite
/// server-side OTel Resource attributes of the same name.
fn promote_baggage_to_span(span: &tracing::Span, baggage: &BTreeMap<String, String>) {
    for (key, value) in baggage {
        if is_reserved_namespace(key) {
            continue;
        }
        span.set_attribute(format!("baggage.{key}"), value.clone());
    }
}

/// Truncate to ≤ `max_len` bytes at a valid UTF-8 boundary. Returns
/// `(truncated, true)` when truncation happened, `(original.to_string(), false)`
/// otherwise. Snapping to a char boundary avoids panics on non-ASCII values
/// that survive percent-decoding.
fn truncate_baggage_value(value: &str, max_len: usize) -> (String, bool) {
    if value.len() <= max_len {
        return (value.to_string(), false);
    }
    let mut end = max_len;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::middleware::from_fn;
    use axum::routing::get;
    use opentelemetry::KeyValue;
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_sdk::propagation::{BaggagePropagator, TraceContextPropagator};
    use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
    use tower::ServiceExt;

    // --- truncate_baggage_value --------------------------------------------

    #[test]
    fn truncate_short_value_unchanged() {
        let (out, truncated) = truncate_baggage_value("abc", 256);
        assert_eq!(out, "abc");
        assert!(!truncated);
    }

    #[test]
    fn truncate_exact_boundary_not_truncated() {
        let s = "a".repeat(256);
        let (out, truncated) = truncate_baggage_value(&s, 256);
        assert_eq!(out.len(), 256);
        assert!(!truncated);
    }

    #[test]
    fn truncate_long_ascii_value() {
        let s = "x".repeat(300);
        let (out, truncated) = truncate_baggage_value(&s, 256);
        assert!(truncated);
        assert_eq!(out.len(), 256);
        assert!(out.chars().all(|c| c == 'x'));
    }

    #[test]
    fn truncate_snaps_to_utf8_boundary() {
        // 254 ASCII bytes + "あ" (3 bytes in UTF-8) spans byte positions
        // 254..257; truncating at byte 256 would land mid-char. The helper
        // must back up to byte 254.
        let mut s = "x".repeat(254);
        s.push('あ');
        assert!(s.len() > 256);
        let (out, truncated) = truncate_baggage_value(&s, 256);
        assert!(truncated);
        assert!(out.len() <= 256);
        // Valid UTF-8 => `&str` slicing did not panic and result is usable.
        assert!(
            out.is_ascii(),
            "expected multibyte char to be dropped; got {out:?}"
        );
    }

    // --- promote_baggage_to_span via the middleware -------------------------

    /// Build a tracing subscriber wired to an in-memory OTel exporter and
    /// run `body` under it. Returns the recorded `SpanData`.
    fn with_test_subscriber<F, Fut>(body: F) -> Vec<opentelemetry_sdk::trace::SpanData>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        use tracing_subscriber::layer::SubscriberExt;

        // Install a composite propagator so `get_text_map_propagator` works
        // inside the middleware. Idempotent across test runs because global
        // overwrite is a no-op for equal propagators in practice.
        opentelemetry::global::set_text_map_propagator(
            opentelemetry::propagation::TextMapCompositePropagator::new(vec![
                Box::new(TraceContextPropagator::new()),
                Box::new(BaggagePropagator::new()),
            ]),
        );

        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let tracer = provider.tracer("senko-test");
        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
        let subscriber = tracing_subscriber::registry().with(otel_layer);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let _guard = tracing::subscriber::set_default(subscriber);
            body().await;
        });

        provider.force_flush().ok();
        exporter.get_finished_spans().unwrap()
    }

    fn router_under_test() -> Router {
        Router::new()
            .route("/t", get(|| async { StatusCode::OK }))
            .layer(from_fn(propagate_trace_context))
    }

    fn request_with_headers(traceparent: Option<&str>, baggage: Option<&str>) -> HttpRequest<Body> {
        let mut builder = HttpRequest::builder().uri("/t").method("GET");
        if let Some(tp) = traceparent {
            builder = builder.header("traceparent", tp);
        }
        if let Some(bg) = baggage {
            builder = builder.header("baggage", bg);
        }
        builder.body(Body::empty()).unwrap()
    }

    #[test]
    fn middleware_promotes_baggage_to_span_attribute() {
        let spans = with_test_subscriber(|| async {
            let app = router_under_test();
            let resp = app
                .oneshot(request_with_headers(
                    Some("00-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbbb-01"),
                    Some("run.id=xyz,session.id=abc"),
                ))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        });

        assert_eq!(spans.len(), 1, "expected exactly one finished span");
        let span = &spans[0];
        // Propagated trace ID carries through.
        assert_eq!(
            format!(
                "{:032x}",
                u128::from_be_bytes(span.span_context.trace_id().to_bytes())
            ),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        let has_run_id = span
            .attributes
            .iter()
            .any(|kv| kv.key.as_str() == "baggage.run.id" && kv.value.as_str() == "xyz");
        let has_session_id = span
            .attributes
            .iter()
            .any(|kv| kv.key.as_str() == "baggage.session.id" && kv.value.as_str() == "abc");
        assert!(
            has_run_id,
            "missing baggage.run.id in {:?}",
            span.attributes
        );
        assert!(
            has_session_id,
            "missing baggage.session.id in {:?}",
            span.attributes
        );
    }

    #[test]
    fn middleware_filters_reserved_namespace() {
        let spans = with_test_subscriber(|| async {
            let app = router_under_test();
            // Client sends a reserved-namespace key (they shouldn't, but we
            // must drop it defensively).
            let _ = app
                .oneshot(request_with_headers(
                    None,
                    Some("service.name=evil,run.id=good"),
                ))
                .await
                .unwrap();
        });
        let span = spans.into_iter().next().expect("one span");
        for kv in &span.attributes {
            assert_ne!(
                kv.key.as_str(),
                "baggage.service.name",
                "reserved key must not be promoted",
            );
        }
        assert!(
            span.attributes
                .iter()
                .any(|kv| kv.key.as_str() == "baggage.run.id"),
            "non-reserved key must still be promoted",
        );
    }

    #[test]
    fn middleware_truncates_oversized_baggage_value() {
        // 300-byte value percent-encoded as `a` repeated. Baggage parser
        // accepts unencoded alphanumerics, so we just pad raw `a`s.
        let long_value = "a".repeat(300);
        let header = format!("big={long_value}");

        let spans = with_test_subscriber(|| async {
            let app = router_under_test();
            let _ = app
                .oneshot(request_with_headers(None, Some(&header)))
                .await
                .unwrap();
        });
        let span = spans.into_iter().next().expect("one span");
        let attr = span
            .attributes
            .iter()
            .find(|kv| kv.key.as_str() == "baggage.big")
            .expect("baggage.big present");
        assert_eq!(
            attr.value.as_str().len(),
            BAGGAGE_VALUE_MAX_LEN,
            "value should be truncated to BAGGAGE_VALUE_MAX_LEN bytes",
        );
    }

    #[test]
    fn middleware_promotes_senko_operation_id_baggage() {
        use crate::bootstrap::resolve_trace_attributes;
        use crate::infra::http::trace_propagation::build_baggage_header;

        // SAFETY: telemetry tests in this file are not #[serial], but
        // resolve_trace_attributes reads two env vars. Clear them so ambient
        // CI env doesn't leak an override into the auto value.
        unsafe {
            std::env::remove_var("SENKO_TRACE_ATTRIBUTES");
            std::env::remove_var("OTEL_RESOURCE_ATTRIBUTES");
        }

        // Build the exact baggage header a real CLI invocation would send
        // when the user passes no --attr flags.
        let attrs = resolve_trace_attributes(&[]);
        let auto_id = attrs
            .get("senko.operation.id")
            .expect("auto senko.operation.id must be present")
            .clone();
        let header = build_baggage_header(&attrs).expect("baggage header must be non-empty");

        let spans = with_test_subscriber(|| async {
            let app = router_under_test();
            let resp = app
                .oneshot(request_with_headers(None, Some(&header)))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        });

        let span = spans.into_iter().next().expect("one span");
        let attr = span
            .attributes
            .iter()
            .find(|kv| kv.key.as_str() == "baggage.senko.operation.id")
            .expect("baggage.senko.operation.id must be promoted to span attribute");
        assert_eq!(attr.value.as_str(), auto_id);
    }

    // --- Baggage helper (direct unit test of promote_baggage_to_span) -------

    #[test]
    fn promote_baggage_directly_records_and_skips_reserved() {
        // Seed a tracing span into an in-memory OTel exporter, call
        // promote_baggage_to_span directly, assert attributes.
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let tracer = provider.tracer("senko-unit");

        use tracing_subscriber::layer::SubscriberExt;
        let subscriber =
            tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer));
        let _g = tracing::subscriber::set_default(subscriber);

        let map: BTreeMap<String, String> = [
            ("run.id".to_string(), "r1".to_string()),
            ("service.name".to_string(), "nope".to_string()),
        ]
        .into_iter()
        .collect();

        {
            let span = tracing::info_span!("manual");
            promote_baggage_to_span(&span, &map);
            drop(span);
        }

        provider.force_flush().ok();
        let spans = exporter.get_finished_spans().unwrap();
        let span = spans.into_iter().next().expect("one span");
        assert!(
            span.attributes
                .iter()
                .any(|kv| kv.key.as_str() == "baggage.run.id"),
        );
        assert!(
            !span
                .attributes
                .iter()
                .any(|kv| kv.key.as_str() == "baggage.service.name"),
        );
    }

    // --- apply_baggage_limits (DoS / cost-inflation guards) -----------------

    #[test]
    fn apply_baggage_limits_drops_excess_keys() {
        // 100 keys. Names are "k000".."k099" — alphabetical order matches
        // numeric order, so the head 32 retained should be "k000".."k031".
        let pairs: Vec<KeyValue> = (0..100)
            .map(|i| KeyValue::new(format!("k{i:03}"), format!("v{i}")))
            .collect();
        let baggage = Baggage::from_iter(pairs);

        let result = apply_baggage_limits(&baggage);

        assert_eq!(result.len(), BAGGAGE_MAX_KEYS, "must cap at 32");
        assert!(result.contains_key("k000"));
        assert!(result.contains_key("k031"));
        assert!(
            !result.contains_key("k032"),
            "k032 must be dropped (alphabetical tail)",
        );
        assert!(!result.contains_key("k099"));
    }

    #[test]
    fn apply_baggage_limits_truncates_oversized_value_within_total_budget() {
        // 1 key whose value exceeds BAGGAGE_VALUE_MAX_LEN. Per-value
        // truncation kicks in, total stays well under 8 KB so the key is
        // NOT dropped. (We can't construct a 10 KB value via Baggage::from_iter
        // because the OTel SDK enforces a per-pair byte cap; 1024 bytes is
        // well above BAGGAGE_VALUE_MAX_LEN=256 and still accepted.)
        let oversized = "a".repeat(1024);
        let baggage = Baggage::from_iter([KeyValue::new("big", oversized)]);

        let result = apply_baggage_limits(&baggage);

        assert_eq!(result.len(), 1, "single key must survive");
        let value = result.get("big").expect("'big' must remain");
        assert_eq!(
            value.len(),
            BAGGAGE_VALUE_MAX_LEN,
            "value must be truncated to BAGGAGE_VALUE_MAX_LEN",
        );
        let total: usize = result.iter().map(|(k, v)| k.len() + v.len()).sum();
        assert!(
            total <= BAGGAGE_MAX_TOTAL_BYTES,
            "total {total} must fit budget after truncation",
        );
    }

    #[test]
    fn apply_baggage_limits_drops_tail_when_total_bytes_exceeded() {
        // 20 keys × 500-byte values = ~10 KB, well over the 8 KB budget but
        // under the 32-key count limit. Must drop from the alphabetical
        // tail until total ≤ 8 KB while retaining head keys.
        let value_len = 500;
        let pairs: Vec<KeyValue> = (0..20)
            .map(|i| KeyValue::new(format!("k{i:02}"), "x".repeat(value_len)))
            .collect();
        let baggage = Baggage::from_iter(pairs);

        let result = apply_baggage_limits(&baggage);

        let total: usize = result.iter().map(|(k, v)| k.len() + v.len()).sum();
        assert!(
            total <= BAGGAGE_MAX_TOTAL_BYTES,
            "total {total} must fit BAGGAGE_MAX_TOTAL_BYTES",
        );
        // At least one key must have been dropped (we sent ~10 KB).
        assert!(
            result.len() < 20,
            "expected tail drops, got len = {}",
            result.len(),
        );
        // Head retention: alphabetical-smallest keys survive. "k00" must be
        // present; the last key ("k19") must be gone.
        assert!(result.contains_key("k00"), "head key must survive");
        assert!(
            !result.contains_key("k19"),
            "tail key must be dropped first",
        );
    }

    // --- INBOUND_BAGGAGE task-local propagation (relay re-emit) -------------

    /// Build a router where the handler captures the `INBOUND_BAGGAGE`
    /// task-local into a shared map. This lets us assert what the middleware
    /// handed down to the per-request scope.
    fn router_capturing_inbound_baggage(
        captured: std::sync::Arc<std::sync::Mutex<Option<BTreeMap<String, String>>>>,
    ) -> Router {
        Router::new()
            .route(
                "/t",
                get(move || {
                    let captured = captured.clone();
                    async move {
                        let map = INBOUND_BAGGAGE.try_with(|b| b.clone()).unwrap_or_default();
                        *captured.lock().unwrap() = Some(map);
                        StatusCode::OK
                    }
                }),
            )
            .layer(from_fn(propagate_trace_context))
    }

    #[test]
    fn middleware_scopes_inbound_baggage_for_downstream() {
        opentelemetry::global::set_text_map_propagator(
            opentelemetry::propagation::TextMapCompositePropagator::new(vec![
                Box::new(TraceContextPropagator::new()),
                Box::new(BaggagePropagator::new()),
            ]),
        );

        let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
        let app = router_capturing_inbound_baggage(captured.clone());

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let resp = app
                .oneshot(request_with_headers(
                    None,
                    Some("run.id=xyz,session.id=abc"),
                ))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        });

        let map = captured.lock().unwrap().clone().expect("handler ran");
        assert_eq!(map.get("run.id").map(String::as_str), Some("xyz"));
        assert_eq!(map.get("session.id").map(String::as_str), Some("abc"));
    }

    #[test]
    fn middleware_scopes_empty_map_when_no_baggage_header() {
        opentelemetry::global::set_text_map_propagator(
            opentelemetry::propagation::TextMapCompositePropagator::new(vec![
                Box::new(TraceContextPropagator::new()),
                Box::new(BaggagePropagator::new()),
            ]),
        );

        let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
        let app = router_capturing_inbound_baggage(captured.clone());

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let _ = app.oneshot(request_with_headers(None, None)).await.unwrap();
        });

        let map = captured.lock().unwrap().clone().expect("handler ran");
        assert!(map.is_empty(), "expected empty map, got {map:?}");
    }

    #[test]
    fn middleware_does_not_re_filter_reserved_namespace_for_inbound() {
        // The CLI filters reserved keys from outbound; relay must NOT re-filter
        // (DoD #3). If a reserved key somehow arrives at the relay it flows
        // through to the upstream as-is; the upstream's promote_baggage_to_span
        // is the line of defense for span attributes.
        opentelemetry::global::set_text_map_propagator(
            opentelemetry::propagation::TextMapCompositePropagator::new(vec![
                Box::new(TraceContextPropagator::new()),
                Box::new(BaggagePropagator::new()),
            ]),
        );

        let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
        let app = router_capturing_inbound_baggage(captured.clone());

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let _ = app
                .oneshot(request_with_headers(
                    None,
                    Some("service.name=evil,run.id=ok"),
                ))
                .await
                .unwrap();
        });

        let map = captured.lock().unwrap().clone().expect("handler ran");
        // Both keys present in the re-emission map — no relay-side filtering.
        assert_eq!(map.get("service.name").map(String::as_str), Some("evil"));
        assert_eq!(map.get("run.id").map(String::as_str), Some("ok"));
    }

    // --- Integration: full relay-style chain (middleware + HttpClient) -------

    /// End-to-end: inbound `baggage` header at the relay reaches a mock
    /// upstream as a forwarded `baggage` header via `HttpClient.propagate`.
    /// Exercises: middleware scope → task-local → HttpClient.merged_attributes
    /// → build_baggage_header → outbound reqwest.
    #[test]
    fn baggage_relayed_from_inbound_to_upstream() {
        use crate::infra::http::client::HttpClient;
        use std::sync::{Arc, Mutex};

        opentelemetry::global::set_text_map_propagator(
            opentelemetry::propagation::TextMapCompositePropagator::new(vec![
                Box::new(TraceContextPropagator::new()),
                Box::new(BaggagePropagator::new()),
            ]),
        );

        let received = Arc::new(Mutex::new(Option::<String>::None));
        let received_for_block = received.clone();

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async move {
            // 1. Mock upstream: captures the forwarded `baggage` header.
            let received_up = received_for_block.clone();
            let upstream = Router::new().route(
                "/upstream",
                get(move |headers: HeaderMap| {
                    let received_up = received_up.clone();
                    async move {
                        let bg = headers
                            .get("baggage")
                            .and_then(|v| v.to_str().ok())
                            .map(String::from);
                        *received_up.lock().unwrap() = bg;
                        StatusCode::OK
                    }
                }),
            );
            let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let upstream_addr = upstream_listener.local_addr().unwrap();
            tokio::spawn(async move {
                axum::serve(upstream_listener, upstream).await.unwrap();
            });
            let upstream_url = format!("http://{upstream_addr}");

            // 2. Relay: propagate_trace_context middleware + handler that
            //    forwards to upstream via HttpClient (static attrs empty, just
            //    like serve_proxy's proxy_attrs).
            let client = Arc::new(HttpClient::new(&upstream_url, None, BTreeMap::new()));
            let client_for_handler = client.clone();
            let upstream_url_for_handler = upstream_url.clone();
            let relay = Router::new()
                .route(
                    "/relay",
                    get(move || {
                        let client = client_for_handler.clone();
                        let url = upstream_url_for_handler.clone();
                        async move {
                            let resp = client
                                .propagate(client.reqwest().get(format!("{url}/upstream")))
                                .send()
                                .await
                                .unwrap();
                            assert!(resp.status().is_success());
                            StatusCode::OK
                        }
                    }),
                )
                .layer(from_fn(propagate_trace_context));
            let relay_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let relay_addr = relay_listener.local_addr().unwrap();
            tokio::spawn(async move {
                axum::serve(relay_listener, relay).await.unwrap();
            });
            let relay_url = format!("http://{relay_addr}");

            // 3. Send request to relay with a baggage header.
            let resp = reqwest::Client::new()
                .get(format!("{relay_url}/relay"))
                .header("baggage", "run.id=xyz,session.id=abc")
                .send()
                .await
                .unwrap();
            assert!(resp.status().is_success());
        });

        let forwarded = received
            .lock()
            .unwrap()
            .clone()
            .expect("upstream did not receive a request");
        // NON_ALPHANUMERIC encodes `.` as %2E; BTreeMap sorts keys.
        assert_eq!(forwarded, "run%2Eid=xyz,session%2Eid=abc");
    }

    // --- senko.api.call LogRecord emission (Contract #8 / C1) ---------------

    use opentelemetry::logs::AnyValue;
    use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
    use opentelemetry_sdk::logs::{InMemoryLogExporter, SdkLoggerProvider};

    /// Drive the middleware under a tracing subscriber that bridges
    /// `tracing::event!` calls (including `emit_business_event!`) into an
    /// in-memory OTel `LogRecord` exporter. Returns every emitted record.
    ///
    /// Intentionally does NOT install a global `TextMapPropagator` — these
    /// tests only assert middleware-emitted `senko.api.call` records and the
    /// middleware tolerates a noop propagator (empty `parent_cx` ⇒ empty
    /// inbound baggage). Avoiding the global side effect keeps the test from
    /// stepping on parallel tests in this file.
    fn with_log_subscriber<F, Fut>(body: F) -> Vec<opentelemetry_sdk::logs::SdkLogRecord>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        use tracing_subscriber::layer::SubscriberExt;

        let exporter = InMemoryLogExporter::default();
        let provider = SdkLoggerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let subscriber =
            tracing_subscriber::registry().with(OpenTelemetryTracingBridge::new(&provider));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let _g = tracing::subscriber::set_default(subscriber);
            body().await;
        });

        provider.force_flush().ok();
        exporter
            .get_emitted_logs()
            .unwrap()
            .into_iter()
            .map(|d| d.record)
            .collect()
    }

    fn lookup_log_attr(
        record: &opentelemetry_sdk::logs::SdkLogRecord,
        key: &str,
    ) -> Option<AnyValue> {
        record
            .attributes_iter()
            .find(|(k, _)| k.as_str() == key)
            .map(|(_, v)| v.clone())
    }

    fn business_calls(
        records: &[opentelemetry_sdk::logs::SdkLogRecord],
    ) -> Vec<&opentelemetry_sdk::logs::SdkLogRecord> {
        records
            .iter()
            .filter(|r| r.event_name() == Some("senko.api.call"))
            .collect()
    }

    fn request(uri: &str) -> HttpRequest<Body> {
        HttpRequest::builder()
            .uri(uri)
            .method("GET")
            .body(Body::empty())
            .unwrap()
    }

    #[test]
    fn emits_senko_api_call_on_success_with_matched_route() {
        let records = with_log_subscriber(|| async {
            let app: Router = Router::new()
                .route(
                    "/api/v1/projects/{project_id}/tasks/{id}",
                    get(|| async { StatusCode::OK }),
                )
                .layer(from_fn(propagate_trace_context));

            let resp = app
                .oneshot(request("/api/v1/projects/1/tasks/42?cursor=xyz"))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        });

        let calls = business_calls(&records);
        assert_eq!(calls.len(), 1, "expected exactly one senko.api.call record");
        let r = calls[0];
        assert_eq!(
            lookup_log_attr(r, "http.method"),
            Some(AnyValue::String("GET".into()))
        );
        assert_eq!(
            lookup_log_attr(r, "http.route"),
            Some(AnyValue::String(
                "/api/v1/projects/{project_id}/tasks/{id}".into()
            )),
            "http.route must be the matched template, not the actual path"
        );
        assert_eq!(
            lookup_log_attr(r, "http.status_code"),
            Some(AnyValue::Int(200))
        );
        assert!(
            matches!(lookup_log_attr(r, "latency_ms"), Some(AnyValue::Int(_))),
            "latency_ms must be an integer attribute"
        );
        assert_eq!(
            lookup_log_attr(r, "senko.project.id"),
            Some(AnyValue::Int(1))
        );
    }

    #[test]
    fn emits_senko_api_call_on_5xx() {
        let records = with_log_subscriber(|| async {
            let app: Router = Router::new()
                .route("/boom", get(|| async { StatusCode::INTERNAL_SERVER_ERROR }))
                .layer(from_fn(propagate_trace_context));

            let resp = app.oneshot(request("/boom")).await.unwrap();
            assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        });

        let calls = business_calls(&records);
        assert_eq!(calls.len(), 1, "5xx must still produce one senko.api.call");
        assert_eq!(
            lookup_log_attr(calls[0], "http.status_code"),
            Some(AnyValue::Int(500))
        );
        assert_eq!(
            lookup_log_attr(calls[0], "http.route"),
            Some(AnyValue::String("/boom".into()))
        );
        // Phase C2 will introduce senko.api.error; it must NOT exist yet.
        assert!(
            !records
                .iter()
                .any(|r| r.event_name() == Some("senko.api.error")),
            "C2 has not landed yet; no senko.api.error should be emitted"
        );
    }

    #[test]
    fn http_route_template_strips_query_string() {
        let records = with_log_subscriber(|| async {
            let app: Router = Router::new()
                .route(
                    "/api/v1/projects/{project_id}/tasks",
                    get(|| async { StatusCode::OK }),
                )
                .layer(from_fn(propagate_trace_context));

            let _ = app
                .oneshot(request("/api/v1/projects/9/tasks?cursor=secret&limit=5"))
                .await
                .unwrap();
        });

        let r = business_calls(&records)[0];
        assert_eq!(
            lookup_log_attr(r, "http.route"),
            Some(AnyValue::String(
                "/api/v1/projects/{project_id}/tasks".into()
            ))
        );
        // Defensive: query string substrings must not have leaked into any attr.
        for (k, v) in r.attributes_iter() {
            let key = k.as_str();
            let val = match v {
                AnyValue::String(s) => s.to_string(),
                _ => String::new(),
            };
            assert!(
                !key.contains("cursor") && !val.contains("cursor=secret"),
                "query-string token leaked into telemetry: {key}={val:?}"
            );
        }
    }

    #[test]
    fn omits_project_id_when_route_lacks_placeholder() {
        let records = with_log_subscriber(|| async {
            let app: Router = Router::new()
                .route("/api/v1/health", get(|| async { StatusCode::OK }))
                .layer(from_fn(propagate_trace_context));

            let _ = app.oneshot(request("/api/v1/health")).await.unwrap();
        });

        let r = business_calls(&records)[0];
        assert!(
            lookup_log_attr(r, "senko.project.id").is_none(),
            "senko.project.id must be absent when the route has no {{project_id}}"
        );
    }

    #[test]
    fn falls_back_to_literal_path_when_unmatched() {
        let records = with_log_subscriber(|| async {
            let app: Router = Router::new()
                .route("/known", get(|| async { StatusCode::OK }))
                .layer(from_fn(propagate_trace_context));

            let resp = app.oneshot(request("/nope?x=1")).await.unwrap();
            assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        });

        let r = business_calls(&records)
            .first()
            .copied()
            .expect("senko.api.call still emitted on 404");
        assert_eq!(
            lookup_log_attr(r, "http.route"),
            Some(AnyValue::String("/nope".into())),
            "no MatchedPath ⇒ literal path (no query string)",
        );
        assert_eq!(
            lookup_log_attr(r, "http.status_code"),
            Some(AnyValue::Int(404))
        );
    }

    #[test]
    fn http_request_span_uses_http_route_not_target() {
        let spans = with_test_subscriber(|| async {
            let app: Router = Router::new()
                .route(
                    "/api/v1/projects/{project_id}/tasks",
                    get(|| async { StatusCode::OK }),
                )
                .layer(from_fn(propagate_trace_context));

            let _ = app
                .oneshot(request("/api/v1/projects/3/tasks?cursor=xyz"))
                .await
                .unwrap();
        });

        let span = spans.into_iter().next().expect("one span");
        let route = span
            .attributes
            .iter()
            .find(|kv| kv.key.as_str() == "http.route")
            .expect("http.route must be set on the http_request span");
        assert_eq!(route.value.as_str(), "/api/v1/projects/{project_id}/tasks");
        assert!(
            !span
                .attributes
                .iter()
                .any(|kv| kv.key.as_str() == "http.target"),
            "http.target must be removed from http_request span attributes"
        );
    }

    // --- extract_path_param_i64 unit tests ----------------------------------

    #[test]
    fn extract_path_param_i64_finds_named_segment() {
        assert_eq!(
            extract_path_param_i64(
                "/api/v1/projects/{project_id}/tasks/{id}",
                "/api/v1/projects/7/tasks/42",
                "project_id",
            ),
            Some(7),
        );
    }

    #[test]
    fn extract_path_param_i64_returns_none_when_placeholder_missing() {
        assert_eq!(
            extract_path_param_i64("/api/v1/health", "/api/v1/health", "project_id"),
            None,
        );
    }

    #[test]
    fn extract_path_param_i64_returns_none_when_value_not_i64() {
        assert_eq!(
            extract_path_param_i64(
                "/api/v1/projects/{project_id}/tasks",
                "/api/v1/projects/abc/tasks",
                "project_id",
            ),
            None,
        );
    }
}
