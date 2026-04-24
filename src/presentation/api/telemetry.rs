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

use axum::extract::Request;
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

/// Axum middleware — extract inbound trace context, open a server span, and
/// promote baggage to span attributes. Replaces the older `tower_http::trace::TraceLayer`:
/// status + latency are recorded on the span here.
pub async fn propagate_trace_context(req: Request, next: Next) -> Response {
    let parent_cx = opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.extract(&HeaderMapExtractor(req.headers()))
    });

    let method = req.method().clone();
    let uri = req.uri().clone();
    let span = tracing::info_span!(
        "http_request",
        otel.name = %format!("{} {}", method, uri.path()),
        otel.kind = "server",
        http.method = %method,
        http.target = %uri,
        http.status_code = tracing::field::Empty,
        latency_ms = tracing::field::Empty,
    );
    let _ = span.set_parent(parent_cx.clone());

    let baggage = parent_cx.baggage();
    promote_baggage_to_span(&span, baggage, BAGGAGE_VALUE_MAX_LEN);
    let inbound_baggage = collect_inbound_baggage(baggage);

    let start = Instant::now();
    let response = INBOUND_BAGGAGE
        .scope(inbound_baggage, next.run(req).instrument(span.clone()))
        .await;
    let latency = start.elapsed();

    let status = response.status();
    span.record("http.status_code", status.as_u16());
    span.record("latency_ms", latency.as_millis() as u64);

    if status.is_server_error() {
        tracing::error!(
            parent: &span,
            status = status.as_u16(),
            latency_ms = latency.as_millis() as u64,
            "request failed",
        );
    } else {
        tracing::info!(
            parent: &span,
            status = status.as_u16(),
            latency_ms = latency.as_millis() as u64,
            "response",
        );
    }

    response
}

/// Collect baggage entries into a flat `BTreeMap` for re-emission by relay
/// mode. No reserved-namespace filtering here — the CLI already filters, and
/// relay is a passthrough (re-filtering would drop entries the CLI explicitly
/// permitted via `--attr` / `SENKO_TRACE_ATTRIBUTES`).
fn collect_inbound_baggage(baggage: &Baggage) -> BTreeMap<String, String> {
    baggage
        .iter()
        .map(|(key, (value, _metadata))| (key.as_str().to_string(), value.as_str().to_string()))
        .collect()
}

/// Attach each non-reserved baggage entry as a `baggage.<key>` span attribute.
/// Reserved keys (`service.*`, `host.*`, etc. — see `trace_propagation`) are
/// dropped defensively: the CLI already filters them, but a mis-configured
/// client or proxy could still send them, and we never want them to overwrite
/// server-side OTel Resource attributes of the same name.
fn promote_baggage_to_span(span: &tracing::Span, baggage: &Baggage, max_len: usize) {
    for (key, (value, _metadata)) in baggage.iter() {
        let key_str = key.as_str();
        if is_reserved_namespace(key_str) {
            continue;
        }
        let raw = value.as_str();
        let (truncated, was_truncated) = truncate_baggage_value(raw, max_len);
        if was_truncated {
            tracing::warn!(
                key = key_str,
                original_len = raw.len(),
                max = max_len,
                "baggage value truncated",
            );
        }
        span.set_attribute(format!("baggage.{key_str}"), truncated);
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

        let baggage = Baggage::from_iter([
            KeyValue::new("run.id", "r1"),
            KeyValue::new("service.name", "nope"),
        ]);

        {
            let span = tracing::info_span!("manual");
            promote_baggage_to_span(&span, &baggage, BAGGAGE_VALUE_MAX_LEN);
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
}
