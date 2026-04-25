use axum::Json;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::application::port::auth::AuthError;
use crate::application::telemetry::{RESOLVED_USER, ResolvedUser};
use crate::bootstrap::AuthMode;
use crate::domain::user::User;

use super::ErrorBody;

// --- AppState trait for auth extraction ---

pub trait HasAuth {
    fn auth_mode(&self) -> Option<&AuthMode>;
}

// --- AuthUser extractor ---

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user: User,
    #[allow(dead_code)]
    pub groups: Vec<String>,
    #[allow(dead_code)]
    pub scopes: Vec<String>,
    pub is_master: bool,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: HasAuth + Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let mode = match state.auth_mode() {
            Some(m) => m,
            None => {
                return Err(AuthError::MissingToken);
            }
        };

        match mode {
            AuthMode::Token(provider) => {
                let auth_header = parts
                    .headers
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .ok_or(AuthError::MissingToken)?;

                let token = auth_header
                    .strip_prefix("Bearer ")
                    .ok_or(AuthError::InvalidToken)?;

                let auth_result = provider.authenticate(token).await?;
                Ok(AuthUser {
                    user: auth_result.user,
                    groups: Vec::new(),
                    scopes: Vec::new(),
                    is_master: auth_result.is_master,
                })
            }
            AuthMode::TrustedHeaders(provider) => {
                let result = provider.authenticate_from_headers(&parts.headers).await?;
                Ok(AuthUser {
                    user: result.user,
                    groups: result.groups,
                    scopes: result.scopes,
                    is_master: result.is_master,
                })
            }
        }
    }
}

// --- Optional auth extractor ---

#[derive(Debug, Clone)]
pub struct OptionalAuthUser(pub Option<AuthUser>);

impl<S> FromRequestParts<S> for OptionalAuthUser
where
    S: HasAuth + Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        match AuthUser::from_request_parts(parts, state).await {
            Ok(user) => Ok(OptionalAuthUser(Some(user))),
            Err(_) => Ok(OptionalAuthUser(None)),
        }
    }
}

// --- IntoResponse for AuthError (presentation-layer conversion) ---

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AuthError::MissingToken => (StatusCode::UNAUTHORIZED, "missing authorization header"),
            AuthError::InvalidToken => (StatusCode::UNAUTHORIZED, "invalid api key"),
            AuthError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.as_str()),
        };
        (
            status,
            Json(ErrorBody {
                error: message.to_string(),
            }),
        )
            .into_response()
    }
}

// --- enduser resolution middleware (Contract #8 / Phase C3) -----------------

/// Best-effort auth resolution layered as the **outermost** middleware so
/// every business event emitted within the request scope picks up the
/// caller's identity automatically.
///
/// Mirrors `OptionalAuthUser`'s logic but, instead of returning the user, it
/// publishes the resolved principal to the [`RESOLVED_USER`] task-local for
/// the duration of `next.run(req)`. The
/// [`BusinessAttributesProcessor`](crate::application::telemetry::BusinessAttributesProcessor)
/// reads that task-local on every `senko_business` `LogRecord` emit and
/// auto-attaches `enduser.id` / `enduser.name`. Span-attribute attachment
/// happens one layer down, in `propagate_trace_context`, since the
/// `http_request` span is created there.
///
/// This is observability scaffolding — it never rejects. Missing /
/// invalid credentials simply skip the scope, leaving the per-handler
/// `AuthUser` / `OptionalAuthUser` extractors as the canonical auth gates.
///
/// `proxy_mode` (relay) leaves `auth_mode == None`, so this layer no-ops in
/// that deployment; Phase E1 will plug the upstream `/auth/me` resolver in
/// at the same boundary.
pub async fn resolve_enduser_middleware<S>(
    State(state): State<S>,
    req: Request,
    next: Next,
) -> Response
where
    S: HasAuth + Clone + Send + Sync + 'static,
{
    let (mut parts, body) = req.into_parts();
    let resolved = OptionalAuthUser::from_request_parts(&mut parts, &state)
        .await
        .ok()
        .and_then(|OptionalAuthUser(opt)| opt);
    let req = Request::from_parts(parts, body);

    match resolved {
        Some(auth_user) => {
            let id = auth_user.user.id().0;
            let username = auth_user.user.username().0.clone();
            let resolved = ResolvedUser { id, username };
            RESOLVED_USER.scope(resolved, next.run(req)).await
        }
        None => next.run(req).await,
    }
}

#[cfg(test)]
mod resolve_enduser_tests {
    use super::*;

    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::middleware::from_fn_with_state;
    use axum::routing::get;
    use opentelemetry::logs::AnyValue;
    use opentelemetry_sdk::logs::SdkLogRecord;
    use tower::ServiceExt;
    use tracing_subscriber::layer::SubscriberExt;

    use crate::application::port::auth::{AuthProvider, AuthResult};
    use crate::application::telemetry::test_support::{
        build_capture_provider, capture_layer, lookup_attr,
    };
    use crate::domain::user::{User, UserId, Username};

    /// Token-mode auth provider that either returns a fixed user or fails.
    /// Avoids spinning up the real JWT / API-key plumbing.
    struct StubProvider {
        user_id: i64,
        username: &'static str,
        fail: bool,
    }

    #[async_trait]
    impl AuthProvider for StubProvider {
        async fn authenticate(&self, _token: &str) -> std::result::Result<AuthResult, AuthError> {
            if self.fail {
                return Err(AuthError::InvalidToken);
            }
            Ok(AuthResult {
                user: User::new(
                    UserId(self.user_id),
                    Username(self.username.into()),
                    "sub".into(),
                    None,
                    None,
                    "2026-04-25T00:00:00Z".into(),
                ),
                is_master: false,
            })
        }
    }

    #[derive(Clone)]
    struct StubState {
        mode: Option<Arc<AuthMode>>,
    }

    impl HasAuth for StubState {
        fn auth_mode(&self) -> Option<&AuthMode> {
            self.mode.as_deref()
        }
    }

    async fn business_event_handler() -> StatusCode {
        crate::emit_business_event!("senko.task.created", senko.task.id = 99_i64);
        StatusCode::OK
    }

    fn token_mode(provider: StubProvider) -> Option<Arc<AuthMode>> {
        Some(Arc::new(AuthMode::Token(Arc::new(provider))))
    }

    fn req_with_bearer(token: Option<&str>) -> HttpRequest<Body> {
        let mut b = HttpRequest::builder().uri("/t").method("GET");
        if let Some(t) = token {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        b.body(Body::empty()).unwrap()
    }

    /// Run a request through a router wired with `resolve_enduser_middleware`
    /// and the OTel-bridge subscriber, then return the captured records.
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
                    resolve_enduser_middleware::<StubState>,
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
            .expect("handler must have emitted senko.task.created")
    }

    #[test]
    fn middleware_attaches_enduser_for_valid_token() {
        let state = StubState {
            mode: token_mode(StubProvider {
                user_id: 7,
                username: "alice",
                fail: false,
            }),
        };
        let records = capture_business_records(state, req_with_bearer(Some("any")));
        let record = pick_task_created(&records);

        assert_eq!(lookup_attr(record, "enduser.id"), Some(AnyValue::Int(7)));
        assert_eq!(
            lookup_attr(record, "enduser.name"),
            Some(AnyValue::String("alice".into()))
        );
        // Sanity: callsite attribute survives alongside auto-attached ones.
        assert_eq!(
            lookup_attr(record, "senko.task.id"),
            Some(AnyValue::Int(99))
        );
    }

    #[test]
    fn middleware_skips_for_invalid_token() {
        let state = StubState {
            mode: token_mode(StubProvider {
                user_id: 0,
                username: "",
                fail: true,
            }),
        };
        // Middleware is observability-only; an invalid bearer must not
        // short-circuit the handler — the handler-side `AuthUser` extractor
        // remains the canonical 401 gate.
        let records = capture_business_records(state, req_with_bearer(Some("bad")));
        let record = pick_task_created(&records);

        assert!(lookup_attr(record, "enduser.id").is_none());
        assert!(lookup_attr(record, "enduser.name").is_none());
    }

    #[test]
    fn middleware_skips_when_no_credentials() {
        // Public-endpoint shape: auth configured but the request carries no
        // Authorization header. The middleware must not call the provider
        // and must leave RESOLVED_USER unset.
        let state = StubState {
            mode: token_mode(StubProvider {
                user_id: 0,
                username: "",
                fail: true,
            }),
        };
        let records = capture_business_records(state, req_with_bearer(None));
        let record = pick_task_created(&records);

        assert!(lookup_attr(record, "enduser.id").is_none());
        assert!(lookup_attr(record, "enduser.name").is_none());
    }

    #[test]
    fn middleware_skips_when_auth_mode_disabled() {
        // Relay (`proxy_mode = true`) leaves `auth_mode = None`. C3 is a
        // no-op there; Phase E1 will plug `/auth/me`-based resolution at
        // the same boundary.
        let state = StubState { mode: None };
        let records = capture_business_records(state, req_with_bearer(Some("anything")));
        let record = pick_task_created(&records);

        assert!(lookup_attr(record, "enduser.id").is_none());
        assert!(lookup_attr(record, "enduser.name").is_none());
    }
}
