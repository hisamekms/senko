//! Relay-side enduser resolution middleware (Contract #8 / Phase E1).
//!
//! When senko runs as a relay (`senko serve --proxy`), it has no local auth
//! provider — it forwards every request to the upstream Remote which holds
//! the canonical identity. Phase C3's `resolve_enduser_middleware` therefore
//! no-ops in proxy mode (`auth_mode == None`), leaving Relay-side OTel
//! events (`senko.api.call`, hook events, etc.) without `enduser.*`.
//!
//! E1 plugs that gap: this middleware calls upstream `/auth/me` with the
//! inbound credentials, caches the result with an LRU + 5-minute TTL, and
//! scopes the resolved principal into the same `RESOLVED_USER` task-local
//! that C3 introduced. From that point on, `propagate_trace_context` can
//! stamp `enduser.*` on the `http_request` span and
//! `BusinessAttributesProcessor` auto-attaches the same attributes to every
//! `senko_business` LogRecord — without any further plumbing.
//!
//! Cache key (in order):
//!
//! 1. Bearer token, JWT-shaped with `sub` claim → `jwt:<sub>` (multiple
//!    sessions for the same user share the cache entry; the JWT signature
//!    is **not** verified — verification is the upstream's job).
//! 2. Bearer token otherwise (API key / opaque) → `tok:<sha256_hex(token)>`.
//! 3. Trusted-headers `subject_header` value → `thv:<value>`.
//!
//! Failures (network error, non-2xx, parse error) are silent: the cache is
//! left untouched and the request flows through without `RESOLVED_USER`,
//! preserving the relay's passthrough invariants (graceful degrade).

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;
use base64::Engine;
use lru::LruCache;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::application::telemetry::{RESOLVED_USER, ResolvedUser};
use crate::infra::config::TrustedHeadersConfig;

/// 5 minutes — see Contract #8 task #359.
const DEFAULT_TTL: Duration = Duration::from_secs(300);
/// 1000 entries — see Contract #8 task #359.
const DEFAULT_CAPACITY: usize = 1000;
/// Short timeout: middleware blocks the inbound request, the default
/// 30s `HttpClient` timeout is far too long here.
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);

// --- AppState trait ----------------------------------------------------------

/// State extension trait that the middleware reads. `AppState` implements
/// this with `Some(_)` only in proxy mode (initialized in
/// `presentation::api::serve_proxy`).
pub trait HasRelayResolver {
    fn relay_resolver(&self) -> Option<&Arc<RelayEnduserResolver>>;
}

// --- Fetcher abstraction -----------------------------------------------------

/// Pluggable upstream `/auth/me` fetcher. Production code uses
/// [`HttpEnduserFetcher`]; tests substitute a stub to observe call counts
/// without spinning up a real upstream server.
#[async_trait::async_trait]
pub trait EnduserFetcher: Send + Sync {
    async fn fetch(&self, headers: &HeaderMap) -> Option<ResolvedUser>;
}

/// Production fetcher: forwards Authorization + relevant trusted-headers
/// verbatim to upstream `/auth/me` and parses `{ user: { id, username } }`
/// from the JSON body.
pub struct HttpEnduserFetcher {
    client: reqwest::Client,
    upstream_url: String,
    trusted_headers_config: Option<Arc<TrustedHeadersConfig>>,
}

impl HttpEnduserFetcher {
    pub fn new(
        upstream_url: String,
        trusted_headers_config: Option<Arc<TrustedHeadersConfig>>,
    ) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(FETCH_TIMEOUT)
            .build()
            .context("failed to build relay /auth/me client")?;
        Ok(Self {
            client,
            upstream_url: upstream_url.trim_end_matches('/').to_string(),
            trusted_headers_config,
        })
    }
}

#[async_trait::async_trait]
impl EnduserFetcher for HttpEnduserFetcher {
    async fn fetch(&self, headers: &HeaderMap) -> Option<ResolvedUser> {
        let url = format!("{}/auth/me", self.upstream_url);
        let mut builder = self.client.get(&url);

        if let Some(auth) = headers.get("authorization") {
            builder = builder.header("authorization", auth.clone());
        }

        if let Some(cfg) = self.trusted_headers_config.as_deref() {
            for name_opt in [
                cfg.subject_header.as_deref(),
                cfg.name_header.as_deref(),
                cfg.display_name_header.as_deref(),
                cfg.email_header.as_deref(),
                cfg.groups_header.as_deref(),
                cfg.scope_header.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                if let Some(val) = headers.get(name_opt) {
                    builder = builder.header(name_opt, val.clone());
                }
            }
        }

        let resp = match builder.send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(error = %e, "relay /auth/me request failed");
                return None;
            }
        };
        if !resp.status().is_success() {
            tracing::debug!(status = %resp.status(), "relay /auth/me non-2xx");
            return None;
        }
        let body = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                tracing::debug!(error = %e, "relay /auth/me body read failed");
                return None;
            }
        };
        let v: serde_json::Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!(error = %e, "relay /auth/me parse failed");
                return None;
            }
        };
        let user = v.get("user")?;
        let id = user.get("id")?.as_i64()?;
        let username = user.get("username")?.as_str()?.to_string();
        Some(ResolvedUser { id, username })
    }
}

// --- Resolver (cache + fetcher orchestration) --------------------------------

struct CachedEntry {
    user: ResolvedUser,
    fetched_at: Instant,
}

/// LRU + TTL cache over an [`EnduserFetcher`]. The TTL is enforced at
/// read-time (`elapsed() < ttl`); expired entries stay in the cache until
/// the LRU evicts them. That is fine for capacity 1000 + 5-minute TTL —
/// no background sweep is needed.
pub struct RelayEnduserResolver {
    fetcher: Arc<dyn EnduserFetcher>,
    cache: Mutex<LruCache<String, CachedEntry>>,
    ttl: Duration,
    trusted_headers_config: Option<Arc<TrustedHeadersConfig>>,
}

impl RelayEnduserResolver {
    pub fn new(
        fetcher: Arc<dyn EnduserFetcher>,
        ttl: Duration,
        capacity: usize,
        trusted_headers_config: Option<Arc<TrustedHeadersConfig>>,
    ) -> Self {
        let capacity = NonZeroUsize::new(capacity.max(1)).expect("non-zero capacity");
        Self {
            fetcher,
            cache: Mutex::new(LruCache::new(capacity)),
            ttl,
            trusted_headers_config,
        }
    }

    /// Production constructor: wraps an [`HttpEnduserFetcher`] with the
    /// default TTL / capacity.
    pub fn http(
        upstream_url: String,
        trusted_headers_config: Option<Arc<TrustedHeadersConfig>>,
    ) -> anyhow::Result<Self> {
        let fetcher: Arc<dyn EnduserFetcher> = Arc::new(HttpEnduserFetcher::new(
            upstream_url,
            trusted_headers_config.clone(),
        )?);
        Ok(Self::new(
            fetcher,
            DEFAULT_TTL,
            DEFAULT_CAPACITY,
            trusted_headers_config,
        ))
    }

    pub async fn resolve(&self, headers: &HeaderMap) -> Option<ResolvedUser> {
        let key = compute_cache_key(headers, self.trusted_headers_config.as_deref())?;

        if let Some(user) = self.cache_get(&key).await {
            return Some(user);
        }

        let user = self.fetcher.fetch(headers).await?;
        self.cache_put(key, user.clone()).await;
        Some(user)
    }

    async fn cache_get(&self, key: &str) -> Option<ResolvedUser> {
        let mut cache = self.cache.lock().await;
        let entry = cache.get(key)?;
        if entry.fetched_at.elapsed() < self.ttl {
            Some(entry.user.clone())
        } else {
            None
        }
    }

    async fn cache_put(&self, key: String, user: ResolvedUser) {
        let mut cache = self.cache.lock().await;
        cache.put(
            key,
            CachedEntry {
                user,
                fetched_at: Instant::now(),
            },
        );
    }
}

// --- Cache-key derivation ----------------------------------------------------

pub(crate) fn compute_cache_key(
    headers: &HeaderMap,
    trusted: Option<&TrustedHeadersConfig>,
) -> Option<String> {
    if let Some(token) = bearer_token(headers) {
        if let Some(sub) = jwt_unverified_sub(&token) {
            return Some(format!("jwt:{sub}"));
        }
        return Some(format!("tok:{}", sha256_hex(&token)));
    }
    if let Some(cfg) = trusted
        && let Some(name) = cfg.subject_header.as_deref()
        && let Some(val) = headers.get(name).and_then(|v| v.to_str().ok())
    {
        return Some(format!("thv:{val}"));
    }
    None
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_owned)
}

/// Decode a JWT's `sub` claim **without verifying** the signature. Used
/// only for cache-key derivation; verification happens upstream.
fn jwt_unverified_sub(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    v.get("sub")?.as_str().map(String::from)
}

fn sha256_hex(s: &str) -> String {
    let digest = Sha256::digest(s.as_bytes());
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

// --- Middleware --------------------------------------------------------------

/// Outermost middleware (paired with C3's `resolve_enduser_middleware`,
/// only one of which is active at a time depending on deployment mode).
/// Resolves the inbound user via `/auth/me` and scopes `RESOLVED_USER` for
/// the entire downstream pipeline. Failures degrade gracefully: the
/// request still flows, just without `enduser.*` on Relay-side telemetry.
pub async fn relay_resolve_enduser_middleware<S>(
    State(state): State<S>,
    req: Request,
    next: Next,
) -> Response
where
    S: HasRelayResolver + Clone + Send + Sync + 'static,
{
    let resolver = match state.relay_resolver() {
        Some(r) => r.clone(),
        None => return next.run(req).await,
    };

    let resolved = resolver.resolve(req.headers()).await;
    match resolved {
        Some(user) => RESOLVED_USER.scope(user, next.run(req)).await,
        None => next.run(req).await,
    }
}

// --- Tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{HeaderValue, Request as HttpRequest, StatusCode};
    use axum::middleware::from_fn_with_state;
    use axum::routing::get;
    use opentelemetry::logs::AnyValue;
    use opentelemetry_sdk::logs::SdkLogRecord;
    use tower::ServiceExt;
    use tracing_subscriber::layer::SubscriberExt;

    use crate::application::telemetry::test_support::{
        build_capture_provider, capture_layer, lookup_attr,
    };

    // --- helpers -----------------------------------------------------------

    /// Stub fetcher: returns canned `Option<ResolvedUser>` and counts calls.
    struct StubFetcher {
        user: Option<ResolvedUser>,
        calls: Arc<AtomicUsize>,
    }

    impl StubFetcher {
        fn new(user: Option<ResolvedUser>) -> (Arc<Self>, Arc<AtomicUsize>) {
            let calls = Arc::new(AtomicUsize::new(0));
            (
                Arc::new(Self {
                    user,
                    calls: calls.clone(),
                }),
                calls,
            )
        }
    }

    #[async_trait]
    impl EnduserFetcher for StubFetcher {
        async fn fetch(&self, _headers: &HeaderMap) -> Option<ResolvedUser> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.user.clone()
        }
    }

    fn header_map_with_bearer(token: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        h
    }

    fn header_map_with_subject(name: &str, value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            HeaderValue::from_str(value).unwrap(),
        );
        h
    }

    fn jwt_with_sub(sub: &str) -> String {
        let header_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(b"{\"alg\":\"none\",\"typ\":\"JWT\"}");
        let payload = serde_json::json!({ "sub": sub }).to_string();
        let payload_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
        // Signature segment is required for the 3-segment shape but never
        // inspected since we don't verify.
        format!("{header_b64}.{payload_b64}.sig")
    }

    fn trusted_with_subject(name: &str) -> TrustedHeadersConfig {
        TrustedHeadersConfig {
            subject_header: Some(name.to_string()),
            ..Default::default()
        }
    }

    // --- compute_cache_key -------------------------------------------------

    #[test]
    fn cache_key_jwt_uses_sub_claim() {
        let token = jwt_with_sub("user-7");
        let key = compute_cache_key(&header_map_with_bearer(&token), None).unwrap();
        assert_eq!(key, "jwt:user-7");
    }

    #[test]
    fn cache_key_opaque_bearer_falls_back_to_sha256() {
        let key = compute_cache_key(&header_map_with_bearer("opaque-key-abc"), None).unwrap();
        assert!(key.starts_with("tok:"));
        // sha256 hex is 64 chars → "tok:" + 64 == 68
        assert_eq!(key.len(), 4 + 64);
    }

    #[test]
    fn cache_key_jwt_without_sub_falls_back_to_sha256() {
        let header_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(b"{\"alg\":\"none\",\"typ\":\"JWT\"}");
        let payload_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"{}");
        let token = format!("{header_b64}.{payload_b64}.sig");
        let key = compute_cache_key(&header_map_with_bearer(&token), None).unwrap();
        assert!(
            key.starts_with("tok:"),
            "no sub → must fall back to token hash, got {key}"
        );
    }

    #[test]
    fn cache_key_trusted_headers_uses_subject_value() {
        let trusted = trusted_with_subject("X-User-Sub");
        let headers = header_map_with_subject("X-User-Sub", "alice");
        let key = compute_cache_key(&headers, Some(&trusted)).unwrap();
        assert_eq!(key, "thv:alice");
    }

    #[test]
    fn cache_key_returns_none_without_credentials() {
        assert!(compute_cache_key(&HeaderMap::new(), None).is_none());

        // trusted-headers configured but the request lacks the subject header
        let trusted = trusted_with_subject("X-User-Sub");
        assert!(compute_cache_key(&HeaderMap::new(), Some(&trusted)).is_none());
    }

    #[test]
    fn cache_key_bearer_takes_precedence_over_trusted() {
        // Trusted-headers also present, but Bearer should win (matches the
        // upstream's auth-mode selection: token mode short-circuits).
        let trusted = trusted_with_subject("X-User-Sub");
        let mut headers = header_map_with_bearer("opaque");
        headers.insert("X-User-Sub", HeaderValue::from_static("ignored"));
        let key = compute_cache_key(&headers, Some(&trusted)).unwrap();
        assert!(key.starts_with("tok:"));
    }

    // --- Resolver cache behavior ------------------------------------------

    #[tokio::test]
    async fn resolver_caches_user_across_repeat_calls() {
        let user = ResolvedUser {
            id: 42,
            username: "alice".into(),
        };
        let (fetcher, calls) = StubFetcher::new(Some(user.clone()));
        let resolver = RelayEnduserResolver::new(fetcher, Duration::from_secs(60), 10, None);

        let h = header_map_with_bearer("opaque");
        let r1 = resolver.resolve(&h).await.unwrap();
        let r2 = resolver.resolve(&h).await.unwrap();

        assert_eq!(r1.id, 42);
        assert_eq!(r2.username, "alice");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "second resolve must be a cache hit"
        );
    }

    #[tokio::test]
    async fn resolver_refetches_after_ttl_expiration() {
        let user = ResolvedUser {
            id: 7,
            username: "bob".into(),
        };
        let (fetcher, calls) = StubFetcher::new(Some(user));
        // Zero TTL forces every read to be considered expired.
        let resolver = RelayEnduserResolver::new(fetcher, Duration::from_nanos(1), 10, None);

        let h = header_map_with_bearer("opaque");
        resolver.resolve(&h).await.unwrap();
        // sleep a hair so elapsed() > 1ns even on fast machines
        tokio::time::sleep(Duration::from_millis(1)).await;
        resolver.resolve(&h).await.unwrap();

        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "expired cache entry must trigger re-fetch"
        );
    }

    #[tokio::test]
    async fn resolver_does_not_cache_fetcher_failures() {
        let (fetcher, calls) = StubFetcher::new(None);
        let resolver = RelayEnduserResolver::new(fetcher, Duration::from_secs(60), 10, None);

        let h = header_map_with_bearer("opaque");
        assert!(resolver.resolve(&h).await.is_none());
        assert!(resolver.resolve(&h).await.is_none());

        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "failure must not be cached; second call hits the fetcher again"
        );
    }

    #[tokio::test]
    async fn resolver_returns_none_when_no_credentials() {
        // Without Authorization (and no trusted-headers config), there's no
        // cache key to derive; the fetcher must NOT be called.
        let (fetcher, calls) = StubFetcher::new(Some(ResolvedUser {
            id: 1,
            username: "x".into(),
        }));
        let resolver = RelayEnduserResolver::new(fetcher, Duration::from_secs(60), 10, None);

        let result = resolver.resolve(&HeaderMap::new()).await;
        assert!(result.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    // --- Middleware integration -------------------------------------------

    #[derive(Clone)]
    struct StubState {
        resolver: Option<Arc<RelayEnduserResolver>>,
    }

    impl HasRelayResolver for StubState {
        fn relay_resolver(&self) -> Option<&Arc<RelayEnduserResolver>> {
            self.resolver.as_ref()
        }
    }

    async fn business_event_handler() -> StatusCode {
        crate::emit_business_event!("senko.task.created", senko.task.id = 99_i64);
        StatusCode::OK
    }

    fn req_with_bearer(token: Option<&str>) -> HttpRequest<Body> {
        let mut b = HttpRequest::builder().uri("/t").method("GET");
        if let Some(t) = token {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        b.body(Body::empty()).unwrap()
    }

    fn capture_business_records(state: StubState, request: HttpRequest<Body>) -> Vec<SdkLogRecord> {
        let (exporter, provider) = build_capture_provider();
        let subscriber = tracing_subscriber::registry().with(capture_layer(&provider));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let _g = tracing::subscriber::set_default(subscriber);
            let router: Router = Router::new()
                .route("/t", get(business_event_handler))
                .layer(from_fn_with_state(
                    state,
                    relay_resolve_enduser_middleware::<StubState>,
                ));
            let resp = router.oneshot(request).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        });

        provider.force_flush().expect("flush ok");
        exporter
            .get_emitted_logs()
            .expect("logs exported")
            .into_iter()
            .map(|l| l.record)
            .collect()
    }

    fn pick_task_created(records: &[SdkLogRecord]) -> &SdkLogRecord {
        records
            .iter()
            .find(|r| r.event_name() == Some("senko.task.created"))
            .expect("handler must emit senko.task.created")
    }

    #[test]
    fn middleware_attaches_enduser_when_resolver_succeeds() {
        let user = ResolvedUser {
            id: 11,
            username: "carol".into(),
        };
        let (fetcher, _) = StubFetcher::new(Some(user));
        let resolver = Arc::new(RelayEnduserResolver::new(
            fetcher,
            Duration::from_secs(60),
            10,
            None,
        ));
        let state = StubState {
            resolver: Some(resolver),
        };

        let records = capture_business_records(state, req_with_bearer(Some("any")));
        let record = pick_task_created(&records);
        assert_eq!(lookup_attr(record, "enduser.id"), Some(AnyValue::Int(11)));
        assert_eq!(
            lookup_attr(record, "enduser.name"),
            Some(AnyValue::String("carol".into()))
        );
    }

    #[test]
    fn middleware_no_op_when_resolver_absent() {
        // Non-proxy deployment: state.resolver = None. Middleware must
        // pass through without scoping RESOLVED_USER.
        let state = StubState { resolver: None };
        let records = capture_business_records(state, req_with_bearer(Some("any")));
        let record = pick_task_created(&records);
        assert!(lookup_attr(record, "enduser.id").is_none());
        assert!(lookup_attr(record, "enduser.name").is_none());
    }

    #[test]
    fn middleware_no_op_when_upstream_fails() {
        // Resolver present but fetcher returns None (upstream 401, network
        // error, parse error). Request still flows; no enduser attached.
        let (fetcher, _) = StubFetcher::new(None);
        let resolver = Arc::new(RelayEnduserResolver::new(
            fetcher,
            Duration::from_secs(60),
            10,
            None,
        ));
        let state = StubState {
            resolver: Some(resolver),
        };

        let records = capture_business_records(state, req_with_bearer(Some("any")));
        let record = pick_task_created(&records);
        assert!(lookup_attr(record, "enduser.id").is_none());
        assert!(lookup_attr(record, "enduser.name").is_none());
    }
}
