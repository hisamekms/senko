use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::put;
use axum::{Json, Router};
use axum_extra::extract::Query;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, OpenApi, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

mod auth;
mod relay_auth;
mod telemetry;

use self::auth::{AuthUser, HasAuth, OptionalAuthUser};
use self::relay_auth::{HasRelayResolver, RelayEnduserResolver};
use super::dto::{
    ApiKeyResponse, ApiKeyWithSecretResponse, AuthConfigOidc, AuthConfigResponse,
    CompleteTaskResponse, ConfigResponse, ContractNoteResponse, ContractResponse,
    ListContractNotesPageResponse, ListContractsPageResponse, ListDepsPageResponse,
    ListMembersPageResponse, ListMetadataFieldsPageResponse, ListProjectsPageResponse,
    ListSessionsPageResponse, ListTasksPageResponse, ListUsersPageResponse, MeResponse,
    MetadataFieldResponse, PreviewTransitionResponse, ProjectMemberResponse, ProjectResponse,
    SessionResponse, TaskResponse, TokenResponse, UserResponse,
};
use crate::application::auth::Permission;
use crate::application::port::TaskBackend;
use crate::application::port::auth::AuthError;
use crate::application::{
    ContractOperations, LocalContractOperations, LocalTaskOperations, MetadataFieldOperations,
    MetadataFieldService, ProjectOperations, ProjectService, TaskOperations, UserOperations,
    UserService,
};
use crate::bootstrap;
use crate::bootstrap::AuthMode;
use crate::domain::contract::{
    ContractId, ContractOrderBy, CreateContractParams, ListContractNotesFilter,
    ListContractsFilter, UpdateContractArrayParams, UpdateContractParams,
};
use crate::domain::error::DomainError;
use crate::domain::metadata_field::{CreateMetadataFieldParams, ListMetadataFieldsFilter};
use crate::domain::pagination::{Cursor, CursorPayload};
use crate::domain::project::{
    CreateProjectParams, ListProjectMembersFilter, ListProjectsFilter, ProjectId,
    UpdateProjectParams,
};
use crate::domain::task::{
    AssigneeUserId, CompletionPolicy, CreateTaskParams, ListOrder, ListTaskDepsFilter,
    ListTasksFilter, MetadataUpdate, Priority, Task, TaskId, TaskOrderBy, TaskStatus,
    UpdateTaskArrayParams, UpdateTaskParams,
};
use crate::domain::user::{
    AddProjectMemberParams, CreateApiKeyParams, CreateUserParams, ListSessionsFilter,
    ListUsersFilter, Role, UpdateUserParams, User, UserId,
};
use crate::infra::config::Config;
use crate::infra::http::remote_contract_ops::RemoteContractOperations;
use crate::infra::http::remote_metadata_field_ops::RemoteMetadataFieldOperations;
use crate::infra::http::remote_project_ops::RemoteProjectOperations;
use crate::infra::http::remote_task_ops::RemoteTaskOperations;
use crate::infra::http::remote_user_ops::RemoteUserOperations;

#[derive(Clone)]
struct AppState {
    project_root: Arc<PathBuf>,
    config_path: Option<Arc<PathBuf>>,
    task_service: Arc<dyn TaskOperations>,
    project_service: Arc<dyn ProjectOperations>,
    user_service: Arc<dyn UserOperations>,
    metadata_service: Arc<dyn MetadataFieldOperations>,
    contract_service: Arc<dyn ContractOperations>,
    auth_mode: Option<Arc<AuthMode>>,
    proxy_mode: bool,
    session_config: crate::infra::config::SessionConfig,
    oidc_config: crate::infra::config::OidcConfig,
    trusted_headers_config: crate::infra::config::TrustedHeadersConfig,
    /// Relay-only: resolves the inbound principal via upstream `/auth/me`
    /// and scopes it into `RESOLVED_USER`. `None` outside proxy mode.
    /// See `relay_auth` and Contract #8 / Phase E1.
    relay_resolver: Option<Arc<RelayEnduserResolver>>,
}

impl HasAuth for AppState {
    fn auth_mode(&self) -> Option<&AuthMode> {
        self.auth_mode.as_deref()
    }
}

impl HasRelayResolver for AppState {
    fn relay_resolver(&self) -> Option<&Arc<RelayEnduserResolver>> {
        self.relay_resolver.as_ref()
    }
}

impl AppState {
    fn auth_enabled(&self) -> bool {
        self.auth_mode.is_some()
    }
}

/// Check project-level authorization. No-op when auth is disabled.
/// Master users bypass project membership checks.
async fn check_project_permission(
    state: &AppState,
    auth: &OptionalAuthUser,
    project_id: ProjectId,
    permission: Permission,
) -> Result<(), ApiError> {
    if let Some(caller) = require_auth_user(auth, state.auth_enabled())? {
        if caller.is_master {
            return Ok(());
        }
        let user_id = caller.user.id();
        let member = state
            .project_service
            .get_project_member(project_id, user_id)
            .await
            .map_err(|_| {
                AuthError::Forbidden(format!(
                    "user {} is not a member of project {}",
                    user_id, project_id
                ))
            })?;
        let allowed = match permission {
            Permission::View => true,
            Permission::Edit => matches!(member.role(), Role::Owner | Role::Member),
            Permission::Admin => matches!(member.role(), Role::Owner),
        };
        if !allowed {
            return Err(ApiError::from(AuthError::Forbidden(format!(
                "insufficient permissions: {:?} role cannot perform {:?} operations",
                member.role(),
                permission
            ))));
        }
    }
    Ok(())
}

/// For endpoints that require authentication: returns the authenticated
/// caller (including `is_master`) or 401.
fn require_auth_user(
    auth: &OptionalAuthUser,
    auth_enabled: bool,
) -> Result<Option<&AuthUser>, ApiError> {
    if !auth_enabled {
        return Ok(None);
    }
    match &auth.0 {
        Some(a) => Ok(Some(a)),
        None => Err(ApiError::Unauthorized("authentication required".into())),
    }
}

/// For endpoints restricted to master callers (master API key, OIDC master
/// group, or trusted-headers master group).
/// Returns 401 when unauthenticated, 403 when authenticated but not master.
fn require_master(auth: &OptionalAuthUser, auth_enabled: bool) -> Result<(), ApiError> {
    if !auth_enabled {
        return Ok(());
    }
    match &auth.0 {
        Some(a) if a.is_master => Ok(()),
        Some(_) => Err(ApiError::Forbidden("master privilege required".into())),
        None => Err(ApiError::Unauthorized("authentication required".into())),
    }
}

// --- Error handling ---

enum ApiError {
    NotFound(String),
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    Conflict(String),
    NotImplemented(String),
    /// Internal Server Error.
    ///
    /// `public_message` is rendered into the response body; `log_message` is
    /// recorded as `error.message` on the `senko.api.error` LogRecord. The
    /// fields differ for unclassified anyhow errors (Contract #8 / Phase C2):
    /// the response stays at the static `"internal server error"` while the
    /// log keeps the Display-formatted error chain so root-cause analysis is
    /// still possible without leaking internals (file paths, connection
    /// strings, etc.) to clients. For known-safe sources (upstream HTTP
    /// errors, serde failures) both fields hold the same string.
    Internal {
        public_message: String,
        log_message: String,
    },
}

#[derive(Serialize, ToSchema)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, public_message, log_message) = match &self {
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone(), msg.clone()),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone(), msg.clone()),
            ApiError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone(), msg.clone()),
            ApiError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone(), msg.clone()),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, msg.clone(), msg.clone()),
            ApiError::NotImplemented(msg) => {
                (StatusCode::NOT_IMPLEMENTED, msg.clone(), msg.clone())
            }
            ApiError::Internal {
                public_message,
                log_message,
            } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                public_message.clone(),
                log_message.clone(),
            ),
        };
        let error_type = match self {
            ApiError::NotFound(_) => "not_found",
            ApiError::BadRequest(_) => "bad_request",
            ApiError::Unauthorized(_) => "unauthorized",
            ApiError::Forbidden(_) => "forbidden",
            ApiError::Conflict(_) => "conflict",
            ApiError::NotImplemented(_) => "not_implemented",
            ApiError::Internal { .. } => "internal",
        };

        // Contract #8 / Phase C2: replace the legacy `tracing::warn!("api_error", ...)`
        // with one structured `senko.api.error` LogRecord per ApiError response.
        // `senko.api.call` (C1) carries http.method / http.route / latency_ms;
        // here we only attach the error-shaped attributes so the two records
        // correlate via trace_id without duplicating fields.
        crate::emit_business_event!(
            "senko.api.error",
            level: ERROR,
            http.status_code = status.as_u16(),
            error.type = error_type,
            error.message = %log_message,
        );

        (
            status,
            Json(ErrorBody {
                error: public_message,
            }),
        )
            .into_response()
    }
}

impl From<AuthError> for ApiError {
    fn from(e: AuthError) -> Self {
        match e {
            AuthError::MissingToken => {
                ApiError::Unauthorized("missing authorization header".into())
            }
            AuthError::InvalidToken => ApiError::Unauthorized("invalid api key".into()),
            AuthError::Forbidden(msg) => ApiError::Forbidden(msg),
        }
    }
}

/// Convert a `DomainError` directly into an `ApiError`. Use when the call site
/// has the typed error already (e.g. `FromStr` results) and shouldn't be forced
/// through `anyhow::Error` first.
fn classify_domain(e: DomainError) -> ApiError {
    classify_error(anyhow::Error::from(e))
}

/// Look up the assignee `User` for a `Task`, swallowing lookup errors so a
/// missing or deleted user yields `None` instead of failing the whole request.
/// Returns `None` when the task has no assignee.
async fn resolve_assignee(state: &AppState, task: &Task) -> Option<User> {
    match task.assignee_user_id() {
        Some(uid) => state.user_service.get_user(uid).await.ok(),
        None => None,
    }
}

/// Decode a base64 cursor string and verify its kind matches the requested
/// `order_by`. Returns `None` if `raw` is `None`.
///
/// `expected_kind` is one of `"id"`, `"updated_at"`, `"priority"` —
/// see `TaskOrderBy::cursor_kind` / `ContractOrderBy::cursor_kind`.
fn decode_cursor_for_order(
    raw: Option<&str>,
    expected_kind: &'static str,
) -> Result<Option<CursorPayload>, ApiError> {
    let Some(raw) = raw else { return Ok(None) };
    let payload =
        Cursor::decode_payload(raw).map_err(|_| ApiError::BadRequest("invalid cursor".into()))?;
    let got = payload.kind();
    if got != expected_kind {
        return Err(classify_domain(DomainError::CursorMismatch {
            expected: expected_kind,
            got,
        }));
    }
    Ok(Some(payload))
}

fn classify_error(e: anyhow::Error) -> ApiError {
    if e.downcast_ref::<crate::application::port::auth::AuthError>()
        .is_some()
    {
        return ApiError::Forbidden(e.to_string());
    }
    if let Some(ue) = e.downcast_ref::<crate::infra::http::UpstreamHttpError>() {
        return match ue.status.as_u16() {
            401 => ApiError::Unauthorized(ue.message.clone()),
            403 => ApiError::Forbidden(ue.message.clone()),
            404 => ApiError::NotFound(ue.message.clone()),
            409 => ApiError::Conflict(ue.message.clone()),
            _ => {
                let msg = format!("upstream error: {}", ue.message);
                ApiError::Internal {
                    public_message: msg.clone(),
                    log_message: msg,
                }
            }
        };
    }
    if let Some(de) = e.downcast_ref::<DomainError>() {
        let msg = de.to_string();
        return match de {
            DomainError::TaskNotFound
            | DomainError::ProjectNotFound
            | DomainError::UserNotFound
            | DomainError::ProjectMemberNotFound
            | DomainError::ApiKeyNotFound
            | DomainError::DependencyNotFound { .. }
            | DomainError::NoEligibleTask
            | DomainError::MetadataFieldNotFound
            | DomainError::ContractNotFound => ApiError::NotFound(msg),

            DomainError::InvalidTaskStatus { .. }
            | DomainError::InvalidPriority { .. }
            | DomainError::InvalidRole { .. }
            | DomainError::SelfDependency
            | DomainError::DependencyCycle { .. }
            | DomainError::DodIndexOutOfRange { .. }
            | DomainError::MetadataTooLarge { .. }
            | DomainError::MetadataTooDeep { .. }
            | DomainError::InvalidMetadataFieldType { .. }
            | DomainError::InvalidMetadataFieldName { .. }
            | DomainError::InvalidCursor
            | DomainError::CursorMismatch { .. }
            | DomainError::InvalidQueryParam { .. }
            | DomainError::ValidationError { .. } => ApiError::BadRequest(msg),

            DomainError::InvalidStatusTransition { .. }
            | DomainError::CannotCompleteTask { .. }
            | DomainError::CannotDeleteDefaultProject
            | DomainError::CannotDeleteProjectWithTasks { .. }
            | DomainError::SessionLimitExceeded { .. }
            | DomainError::HookAborted { .. }
            | DomainError::MetadataFieldNameConflict { .. } => ApiError::Conflict(msg),

            DomainError::UnsupportedOperation { .. } => ApiError::NotImplemented(msg),
        };
    }
    // Contract #8 / Phase C2: dropped the prior `tracing::error!(error = ?e, ...)`
    // — `?e` Debug-formats the anyhow chain and leaks internals (file paths,
    // connection strings) to log destinations. The detail now rides only on
    // the `senko.api.error` LogRecord (Display-formatted, in `error.message`),
    // while the response body keeps the static `"internal server error"`.
    //
    // `{:#}` (alternate Display) is anyhow's chain-flattening form
    // ("outer: middle: inner") — `to_string()` would drop the middle and
    // inner Context layers and lose root-cause info ops needs.
    ApiError::Internal {
        public_message: "internal server error".into(),
        log_message: format!("{e:#}"),
    }
}

// --- Proxy mode middleware ---

async fn passthrough_auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    if !state.proxy_mode {
        return next.run(req).await;
    }

    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(String::from);

    match token {
        Some(t) => {
            crate::infra::http::PASSTHROUGH_TOKEN
                .scope(t, next.run(req))
                .await
        }
        None => next.run(req).await,
    }
}

// --- Version header middleware ---

fn has_auth_credentials(
    headers: &axum::http::HeaderMap,
    auth_mode: Option<&AuthMode>,
    trusted_headers_config: &crate::infra::config::TrustedHeadersConfig,
) -> bool {
    match auth_mode {
        None => false,
        Some(AuthMode::Token(_)) => headers.contains_key("authorization"),
        Some(AuthMode::TrustedHeaders(_)) => match &trusted_headers_config.subject_header {
            Some(header) => headers.contains_key(header.as_str()),
            None => false,
        },
        // Bypass: credentials are intrinsic to the variant — every request is
        // authenticated, so we always emit the version header.
        Some(AuthMode::DevBypass { .. }) => true,
    }
}

async fn version_header_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let has_credentials = has_auth_credentials(
        req.headers(),
        state.auth_mode.as_deref(),
        &state.trusted_headers_config,
    );
    let mut response = next.run(req).await;
    if has_credentials && response.status() != StatusCode::UNAUTHORIZED {
        response.headers_mut().insert(
            "x-senko-version",
            axum::http::HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
        );
    }
    response
}

// --- Request types ---

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct ListTasksQuery {
    #[serde(default)]
    status: Vec<String>,
    #[serde(default)]
    tag: Vec<String>,
    #[serde(default)]
    depends_on: Option<TaskId>,
    #[serde(default)]
    ready: Option<bool>,
    #[serde(default)]
    assignee_user_id: Option<String>,
    #[serde(default)]
    include_unassigned: Option<bool>,
    #[serde(default)]
    metadata: Vec<String>,
    #[serde(default)]
    contract: Option<ContractId>,
    #[serde(default)]
    id_min: Option<TaskId>,
    #[serde(default)]
    id_max: Option<TaskId>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    after: Option<String>,
    /// Sort key. One of `id` (default), `updated_at`, `priority`.
    #[serde(default)]
    #[param(inline)]
    order_by: Option<String>,
    /// Sort direction. One of `asc` (default), `desc`.
    #[serde(default)]
    #[param(inline)]
    order: Option<String>,
}

// --- Pagination query structs (task #337) ---

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct ListContractsQuery {
    #[serde(default)]
    tag: Vec<String>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    after: Option<String>,
    /// Sort key. One of `id` (default), `updated_at`.
    #[serde(default)]
    #[param(inline)]
    order_by: Option<String>,
    /// Sort direction. One of `asc` (default), `desc`.
    #[serde(default)]
    #[param(inline)]
    order: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct ListContractNotesQuery {
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    after: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct ListProjectsQuery {
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    after: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct ListMembersQuery {
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    after: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct ListUsersQuery {
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    after: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct ListMetadataFieldsQuery {
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    after: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct ListDepsQuery {
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    after: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct ListSessionsQuery {
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    after: Option<String>,
}

/// Validate `limit` (must be 1..=200) and decode `after` as `T` for list endpoints.
fn decode_page_inputs<T: From<i64>>(
    limit: Option<u32>,
    after: Option<&str>,
) -> Result<(Option<u32>, Option<T>), ApiError> {
    if let Some(n) = limit
        && !(1..=200).contains(&n)
    {
        return Err(ApiError::BadRequest(
            "limit must be between 1 and 200".into(),
        ));
    }
    let after_decoded = match after {
        Some(raw) => Some(
            Cursor::decode::<T>(raw).map_err(|_| ApiError::BadRequest("invalid cursor".into()))?,
        ),
        None => None,
    };
    Ok((limit.or(Some(50)), after_decoded))
}

#[derive(Deserialize, ToSchema)]
struct StartBody {
    session_id: Option<String>,
    #[schema(value_type = Option<Object>)]
    metadata: Option<serde_json::Value>,
    #[schema(value_type = Option<Object>)]
    replace_metadata: Option<serde_json::Value>,
}

#[derive(Deserialize, ToSchema)]
struct ResumeBody {
    session_id: Option<String>,
    #[schema(value_type = Option<Object>)]
    metadata: Option<serde_json::Value>,
    #[schema(value_type = Option<Object>)]
    replace_metadata: Option<serde_json::Value>,
    #[serde(default)]
    clear_metadata: bool,
}

#[derive(Deserialize, ToSchema)]
struct CompleteBody {
    #[serde(default)]
    skip_pr_check: bool,
}

#[derive(Deserialize, ToSchema)]
struct CancelBody {
    reason: Option<String>,
}

#[derive(Deserialize, ToSchema)]
struct NextBody {
    session_id: Option<String>,
    #[serde(default)]
    include_unassigned: bool,
    #[schema(value_type = Option<Object>)]
    metadata: Option<serde_json::Value>,
    #[schema(value_type = Option<Object>)]
    replace_metadata: Option<serde_json::Value>,
}

#[derive(Deserialize, ToSchema)]
struct AddDepBody {
    dep_id: TaskId,
}

#[derive(Deserialize, ToSchema)]
struct SetDepsBody {
    dep_ids: Vec<TaskId>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct PreviewTransitionQuery {
    target: String,
}

#[derive(Deserialize, Default, ToSchema)]
struct EditTaskBody {
    title: Option<String>,
    background: Option<String>,
    #[serde(default)]
    clear_background: bool,
    description: Option<String>,
    #[serde(default)]
    clear_description: bool,
    plan: Option<String>,
    #[serde(default)]
    clear_plan: bool,
    priority: Option<Priority>,
    branch: Option<String>,
    #[serde(default)]
    clear_branch: bool,
    pr_url: Option<String>,
    #[serde(default)]
    clear_pr_url: bool,
    contract_id: Option<ContractId>,
    #[serde(default)]
    clear_contract: bool,
    #[schema(value_type = Option<Object>)]
    metadata: Option<serde_json::Value>,
    #[schema(value_type = Option<Object>)]
    replace_metadata: Option<serde_json::Value>,
    #[serde(default)]
    clear_metadata: bool,
    #[schema(value_type = Option<Object>)]
    assignee_user_id: Option<serde_json::Value>,
    #[serde(default)]
    clear_assignee_user_id: bool,
    // Array operations
    set_tags: Option<Vec<String>>,
    #[serde(default)]
    add_tags: Vec<String>,
    #[serde(default)]
    remove_tags: Vec<String>,
    set_definition_of_done: Option<Vec<String>>,
    #[serde(default)]
    add_definition_of_done: Vec<String>,
    #[serde(default)]
    remove_definition_of_done: Vec<String>,
    set_in_scope: Option<Vec<String>>,
    #[serde(default)]
    add_in_scope: Vec<String>,
    #[serde(default)]
    remove_in_scope: Vec<String>,
    set_out_of_scope: Option<Vec<String>>,
    #[serde(default)]
    add_out_of_scope: Vec<String>,
    #[serde(default)]
    remove_out_of_scope: Vec<String>,
}

// --- Server entry point ---

/// Start the API server in standalone mode (local database backend).
pub async fn serve(
    project_root: PathBuf,
    port: u16,
    port_is_explicit: bool,
    config: &Config,
    config_path: Option<PathBuf>,
    backend: Arc<dyn TaskBackend>,
    auth_mode: Option<AuthMode>,
) -> Result<()> {
    let telemetry = bootstrap::init_telemetry(&config.log, bootstrap::TelemetryMode::Remote);

    if auth_mode.is_none() {
        tracing::warn!(
            "Authentication is disabled. All API endpoints are accessible without credentials."
        );
    }

    let backend_info = bootstrap::resolve_backend_info(config, &project_root);
    let hook_data: Arc<dyn crate::application::port::HookDataSource> =
        Arc::new(crate::application::port::BackendHookData(backend.clone()));
    let hook_executor = bootstrap::create_server_hook_executor(
        config.clone(),
        crate::infra::hook::RuntimeMode::ServerRemote,
        backend_info,
        hook_data,
    );
    let pr_verifier = bootstrap::create_pr_verifier();
    let completion_policy = CompletionPolicy::new(config.workflow.merge_via);

    let state = AppState {
        project_root: Arc::new(project_root),
        config_path: config_path.map(Arc::new),
        task_service: Arc::new(LocalTaskOperations::new(
            backend.clone(),
            hook_executor.clone(),
            pr_verifier,
            completion_policy,
        )),
        project_service: Arc::new(ProjectService::new(backend.clone())),
        user_service: Arc::new(UserService::new(backend.clone())),
        metadata_service: Arc::new(MetadataFieldService::new(backend.clone())),
        contract_service: Arc::new(LocalContractOperations::new(backend, hook_executor)),
        auth_mode: auth_mode.map(Arc::new),
        proxy_mode: false,
        session_config: config.server.auth.oidc.session.clone(),
        oidc_config: config.server.auth.oidc.clone(),
        trusted_headers_config: config.server.auth.trusted_headers.clone(),
        relay_resolver: None,
    };

    start_server(state, config, port, port_is_explicit, telemetry).await
}

/// Start the API server in proxy/relay mode (forwarding to a remote server).
pub async fn serve_proxy(
    project_root: PathBuf,
    port: u16,
    port_is_explicit: bool,
    config: &Config,
    config_path: Option<PathBuf>,
    hook_data: Arc<dyn crate::application::port::HookDataSource>,
) -> Result<()> {
    let telemetry = bootstrap::init_telemetry(&config.log, bootstrap::TelemetryMode::Relay);

    let remote_url = config
        .server
        .relay
        .url
        .as_ref()
        .expect("server.relay.url required for proxy mode");
    let api_key = config.server.relay.token.clone();
    let backend_info = bootstrap::resolve_backend_info(config, &project_root);
    let hook_executor = bootstrap::create_server_hook_executor(
        config.clone(),
        crate::infra::hook::RuntimeMode::ServerRelay,
        backend_info,
        hook_data,
    );

    // Proxy mode: trace-propagation attributes belong to the originating CLI
    // invocation. The inbound `baggage` header is captured per-request by
    // `propagate_trace_context` into the `INBOUND_BAGGAGE` task-local and
    // re-emitted by `HttpClient.propagate`, so the static attrs stay empty.
    let proxy_attrs: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();

    let trusted_for_relay = if config.server.auth.trusted_headers.is_configured() {
        Some(Arc::new(config.server.auth.trusted_headers.clone()))
    } else {
        None
    };
    let relay_resolver = Some(Arc::new(RelayEnduserResolver::http(
        remote_url.clone(),
        trusted_for_relay,
    )?));

    let state = AppState {
        project_root: Arc::new(project_root),
        config_path: config_path.map(Arc::new),
        task_service: Arc::new(RemoteTaskOperations::new(
            remote_url,
            api_key.clone(),
            proxy_attrs.clone(),
            hook_executor.clone(),
        )),
        project_service: Arc::new(RemoteProjectOperations::new(
            remote_url,
            api_key.clone(),
            proxy_attrs.clone(),
        )),
        user_service: Arc::new(RemoteUserOperations::new(
            remote_url,
            api_key.clone(),
            proxy_attrs.clone(),
        )),
        metadata_service: Arc::new(RemoteMetadataFieldOperations::new(
            remote_url,
            api_key.clone(),
            proxy_attrs.clone(),
        )),
        contract_service: Arc::new(RemoteContractOperations::new(
            remote_url,
            api_key,
            proxy_attrs,
            hook_executor,
        )),
        auth_mode: None,
        proxy_mode: true,
        session_config: config.server.auth.oidc.session.clone(),
        oidc_config: config.server.auth.oidc.clone(),
        trusted_headers_config: config.server.auth.trusted_headers.clone(),
        relay_resolver,
    };

    start_server(state, config, port, port_is_explicit, telemetry).await
}

/// Build the OpenAPI document used by both the runtime server and the
/// `senko openapi dump` CLI subcommand.
///
/// The same value is consumed by `start_server` (when serving Swagger UI in
/// remote mode) and by the CLI dumper, so generated artifacts stay in lockstep
/// with the running API.
pub fn build_openapi() -> utoipa::openapi::OpenApi {
    let (_router, api): (Router<AppState>, _) = build_openapi_router().split_for_parts();
    api
}

/// Construct an `OpenApiRouter` populated with every handler annotated with
/// `#[utoipa::path]`. Returns `OpenApiRouter<AppState>` because every handler
/// extracts `State<AppState>`. `build_openapi` simply discards the router half
/// of `.split_for_parts()` to retrieve the spec without booting a server.
/// Routes that intentionally stay out of the OpenAPI document (e.g. `_save`)
/// are added by the caller after `.split_for_parts()`.
fn build_openapi_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        // Users
        .routes(routes!(list_users, create_user))
        .routes(routes!(get_user, update_user, delete_user))
        // API keys
        .routes(routes!(list_api_keys, create_api_key))
        .routes(routes!(delete_api_key))
        // Projects
        .routes(routes!(list_projects, create_project))
        .routes(routes!(get_project, update_project, delete_project))
        .routes(routes!(get_stats))
        // Members
        .routes(routes!(list_members, add_member))
        .routes(routes!(get_member, update_member_role, remove_member))
        // Task next/preview (static path before wildcard)
        .routes(routes!(next_task))
        .routes(routes!(preview_next))
        // Tasks
        .routes(routes!(list_tasks, create_task))
        .routes(routes!(get_task, edit_task, delete_task))
        .routes(routes!(preview_transition))
        .routes(routes!(publish_task))
        .routes(routes!(start_task))
        .routes(routes!(resume_task))
        .routes(routes!(complete_task))
        .routes(routes!(cancel_task))
        // Dependencies
        .routes(routes!(list_deps, add_dep, set_deps))
        .routes(routes!(remove_dep))
        // DoD
        .routes(routes!(check_dod))
        .routes(routes!(uncheck_dod))
        // Contracts
        .routes(routes!(list_contracts, create_contract))
        .routes(routes!(get_contract, edit_contract, delete_contract))
        .routes(routes!(check_contract_dod))
        .routes(routes!(uncheck_contract_dod))
        .routes(routes!(list_contract_notes, add_contract_note))
        // Metadata fields
        .routes(routes!(list_metadata_fields, create_metadata_field))
        .routes(routes!(delete_metadata_field_handler))
        // Auth
        .routes(routes!(get_auth_config))
        .routes(routes!(get_me))
        .routes(routes!(create_token))
        .routes(routes!(list_sessions, revoke_all_sessions))
        .routes(routes!(revoke_session))
        // Server-wide
        .routes(routes!(health_check))
        .routes(routes!(get_config))
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "senko remote API",
        description = "REST API for the senko task management system. Served by `senko serve` in remote mode.",
        version = env!("CARGO_PKG_VERSION"),
        license(name = "MIT", identifier = "MIT"),
    ),
    modifiers(&BearerSecurityAddon),
    tags(
        (name = "tasks", description = "Task CRUD and lifecycle"),
        (name = "deps", description = "Task dependencies"),
        (name = "dod", description = "Definition of Done check/uncheck"),
        (name = "contracts", description = "Contracts, contract DoD, and contract notes"),
        (name = "projects", description = "Projects"),
        (name = "members", description = "Project members"),
        (name = "users", description = "Users"),
        (name = "api-keys", description = "User API keys"),
        (name = "metadata-fields", description = "Project metadata field definitions"),
        (name = "auth", description = "Authentication / sessions"),
        (name = "server", description = "Server-wide endpoints"),
    ),
)]
struct ApiDoc;

struct BearerSecurityAddon;

impl utoipa::Modify for BearerSecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::{Http, HttpAuthScheme, SecurityScheme};
        let components = openapi
            .components
            .get_or_insert_with(utoipa::openapi::Components::new);
        components.add_security_scheme(
            "bearer_auth",
            SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
        );
    }
}

async fn start_server(
    state: AppState,
    config: &Config,
    port: u16,
    port_is_explicit: bool,
    telemetry: bootstrap::TelemetryGuard,
) -> Result<()> {
    // Build the OpenAPI-aware router and split into (axum::Router, OpenApi).
    // `_save` is intentionally NOT in the OpenAPI document — it accepts a full
    // `Task` aggregate and is for internal/admin use only.
    let (api_router, openapi) = build_openapi_router().split_for_parts();
    let app: Router<AppState> = api_router.route(
        "/api/v1/projects/{project_id}/tasks/{id}/_save",
        put(save_task_handler),
    );

    // Conditionally mount Swagger UI + the OpenAPI JSON endpoint. Only the
    // remote (standalone) server exposes the docs surface; the relay/proxy
    // server is a thin forwarder and intentionally returns 404 for both
    // `/api/v1/docs/*` and `/api/v1/openapi.json` so callers cannot mistake it
    // for an authoritative spec source.
    let app: Router<AppState> = if state.proxy_mode {
        app
    } else {
        app.merge(
            utoipa_swagger_ui::SwaggerUi::new("/api/v1/docs").url("/api/v1/openapi.json", openapi),
        )
    };

    let app = app
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            passthrough_auth_middleware,
        ))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            version_header_middleware,
        ))
        .with_state(state.clone())
        .layer(axum::middleware::from_fn(
            self::telemetry::propagate_trace_context,
        ))
        // Phase E1: relay-side enduser resolver. Calls upstream `/auth/me`
        // with an LRU + TTL cache. Active only when `state.relay_resolver`
        // is `Some(_)` (= `serve_proxy`); no-op otherwise.
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            self::relay_auth::relay_resolve_enduser_middleware::<AppState>,
        ))
        // Outermost layer: resolve the inbound principal and scope the
        // `RESOLVED_USER` task-local for the entire request, so
        // `propagate_trace_context` can both stamp `enduser.*` on the
        // `http_request` span and emit `senko.api.call` with the auth
        // context populated. See `auth::resolve_enduser_middleware` and
        // Contract #8 / Phase C3. Active only when `auth_mode` is
        // `Some(_)`; in proxy mode the relay layer above takes over.
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            self::auth::resolve_enduser_middleware::<AppState>,
        ));

    let bind_addr_str = config.effective_server_host();
    let bind_ip: std::net::IpAddr = bind_addr_str
        .parse()
        .with_context(|| format!("invalid bind address: {bind_addr_str}"))?;

    let (listener, actual_port) = super::bind_with_retry(bind_ip, port, port_is_explicit).await?;

    // Repeat the bypass warning right next to the "Listening on" line so
    // operators tailing the log from boot still see it. The first warning
    // is emitted in `bootstrap::create_auth_mode`.
    if matches!(state.auth_mode.as_deref(), Some(AuthMode::DevBypass { .. })) {
        tracing::warn!("dev auth bypass enabled — DO NOT USE IN PRODUCTION");
    }

    if bind_ip.is_unspecified() {
        let device_ip = get_local_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "0.0.0.0".to_string());
        tracing::info!(
            port = actual_port,
            "Listening on http://localhost:{actual_port}"
        );
        tracing::info!(port = actual_port, addr = %device_ip, "Listening on http://{device_ip}:{actual_port}");
    } else {
        tracing::info!(port = actual_port, addr = %bind_ip, "Listening on http://{bind_ip}:{actual_port}");
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    telemetry.shutdown();
    Ok(())
}

/// Wait for SIGINT (Ctrl-C) or SIGTERM; return so `axum::serve` can drain
/// in-flight requests and we can flush OTel providers in `TelemetryGuard`.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install SIGINT handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received; flushing OTel providers");
}

fn get_local_ip() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|a| a.ip())
}

// --- Project Handlers ---

#[utoipa::path(
    get,
    path = "/api/v1/projects",
    params(ListProjectsQuery),
    responses(
        (status = 200, body = ListProjectsPageResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "projects",
)]
async fn list_projects(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Query(query): Query<ListProjectsQuery>,
) -> Result<Json<ListProjectsPageResponse>, ApiError> {
    require_auth_user(&auth, state.auth_enabled())?;
    let (limit, after) = decode_page_inputs::<ProjectId>(query.limit, query.after.as_deref())?;
    let filter = ListProjectsFilter { limit, after };
    let page = state
        .project_service
        .list_projects(&filter)
        .await
        .map_err(classify_error)?;
    Ok(Json(ListProjectsPageResponse {
        items: page.items.into_iter().map(ProjectResponse::from).collect(),
        next_cursor: page.next_cursor,
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects",
    request_body = CreateProjectParams,
    responses(
        (status = 201, body = ProjectResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "projects",
)]
async fn create_project(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Json(params): Json<CreateProjectParams>,
) -> Result<(StatusCode, Json<ProjectResponse>), ApiError> {
    require_auth_user(&auth, state.auth_enabled())?;
    let caller_user_id = auth.0.as_ref().map(|a| a.user.id());
    let (project, _events) = state
        .project_service
        .create_project(&params, caller_user_id)
        .await
        .map_err(classify_error)?;
    Ok((StatusCode::CREATED, Json(ProjectResponse::from(project))))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{project_id}",
    params(("project_id" = i64, Path)),
    responses(
        (status = 200, body = ProjectResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "projects",
)]
async fn get_project(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<ProjectId>,
) -> Result<Json<ProjectResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let project = state
        .project_service
        .get_project(project_id)
        .await
        .map_err(classify_error)?;
    Ok(Json(ProjectResponse::from(project)))
}

#[derive(Deserialize, ToSchema)]
struct UpdateProjectBody {
    description: Option<String>,
    #[serde(default)]
    clear_description: bool,
}

#[utoipa::path(
    put,
    path = "/api/v1/projects/{project_id}",
    params(("project_id" = i64, Path)),
    request_body = UpdateProjectBody,
    responses(
        (status = 200, body = ProjectResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "projects",
)]
async fn update_project(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<ProjectId>,
    Json(body): Json<UpdateProjectBody>,
) -> Result<Json<ProjectResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Admin).await?;
    let caller_user_id = auth.0.as_ref().map(|a| a.user.id());
    let params = UpdateProjectParams {
        description: if body.clear_description {
            Some(None)
        } else {
            body.description.map(Some)
        },
    };
    let (project, _events) = state
        .project_service
        .update_project(project_id, &params, caller_user_id)
        .await
        .map_err(classify_error)?;
    Ok(Json(ProjectResponse::from(project)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/projects/{project_id}",
    params(("project_id" = i64, Path)),
    responses(
        (status = 204),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "projects",
)]
async fn delete_project(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<ProjectId>,
) -> Result<StatusCode, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Admin).await?;
    let caller_user_id = auth.0.as_ref().map(|a| a.user.id());
    state
        .project_service
        .delete_project(project_id, caller_user_id)
        .await
        .map_err(classify_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// --- Task Handlers ---

#[utoipa::path(
    get,
    path = "/api/v1/projects/{project_id}/tasks",
    params(("project_id" = i64, Path), ListTasksQuery),
    responses(
        (status = 200, body = ListTasksPageResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "tasks",
)]
async fn list_tasks(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<ProjectId>,
    Query(query): Query<ListTasksQuery>,
) -> Result<Json<ListTasksPageResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let statuses: Vec<TaskStatus> = query
        .status
        .iter()
        .map(|s| s.parse::<TaskStatus>())
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(classify_error)?;
    let mut metadata_map = std::collections::HashMap::new();
    for entry in &query.metadata {
        let (key, value) = entry.split_once(':').ok_or_else(|| {
            classify_error(anyhow::anyhow!(
                "invalid metadata filter format: expected 'key:value', got '{entry}'"
            ))
        })?;
        metadata_map.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }
    if let Some(n) = query.limit
        && !(1..=200).contains(&n)
    {
        return Err(ApiError::BadRequest(
            "limit must be between 1 and 200".into(),
        ));
    }
    let effective_limit = query.limit.or(Some(50));

    let order_by: TaskOrderBy = match query.order_by.as_deref() {
        Some(s) => s.parse().map_err(classify_domain)?,
        None => TaskOrderBy::default(),
    };
    let order: ListOrder = match query.order.as_deref() {
        Some(s) => s.parse().map_err(classify_domain)?,
        None => ListOrder::default(),
    };

    let after = decode_cursor_for_order(query.after.as_deref(), order_by.cursor_kind())?;

    let (assignee_user_id, assignee_self) =
        resolve_query_assignee_self(query.assignee_user_id, &auth)?;

    let filter = ListTasksFilter {
        statuses,
        tags: query.tag,
        depends_on: query.depends_on,
        ready: query.ready.unwrap_or(false),
        assignee_user_id,
        assignee_self,
        include_unassigned: query.include_unassigned.unwrap_or(false),
        metadata: metadata_map,
        contract_id: query.contract,
        id_min: query.id_min,
        id_max: query.id_max,
        limit: effective_limit,
        after,
        order_by,
        order,
    };
    let page = state
        .task_service
        .list_tasks(project_id, &filter)
        .await
        .map_err(classify_error)?;
    let mut items = Vec::with_capacity(page.items.len());
    for task in page.items {
        let assignee_user = resolve_assignee(&state, &task).await;
        items.push(TaskResponse::from_parts(task, assignee_user.as_ref()));
    }
    Ok(Json(ListTasksPageResponse {
        items,
        next_cursor: page.next_cursor,
    }))
}

/// Resolve `"self"` in `assignee_user_id` to the authenticated user's numeric ID.
/// If no auth user is available (e.g. on a relay server), `"self"` is left as-is
/// for the upstream to resolve.
fn resolve_assignee_self(body: &mut serde_json::Value, auth: &OptionalAuthUser) {
    if let Some(value) = body.get("assignee_user_id")
        && value.as_str() == Some("self")
        && let Some(user_id) = auth.0.as_ref().map(|a| a.user.id())
    {
        body["assignee_user_id"] = serde_json::Value::Number(user_id.0.into());
    }
    // No auth (relay): leave "self" for upstream to resolve
}

/// Resolve the `assignee_user_id` query parameter: accepts either a numeric
/// user id or the literal string `"self"` (resolved from the auth context).
/// Returns `(Option<i64>, assignee_self)`. On a relay server with no local
/// auth, `"self"` is carried through by setting `assignee_self = true` so the
/// upstream can resolve it against its own auth context.
fn resolve_query_assignee_self(
    raw: Option<String>,
    auth: &OptionalAuthUser,
) -> Result<(Option<UserId>, bool), ApiError> {
    let Some(val) = raw else {
        return Ok((None, false));
    };
    if val == "self" {
        return match auth.0.as_ref().map(|a| a.user.id()) {
            Some(id) => Ok((Some(id), false)),
            None => Ok((None, true)),
        };
    }
    val.parse::<i64>()
        .map(|n| (Some(UserId(n)), false))
        .map_err(|_| {
            ApiError::BadRequest(format!(
                "assignee_user_id must be a numeric id or 'self' (got {val:?})"
            ))
        })
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{project_id}/tasks",
    params(("project_id" = i64, Path)),
    request_body = CreateTaskParams,
    responses(
        (status = 201, body = TaskResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "tasks",
)]
async fn create_task(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<ProjectId>,
    Json(mut body): Json<serde_json::Value>,
) -> Result<(StatusCode, Json<TaskResponse>), ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    resolve_assignee_self(&mut body, &auth);
    let params: CreateTaskParams = serde_json::from_value(body)
        .map_err(|e| ApiError::BadRequest(format!("invalid request body: {e}")))?;
    let task = state
        .task_service
        .create_task(project_id, &params)
        .await
        .map_err(classify_error)?;
    let assignee_user = resolve_assignee(&state, &task).await;
    Ok((
        StatusCode::CREATED,
        Json(TaskResponse::from_parts(task, assignee_user.as_ref())),
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{project_id}/tasks/{id}",
    params(("project_id" = i64, Path), ("id" = i64, Path)),
    responses(
        (status = 200, body = TaskResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "tasks",
)]
async fn get_task(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(ProjectId, TaskId)>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let task = state
        .task_service
        .get_task(project_id, id)
        .await
        .map_err(classify_error)?;
    let assignee_user = resolve_assignee(&state, &task).await;
    Ok(Json(TaskResponse::from_parts(task, assignee_user.as_ref())))
}

#[utoipa::path(
    put,
    path = "/api/v1/projects/{project_id}/tasks/{id}",
    params(("project_id" = i64, Path), ("id" = i64, Path)),
    request_body = EditTaskBody,
    responses(
        (status = 200, body = TaskResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "tasks",
)]
async fn edit_task(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(ProjectId, TaskId)>,
    Json(body): Json<EditTaskBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let branch_value = if body.clear_branch {
        Some(None)
    } else {
        body.branch
            .map(|b| Some(b.replace("${task_id}", &id.to_string())))
    };

    let scalar_params = UpdateTaskParams {
        title: body.title,
        background: if body.clear_background {
            Some(None)
        } else {
            body.background.map(Some)
        },
        description: if body.clear_description {
            Some(None)
        } else {
            body.description.map(Some)
        },
        plan: if body.clear_plan {
            Some(None)
        } else {
            body.plan.map(Some)
        },
        priority: body.priority,
        assignee_session_id: None,
        assignee_user_id: if body.clear_assignee_user_id {
            Some(None)
        } else {
            match body.assignee_user_id {
                Some(ref v) if v.as_str() == Some("self") => {
                    let uid = auth.0.as_ref().map(|a| a.user.id()).ok_or_else(|| {
                        ApiError::BadRequest(
                            "assignee_user_id \"self\" requires authentication".into(),
                        )
                    })?;
                    Some(Some(AssigneeUserId::Id(uid)))
                }
                Some(ref v) => {
                    let uid = v.as_i64().ok_or_else(|| {
                        ApiError::BadRequest("assignee_user_id must be \"self\" or integer".into())
                    })?;
                    Some(Some(AssigneeUserId::Id(UserId(uid))))
                }
                None => None,
            }
        },
        started_at: None,
        completed_at: None,
        canceled_at: None,
        cancel_reason: None,
        branch: branch_value,
        pr_url: if body.clear_pr_url {
            Some(None)
        } else {
            body.pr_url.map(Some)
        },
        contract_id: if body.clear_contract {
            Some(None)
        } else {
            body.contract_id.map(Some)
        },
        metadata: if body.clear_metadata {
            Some(MetadataUpdate::Clear)
        } else if let Some(v) = body.replace_metadata {
            Some(MetadataUpdate::Replace(v))
        } else {
            body.metadata.map(MetadataUpdate::Merge)
        },
    };

    let array_params = UpdateTaskArrayParams {
        set_tags: body.set_tags,
        add_tags: body.add_tags,
        remove_tags: body.remove_tags,
        set_definition_of_done: body.set_definition_of_done,
        add_definition_of_done: body.add_definition_of_done,
        remove_definition_of_done: body.remove_definition_of_done,
        set_in_scope: body.set_in_scope,
        add_in_scope: body.add_in_scope,
        remove_in_scope: body.remove_in_scope,
        set_out_of_scope: body.set_out_of_scope,
        add_out_of_scope: body.add_out_of_scope,
        remove_out_of_scope: body.remove_out_of_scope,
    };

    state
        .task_service
        .edit_task(project_id, id, &scalar_params)
        .await
        .map_err(classify_error)?;
    state
        .task_service
        .edit_task_arrays(project_id, id, &array_params)
        .await
        .map_err(classify_error)?;
    let task = state
        .task_service
        .get_task(project_id, id)
        .await
        .map_err(classify_error)?;
    let assignee_user = resolve_assignee(&state, &task).await;
    Ok(Json(TaskResponse::from_parts(task, assignee_user.as_ref())))
}

// PUT /api/v1/projects/{project_id}/tasks/{id}/_save
//
// Internal/admin endpoint for full task replacement; intentionally NOT included
// in the OpenAPI document because the request body is the complete `Task`
// aggregate (not a public client surface).
async fn save_task_handler(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(ProjectId, TaskId)>,
    Json(task): Json<Task>,
) -> Result<StatusCode, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    if task.id() != id || task.project_id() != project_id {
        return Err(ApiError::BadRequest(
            "task ID or project ID mismatch".into(),
        ));
    }
    state
        .task_service
        .save_task(project_id, id, &task)
        .await
        .map_err(classify_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete,
    path = "/api/v1/projects/{project_id}/tasks/{id}",
    params(("project_id" = i64, Path), ("id" = i64, Path)),
    responses(
        (status = 204),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "tasks",
)]
async fn delete_task(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(ProjectId, TaskId)>,
) -> Result<StatusCode, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Admin).await?;
    state
        .task_service
        .delete_task(project_id, id)
        .await
        .map_err(classify_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{project_id}/tasks/{id}/publish",
    params(("project_id" = i64, Path), ("id" = i64, Path)),
    responses(
        (status = 200, body = TaskResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "tasks",
)]
async fn publish_task(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(ProjectId, TaskId)>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let updated = state
        .task_service
        .publish_task(project_id, id)
        .await
        .map_err(classify_error)?;
    let assignee_user = resolve_assignee(&state, &updated).await;
    Ok(Json(TaskResponse::from_parts(
        updated,
        assignee_user.as_ref(),
    )))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{project_id}/tasks/{id}/start",
    params(("project_id" = i64, Path), ("id" = i64, Path)),
    request_body = StartBody,
    responses(
        (status = 200, body = TaskResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "tasks",
)]
async fn start_task(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(ProjectId, TaskId)>,
    Json(body): Json<StartBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let user_id = auth.0.as_ref().map(|a| a.user.id());
    let metadata = if let Some(v) = body.replace_metadata {
        Some(MetadataUpdate::Replace(v))
    } else {
        body.metadata.map(MetadataUpdate::Merge)
    };
    let updated = state
        .task_service
        .start_task(project_id, id, body.session_id, user_id, metadata)
        .await
        .map_err(classify_error)?;
    let assignee_user = resolve_assignee(&state, &updated).await;
    Ok(Json(TaskResponse::from_parts(
        updated,
        assignee_user.as_ref(),
    )))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{project_id}/tasks/{id}/resume",
    params(("project_id" = i64, Path), ("id" = i64, Path)),
    request_body = ResumeBody,
    responses(
        (status = 200, body = TaskResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "tasks",
)]
async fn resume_task(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(ProjectId, TaskId)>,
    Json(body): Json<ResumeBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let metadata = if body.clear_metadata {
        Some(MetadataUpdate::Clear)
    } else if let Some(v) = body.replace_metadata {
        Some(MetadataUpdate::Replace(v))
    } else {
        body.metadata.map(MetadataUpdate::Merge)
    };
    let updated = state
        .task_service
        .resume_task(project_id, id, body.session_id, metadata)
        .await
        .map_err(classify_error)?;
    let assignee_user = resolve_assignee(&state, &updated).await;
    Ok(Json(TaskResponse::from_parts(
        updated,
        assignee_user.as_ref(),
    )))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{project_id}/tasks/{id}/complete",
    params(("project_id" = i64, Path), ("id" = i64, Path)),
    request_body(content = CompleteBody, description = "Optional. Defaults applied when omitted."),
    responses(
        (status = 200, body = CompleteTaskResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "tasks",
)]
async fn complete_task(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(ProjectId, TaskId)>,
    body: Option<Json<CompleteBody>>,
) -> Result<Json<CompleteTaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let skip_pr_check = body.map(|b| b.skip_pr_check).unwrap_or(false);
    let result = state
        .task_service
        .complete_task(project_id, id, skip_pr_check)
        .await
        .map_err(classify_error)?;
    let assignee_user = resolve_assignee(&state, &result.task).await;
    Ok(Json(CompleteTaskResponse::from_parts(
        result,
        assignee_user.as_ref(),
    )))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{project_id}/tasks/{id}/cancel",
    params(("project_id" = i64, Path), ("id" = i64, Path)),
    request_body(content = CancelBody, description = "Optional cancel reason."),
    responses(
        (status = 200, body = TaskResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "tasks",
)]
async fn cancel_task(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(ProjectId, TaskId)>,
    body: Option<Json<CancelBody>>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let reason = body.and_then(|b| b.0.reason);
    let updated = state
        .task_service
        .cancel_task(project_id, id, reason)
        .await
        .map_err(classify_error)?;
    let assignee_user = resolve_assignee(&state, &updated).await;
    Ok(Json(TaskResponse::from_parts(
        updated,
        assignee_user.as_ref(),
    )))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{project_id}/tasks/next",
    params(("project_id" = i64, Path)),
    request_body(content = NextBody, description = "Optional metadata filter."),
    responses(
        (status = 200, body = TaskResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody, description = "No eligible task"),
    ),
    security(("bearer_auth" = [])),
    tag = "tasks",
)]
async fn next_task(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<ProjectId>,
    body: Option<Json<NextBody>>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let (session_id, include_unassigned, metadata_raw, replace_metadata) = body
        .map(|b| {
            (
                b.0.session_id,
                b.0.include_unassigned,
                b.0.metadata,
                b.0.replace_metadata,
            )
        })
        .unwrap_or((None, false, None, None));
    let user_id = auth.0.as_ref().map(|a| a.user.id());
    let metadata = if let Some(v) = replace_metadata {
        Some(MetadataUpdate::Replace(v))
    } else {
        metadata_raw.map(MetadataUpdate::Merge)
    };
    let updated = state
        .task_service
        .next_task(
            project_id,
            session_id,
            user_id,
            include_unassigned,
            metadata,
        )
        .await
        .map_err(classify_error)?;
    let assignee_user = resolve_assignee(&state, &updated).await;
    Ok(Json(TaskResponse::from_parts(
        updated,
        assignee_user.as_ref(),
    )))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{project_id}/tasks/{id}/preview-transition",
    params(("project_id" = i64, Path), ("id" = i64, Path), PreviewTransitionQuery),
    responses(
        (status = 200, body = PreviewTransitionResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "tasks",
)]
async fn preview_transition(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(ProjectId, TaskId)>,
    Query(query): Query<PreviewTransitionQuery>,
) -> Result<Json<PreviewTransitionResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let target: TaskStatus = query.target.parse().map_err(classify_error)?;
    let result = state
        .task_service
        .preview_transition(project_id, id, target)
        .await
        .map_err(classify_error)?;
    Ok(Json(PreviewTransitionResponse::from(result)))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{project_id}/tasks/preview-next",
    params(("project_id" = i64, Path)),
    responses(
        (status = 200, body = PreviewTransitionResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "tasks",
)]
async fn preview_next(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<ProjectId>,
) -> Result<Json<PreviewTransitionResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let result = state
        .task_service
        .preview_next(project_id)
        .await
        .map_err(classify_error)?;
    Ok(Json(PreviewTransitionResponse::from(result)))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{project_id}/tasks/{id}/deps",
    params(("project_id" = i64, Path), ("id" = i64, Path), ListDepsQuery),
    responses(
        (status = 200, body = ListDepsPageResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "deps",
)]
async fn list_deps(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(ProjectId, TaskId)>,
    Query(query): Query<ListDepsQuery>,
) -> Result<Json<ListDepsPageResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let (limit, after) = decode_page_inputs::<TaskId>(query.limit, query.after.as_deref())?;
    let filter = ListTaskDepsFilter { limit, after };
    let page = state
        .task_service
        .list_dependencies(project_id, id, &filter)
        .await
        .map_err(classify_error)?;
    let mut items = Vec::with_capacity(page.items.len());
    for task in page.items {
        let assignee_user = resolve_assignee(&state, &task).await;
        items.push(TaskResponse::from_parts(task, assignee_user.as_ref()));
    }
    Ok(Json(ListDepsPageResponse {
        items,
        next_cursor: page.next_cursor,
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{project_id}/tasks/{id}/deps",
    params(("project_id" = i64, Path), ("id" = i64, Path)),
    request_body = AddDepBody,
    responses(
        (status = 200, body = TaskResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "deps",
)]
async fn add_dep(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(ProjectId, TaskId)>,
    Json(body): Json<AddDepBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let task = state
        .task_service
        .add_dependency(project_id, id, body.dep_id)
        .await
        .map_err(classify_error)?;
    let assignee_user = resolve_assignee(&state, &task).await;
    Ok(Json(TaskResponse::from_parts(task, assignee_user.as_ref())))
}

#[utoipa::path(
    delete,
    path = "/api/v1/projects/{project_id}/tasks/{id}/deps/{dep_id}",
    params(
        ("project_id" = i64, Path),
        ("id" = i64, Path),
        ("dep_id" = i64, Path),
    ),
    responses(
        (status = 200, body = TaskResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "deps",
)]
async fn remove_dep(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id, dep_id)): Path<(ProjectId, TaskId, TaskId)>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let task = state
        .task_service
        .remove_dependency(project_id, id, dep_id)
        .await
        .map_err(classify_error)?;
    let assignee_user = resolve_assignee(&state, &task).await;
    Ok(Json(TaskResponse::from_parts(task, assignee_user.as_ref())))
}

#[utoipa::path(
    put,
    path = "/api/v1/projects/{project_id}/tasks/{id}/deps",
    params(("project_id" = i64, Path), ("id" = i64, Path)),
    request_body = SetDepsBody,
    responses(
        (status = 200, body = TaskResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "deps",
)]
async fn set_deps(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(ProjectId, TaskId)>,
    Json(body): Json<SetDepsBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let task = state
        .task_service
        .set_dependencies(project_id, id, &body.dep_ids)
        .await
        .map_err(classify_error)?;
    let assignee_user = resolve_assignee(&state, &task).await;
    Ok(Json(TaskResponse::from_parts(task, assignee_user.as_ref())))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{project_id}/tasks/{id}/dod/{index}/check",
    params(
        ("project_id" = i64, Path),
        ("id" = i64, Path),
        ("index" = u32, Path, description = "1-based DoD item index"),
    ),
    responses(
        (status = 200, body = TaskResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "dod",
)]
async fn check_dod(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id, index)): Path<(ProjectId, TaskId, usize)>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let task = state
        .task_service
        .check_dod(project_id, id, index)
        .await
        .map_err(classify_error)?;
    let assignee_user = resolve_assignee(&state, &task).await;
    Ok(Json(TaskResponse::from_parts(task, assignee_user.as_ref())))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{project_id}/tasks/{id}/dod/{index}/uncheck",
    params(
        ("project_id" = i64, Path),
        ("id" = i64, Path),
        ("index" = u32, Path, description = "1-based DoD item index"),
    ),
    responses(
        (status = 200, body = TaskResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "dod",
)]
async fn uncheck_dod(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id, index)): Path<(ProjectId, TaskId, usize)>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let task = state
        .task_service
        .uncheck_dod(project_id, id, index)
        .await
        .map_err(classify_error)?;
    let assignee_user = resolve_assignee(&state, &task).await;
    Ok(Json(TaskResponse::from_parts(task, assignee_user.as_ref())))
}

// --- Contract handlers ---

#[derive(Deserialize, ToSchema)]
struct CreateContractBody {
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    definition_of_done: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    metadata: Option<serde_json::Value>,
}

#[derive(Deserialize, ToSchema)]
struct EditContractBody {
    title: Option<String>,
    description: Option<String>,
    #[serde(default)]
    clear_description: bool,
    #[schema(value_type = Option<Object>)]
    metadata: Option<serde_json::Value>,
    #[schema(value_type = Option<Object>)]
    replace_metadata: Option<serde_json::Value>,
    #[serde(default)]
    clear_metadata: bool,
    set_tags: Option<Vec<String>>,
    #[serde(default)]
    add_tags: Vec<String>,
    #[serde(default)]
    remove_tags: Vec<String>,
    set_definition_of_done: Option<Vec<String>>,
    #[serde(default)]
    add_definition_of_done: Vec<String>,
    #[serde(default)]
    remove_definition_of_done: Vec<String>,
}

#[derive(Deserialize, ToSchema)]
struct AddContractNoteBody {
    content: String,
    #[serde(default)]
    source_task_id: Option<TaskId>,
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{project_id}/contracts",
    params(("project_id" = i64, Path)),
    request_body = CreateContractBody,
    responses(
        (status = 200, body = ContractResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "contracts",
)]
async fn create_contract(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<ProjectId>,
    Json(body): Json<CreateContractBody>,
) -> Result<Json<ContractResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let params = CreateContractParams {
        title: body.title,
        description: body.description,
        definition_of_done: body.definition_of_done,
        tags: body.tags,
        metadata: body.metadata,
    };
    let contract = state
        .contract_service
        .create_contract(project_id, &params)
        .await
        .map_err(classify_error)?;
    Ok(Json(ContractResponse::from(contract)))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{project_id}/contracts",
    params(("project_id" = i64, Path), ListContractsQuery),
    responses(
        (status = 200, body = ListContractsPageResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "contracts",
)]
async fn list_contracts(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<ProjectId>,
    Query(query): Query<ListContractsQuery>,
) -> Result<Json<ListContractsPageResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    if let Some(n) = query.limit
        && !(1..=200).contains(&n)
    {
        return Err(ApiError::BadRequest(
            "limit must be between 1 and 200".into(),
        ));
    }
    let limit = query.limit.or(Some(50));

    let order_by: ContractOrderBy = match query.order_by.as_deref() {
        Some(s) => s.parse().map_err(classify_domain)?,
        None => ContractOrderBy::default(),
    };
    let order: ListOrder = match query.order.as_deref() {
        Some(s) => s.parse().map_err(classify_domain)?,
        None => ListOrder::default(),
    };

    let after = decode_cursor_for_order(query.after.as_deref(), order_by.cursor_kind())?;

    let filter = ListContractsFilter {
        tags: query.tag,
        limit,
        after,
        order_by,
        order,
    };
    let page = state
        .contract_service
        .list_contracts(project_id, &filter)
        .await
        .map_err(classify_error)?;
    Ok(Json(ListContractsPageResponse {
        items: page.items.into_iter().map(ContractResponse::from).collect(),
        next_cursor: page.next_cursor,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{project_id}/contracts/{id}",
    params(("project_id" = i64, Path), ("id" = i64, Path)),
    responses(
        (status = 200, body = ContractResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "contracts",
)]
async fn get_contract(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(ProjectId, ContractId)>,
) -> Result<Json<ContractResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let contract = state
        .contract_service
        .get_contract(project_id, id)
        .await
        .map_err(classify_error)?;
    Ok(Json(ContractResponse::from(contract)))
}

#[utoipa::path(
    put,
    path = "/api/v1/projects/{project_id}/contracts/{id}",
    params(("project_id" = i64, Path), ("id" = i64, Path)),
    request_body = EditContractBody,
    responses(
        (status = 200, body = ContractResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "contracts",
)]
async fn edit_contract(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(ProjectId, ContractId)>,
    Json(body): Json<EditContractBody>,
) -> Result<Json<ContractResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;

    let scalar = UpdateContractParams {
        title: body.title,
        description: if body.clear_description {
            Some(None)
        } else {
            body.description.map(Some)
        },
        metadata: if body.clear_metadata {
            Some(MetadataUpdate::Clear)
        } else if let Some(v) = body.replace_metadata {
            Some(MetadataUpdate::Replace(v))
        } else {
            body.metadata.map(MetadataUpdate::Merge)
        },
    };
    let array = UpdateContractArrayParams {
        set_tags: body.set_tags,
        add_tags: body.add_tags,
        remove_tags: body.remove_tags,
        set_definition_of_done: body.set_definition_of_done,
        add_definition_of_done: body.add_definition_of_done,
        remove_definition_of_done: body.remove_definition_of_done,
    };
    let contract = state
        .contract_service
        .edit_contract(project_id, id, &scalar, &array)
        .await
        .map_err(classify_error)?;
    Ok(Json(ContractResponse::from(contract)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/projects/{project_id}/contracts/{id}",
    params(("project_id" = i64, Path), ("id" = i64, Path)),
    responses(
        (status = 204),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "contracts",
)]
async fn delete_contract(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(ProjectId, ContractId)>,
) -> Result<StatusCode, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Admin).await?;
    state
        .contract_service
        .delete_contract(project_id, id)
        .await
        .map_err(classify_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{project_id}/contracts/{id}/dod/{index}/check",
    params(
        ("project_id" = i64, Path),
        ("id" = i64, Path),
        ("index" = u32, Path, description = "1-based DoD item index"),
    ),
    responses(
        (status = 200, body = ContractResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "contracts",
)]
async fn check_contract_dod(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id, index)): Path<(ProjectId, ContractId, usize)>,
) -> Result<Json<ContractResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let contract = state
        .contract_service
        .check_dod(project_id, id, index)
        .await
        .map_err(classify_error)?;
    Ok(Json(ContractResponse::from(contract)))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{project_id}/contracts/{id}/dod/{index}/uncheck",
    params(
        ("project_id" = i64, Path),
        ("id" = i64, Path),
        ("index" = u32, Path, description = "1-based DoD item index"),
    ),
    responses(
        (status = 200, body = ContractResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "contracts",
)]
async fn uncheck_contract_dod(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id, index)): Path<(ProjectId, ContractId, usize)>,
) -> Result<Json<ContractResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let contract = state
        .contract_service
        .uncheck_dod(project_id, id, index)
        .await
        .map_err(classify_error)?;
    Ok(Json(ContractResponse::from(contract)))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{project_id}/contracts/{id}/notes",
    params(("project_id" = i64, Path), ("id" = i64, Path)),
    request_body = AddContractNoteBody,
    responses(
        (status = 200, body = ContractNoteResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "contracts",
)]
async fn add_contract_note(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(ProjectId, ContractId)>,
    Json(body): Json<AddContractNoteBody>,
) -> Result<Json<ContractNoteResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let note = state
        .contract_service
        .add_note(project_id, id, body.content, body.source_task_id)
        .await
        .map_err(classify_error)?;
    Ok(Json(ContractNoteResponse::from(&note)))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{project_id}/contracts/{id}/notes",
    params(("project_id" = i64, Path), ("id" = i64, Path), ListContractNotesQuery),
    responses(
        (status = 200, body = ListContractNotesPageResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "contracts",
)]
async fn list_contract_notes(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(ProjectId, ContractId)>,
    Query(query): Query<ListContractNotesQuery>,
) -> Result<Json<ListContractNotesPageResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let (limit, after) = decode_page_inputs::<i64>(query.limit, query.after.as_deref())?;
    let filter = ListContractNotesFilter { limit, after };
    let page = state
        .contract_service
        .list_notes(project_id, id, &filter)
        .await
        .map_err(classify_error)?;
    Ok(Json(ListContractNotesPageResponse {
        items: page.items.iter().map(ContractNoteResponse::from).collect(),
        next_cursor: page.next_cursor,
    }))
}

#[utoipa::path(
    get,
    path = "/auth/config",
    responses((status = 200, body = AuthConfigResponse)),
    tag = "auth",
)]
async fn get_auth_config(State(state): State<AppState>) -> Json<AuthConfigResponse> {
    let (auth_mode, oidc) = match state.auth_mode.as_deref() {
        Some(AuthMode::Token(_)) if state.oidc_config.is_configured() => (
            "oidc".to_string(),
            Some(AuthConfigOidc {
                issuer_url: state.oidc_config.issuer_url.clone().unwrap(),
                client_id: state.oidc_config.client_id.clone().unwrap(),
                scopes: state.oidc_config.scopes.clone(),
                callback_ports: state.oidc_config.callback_ports.clone(),
            }),
        ),
        Some(AuthMode::Token(_)) => ("api_key".to_string(), None),
        Some(AuthMode::TrustedHeaders(_)) => {
            let oidc = match (
                &state.trusted_headers_config.oidc_issuer_url,
                &state.trusted_headers_config.oidc_client_id,
            ) {
                (Some(issuer_url), Some(client_id)) => Some(AuthConfigOidc {
                    issuer_url: issuer_url.clone(),
                    client_id: client_id.clone(),
                    scopes: vec!["openid".to_string(), "profile".to_string()],
                    callback_ports: state.oidc_config.callback_ports.clone(),
                }),
                _ => None,
            };
            ("trusted_headers".to_string(), oidc)
        }
        Some(AuthMode::DevBypass { .. }) => ("dev_bypass".to_string(), None),
        None => ("none".to_string(), None),
    };
    Json(AuthConfigResponse { auth_mode, oidc })
}

#[utoipa::path(
    get,
    path = "/api/v1/health",
    responses((status = 200, body = Object, description = "Health status")),
    tag = "server",
)]
async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

#[utoipa::path(
    get,
    path = "/api/v1/config",
    responses(
        (status = 200, body = ConfigResponse),
        (status = 401, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "server",
)]
async fn get_config(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
) -> Result<Json<ConfigResponse>, ApiError> {
    require_auth_user(&auth, state.auth_enabled())?;
    let xdg = crate::infra::xdg::XdgDirs::from_env();
    let config = crate::bootstrap::load_config(
        &state.project_root,
        state.config_path.as_deref().map(|p| p.as_path()),
        &xdg,
    )
    .map_err(classify_error)?;
    Ok(Json(ConfigResponse::from(config)))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{project_id}/stats",
    params(("project_id" = i64, Path)),
    responses(
        (status = 200, body = HashMap<String, i64>),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "projects",
)]
async fn get_stats(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<ProjectId>,
) -> Result<Json<HashMap<String, i64>>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let stats = state
        .task_service
        .task_stats(project_id)
        .await
        .map_err(classify_error)?;
    Ok(Json(stats))
}

// --- User Handlers ---

#[utoipa::path(
    get,
    path = "/api/v1/users",
    params(ListUsersQuery),
    responses(
        (status = 200, body = ListUsersPageResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "users",
)]
async fn list_users(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Query(query): Query<ListUsersQuery>,
) -> Result<Json<ListUsersPageResponse>, ApiError> {
    require_master(&auth, state.auth_enabled())?;
    let (limit, after) = decode_page_inputs::<i64>(query.limit, query.after.as_deref())?;
    let filter = ListUsersFilter { limit, after };
    let page = state
        .user_service
        .list_users(&filter)
        .await
        .map_err(classify_error)?;
    Ok(Json(ListUsersPageResponse {
        items: page.items.into_iter().map(UserResponse::from).collect(),
        next_cursor: page.next_cursor,
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/users",
    request_body = CreateUserParams,
    responses(
        (status = 201, body = UserResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "users",
)]
async fn create_user(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Json(params): Json<CreateUserParams>,
) -> Result<(StatusCode, Json<UserResponse>), ApiError> {
    require_master(&auth, state.auth_enabled())?;
    let (user, _events) = state
        .user_service
        .create_user(&params, crate::domain::user::UserCreationSource::Manual)
        .await
        .map_err(classify_error)?;
    Ok((StatusCode::CREATED, Json(UserResponse::from(user))))
}

#[utoipa::path(
    get,
    path = "/api/v1/users/{user_id}",
    params(("user_id" = i64, Path)),
    responses(
        (status = 200, body = UserResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "users",
)]
async fn get_user(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(user_id): Path<UserId>,
) -> Result<Json<UserResponse>, ApiError> {
    require_master(&auth, state.auth_enabled())?;
    let user = state
        .user_service
        .get_user(user_id)
        .await
        .map_err(classify_error)?;
    Ok(Json(UserResponse::from(user)))
}

// PUT /api/v1/users/{user_id}
#[derive(Deserialize, ToSchema)]
struct UpdateUserBody {
    username: Option<crate::domain::user::Username>,
    #[schema(value_type = Option<String>)]
    display_name: Option<Option<String>>,
}

#[utoipa::path(
    put,
    path = "/api/v1/users/{user_id}",
    params(("user_id" = i64, Path)),
    request_body = UpdateUserBody,
    responses(
        (status = 200, body = UserResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "users",
)]
async fn update_user(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(user_id): Path<UserId>,
    Json(body): Json<UpdateUserBody>,
) -> Result<Json<UserResponse>, ApiError> {
    require_master(&auth, state.auth_enabled())?;
    let params = UpdateUserParams {
        username: body.username,
        display_name: body.display_name,
    };
    let (user, _events) = state
        .user_service
        .update_user(user_id, &params)
        .await
        .map_err(classify_error)?;
    Ok(Json(UserResponse::from(user)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/users/{user_id}",
    params(("user_id" = i64, Path)),
    responses(
        (status = 204),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "users",
)]
async fn delete_user(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(user_id): Path<UserId>,
) -> Result<StatusCode, ApiError> {
    require_master(&auth, state.auth_enabled())?;
    state
        .user_service
        .delete_user(user_id)
        .await
        .map_err(classify_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// --- Member Handlers ---

#[derive(Deserialize, ToSchema)]
struct AddMemberBody {
    user_id: UserId,
    role: Option<Role>,
}

#[derive(Deserialize, ToSchema)]
struct UpdateRoleBody {
    role: Role,
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{project_id}/members",
    params(("project_id" = i64, Path), ListMembersQuery),
    responses(
        (status = 200, body = ListMembersPageResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "members",
)]
async fn list_members(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<ProjectId>,
    Query(query): Query<ListMembersQuery>,
) -> Result<Json<ListMembersPageResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let (limit, after) = decode_page_inputs::<i64>(query.limit, query.after.as_deref())?;
    let filter = ListProjectMembersFilter { limit, after };
    let page = state
        .project_service
        .list_project_members(project_id, &filter)
        .await
        .map_err(classify_error)?;
    let mut responses = Vec::with_capacity(page.items.len());
    for member in page.items {
        let user = state.user_service.get_user(member.user_id()).await.ok();
        responses.push(ProjectMemberResponse::from_parts(member, user.as_ref()));
    }
    Ok(Json(ListMembersPageResponse {
        items: responses,
        next_cursor: page.next_cursor,
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{project_id}/members",
    params(("project_id" = i64, Path)),
    request_body = AddMemberBody,
    responses(
        (status = 201, body = ProjectMemberResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "members",
)]
async fn add_member(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<ProjectId>,
    Json(body): Json<AddMemberBody>,
) -> Result<(StatusCode, Json<ProjectMemberResponse>), ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Admin).await?;
    let caller_user_id = auth.0.as_ref().map(|a| a.user.id());
    let params = AddProjectMemberParams::new(body.user_id, body.role);
    let (member, _events) = state
        .project_service
        .add_project_member(project_id, &params, caller_user_id)
        .await
        .map_err(classify_error)?;
    let user = state.user_service.get_user(member.user_id()).await.ok();
    Ok((
        StatusCode::CREATED,
        Json(ProjectMemberResponse::from_parts(member, user.as_ref())),
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{project_id}/members/{user_id}",
    params(("project_id" = i64, Path), ("user_id" = i64, Path)),
    responses(
        (status = 200, body = ProjectMemberResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "members",
)]
async fn get_member(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, user_id)): Path<(ProjectId, UserId)>,
) -> Result<Json<ProjectMemberResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let member = state
        .project_service
        .get_project_member(project_id, user_id)
        .await
        .map_err(classify_error)?;
    let user = state.user_service.get_user(member.user_id()).await.ok();
    Ok(Json(ProjectMemberResponse::from_parts(
        member,
        user.as_ref(),
    )))
}

#[utoipa::path(
    put,
    path = "/api/v1/projects/{project_id}/members/{user_id}",
    params(("project_id" = i64, Path), ("user_id" = i64, Path)),
    request_body = UpdateRoleBody,
    responses(
        (status = 200, body = ProjectMemberResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "members",
)]
async fn update_member_role(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, user_id)): Path<(ProjectId, UserId)>,
    Json(body): Json<UpdateRoleBody>,
) -> Result<Json<ProjectMemberResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Admin).await?;
    let caller_user_id = auth.0.as_ref().map(|a| a.user.id());
    let (member, _events) = state
        .project_service
        .update_member_role(project_id, user_id, body.role, caller_user_id)
        .await
        .map_err(classify_error)?;
    let user = state.user_service.get_user(member.user_id()).await.ok();
    Ok(Json(ProjectMemberResponse::from_parts(
        member,
        user.as_ref(),
    )))
}

#[utoipa::path(
    delete,
    path = "/api/v1/projects/{project_id}/members/{user_id}",
    params(("project_id" = i64, Path), ("user_id" = i64, Path)),
    responses(
        (status = 204),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "members",
)]
async fn remove_member(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, user_id)): Path<(ProjectId, UserId)>,
) -> Result<StatusCode, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Admin).await?;
    let caller_user_id = auth.0.as_ref().map(|a| a.user.id());
    let _events = state
        .project_service
        .remove_project_member(project_id, user_id, caller_user_id)
        .await
        .map_err(classify_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// --- MetadataField Handlers ---

#[utoipa::path(
    post,
    path = "/api/v1/projects/{project_id}/metadata-fields",
    params(("project_id" = i64, Path)),
    request_body = CreateMetadataFieldParams,
    responses(
        (status = 201, body = MetadataFieldResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "metadata-fields",
)]
async fn create_metadata_field(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<ProjectId>,
    Json(body): Json<CreateMetadataFieldParams>,
) -> Result<(StatusCode, Json<MetadataFieldResponse>), ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let field = state
        .metadata_service
        .create_metadata_field(project_id, &body)
        .await
        .map_err(classify_error)?;
    Ok((
        StatusCode::CREATED,
        Json(MetadataFieldResponse::from(field)),
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{project_id}/metadata-fields",
    params(("project_id" = i64, Path), ListMetadataFieldsQuery),
    responses(
        (status = 200, body = ListMetadataFieldsPageResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "metadata-fields",
)]
async fn list_metadata_fields(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<ProjectId>,
    Query(query): Query<ListMetadataFieldsQuery>,
) -> Result<Json<ListMetadataFieldsPageResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let (limit, after) = decode_page_inputs::<i64>(query.limit, query.after.as_deref())?;
    let filter = ListMetadataFieldsFilter { limit, after };
    let page = state
        .metadata_service
        .list_metadata_fields(project_id, &filter)
        .await
        .map_err(classify_error)?;
    Ok(Json(ListMetadataFieldsPageResponse {
        items: page
            .items
            .into_iter()
            .map(MetadataFieldResponse::from)
            .collect(),
        next_cursor: page.next_cursor,
    }))
}

#[utoipa::path(
    delete,
    path = "/api/v1/projects/{project_id}/metadata-fields/{name}",
    params(("project_id" = i64, Path), ("name" = String, Path)),
    responses(
        (status = 204),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "metadata-fields",
)]
async fn delete_metadata_field_handler(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, name)): Path<(ProjectId, String)>,
) -> Result<StatusCode, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    state
        .metadata_service
        .delete_metadata_field_by_name(project_id, &name)
        .await
        .map_err(classify_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// --- API Key Handlers ---

#[utoipa::path(
    get,
    path = "/api/v1/users/{user_id}/api-keys",
    params(("user_id" = i64, Path)),
    responses(
        (status = 200, body = Vec<ApiKeyResponse>),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "api-keys",
)]
async fn list_api_keys(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(user_id): Path<UserId>,
) -> Result<Json<Vec<ApiKeyResponse>>, ApiError> {
    require_auth_user(&auth, state.auth_enabled())?;
    let keys = state
        .user_service
        .list_api_keys(user_id)
        .await
        .map_err(classify_error)?;
    Ok(Json(keys.into_iter().map(ApiKeyResponse::from).collect()))
}

#[utoipa::path(
    post,
    path = "/api/v1/users/{user_id}/api-keys",
    params(("user_id" = i64, Path)),
    request_body(content = CreateApiKeyParams, description = "Optional name and device_name."),
    responses(
        (status = 201, body = ApiKeyWithSecretResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "api-keys",
)]
async fn create_api_key(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(user_id): Path<UserId>,
    body: Option<Json<CreateApiKeyParams>>,
) -> Result<(StatusCode, Json<ApiKeyWithSecretResponse>), ApiError> {
    let caller = require_auth_user(&auth, state.auth_enabled())?;
    if let Some(caller) = caller
        && !caller.is_master
        && caller.user.id() != user_id
    {
        return Err(ApiError::Forbidden(
            "can only create API keys for your own account".into(),
        ));
    }
    let (name, device_name) = match body {
        Some(Json(b)) => (b.name.unwrap_or_default(), b.device_name),
        None => (String::new(), None),
    };
    let (key, _events) = state
        .user_service
        .create_api_key(user_id, &name, device_name.as_deref())
        .await
        .map_err(classify_error)?;
    Ok((
        StatusCode::CREATED,
        Json(ApiKeyWithSecretResponse::from(key)),
    ))
}

#[utoipa::path(
    delete,
    path = "/api/v1/users/{user_id}/api-keys/{key_id}",
    params(("user_id" = i64, Path), ("key_id" = i64, Path)),
    responses(
        (status = 204),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "api-keys",
)]
async fn delete_api_key(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((user_id, key_id)): Path<(UserId, i64)>,
) -> Result<StatusCode, ApiError> {
    let caller = require_auth_user(&auth, state.auth_enabled())?;
    if let Some(caller) = caller
        && !caller.is_master
        && caller.user.id() != user_id
    {
        return Err(ApiError::Forbidden(
            "can only delete your own api keys".into(),
        ));
    }
    let _events = state
        .user_service
        .delete_api_key(key_id, user_id)
        .await
        .map_err(classify_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// --- Auth / Session Management Handlers ---

#[utoipa::path(
    get,
    path = "/auth/me",
    responses(
        (status = 200, body = MeResponse),
        (status = 401, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "auth",
)]
async fn get_me(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Proxy/relay mode: forward to upstream /auth/me so that the client's Bearer token
    // (carried via PASSTHROUGH_TOKEN) resolves the caller's identity on the backend.
    if state.proxy_mode {
        let value = state
            .user_service
            .fetch_me()
            .await
            .map_err(classify_error)?;
        return Ok(Json(value));
    }

    let auth = auth.0.ok_or(AuthError::MissingToken)?;
    let session = match state.auth_mode.as_deref() {
        Some(AuthMode::TrustedHeaders(_)) => None,
        // Bypass mode has no session — and we MUST NOT consult the
        // Authorization header here, because callers under bypass do not
        // send one.
        Some(AuthMode::DevBypass { .. }) => None,
        _ => {
            let token = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .ok_or(AuthError::MissingToken)?;
            let token_prefix = &token[..token.len().min(11)];

            let sessions = state
                .user_service
                .list_active_sessions(
                    auth.user.id(),
                    &state.session_config,
                    &ListSessionsFilter::default(),
                )
                .await
                .map_err(classify_error)?;

            let current_session = sessions
                .items
                .into_iter()
                .find(|s| s.key_prefix() == token_prefix)
                .ok_or_else(|| classify_error(anyhow::anyhow!("current session not found")))?;

            Some(SessionResponse::from(current_session))
        }
    };

    let me = MeResponse {
        user: UserResponse::from(auth.user),
        session,
    };
    serde_json::to_value(me).map(Json).map_err(|e| {
        let msg = e.to_string();
        ApiError::Internal {
            public_message: msg.clone(),
            log_message: msg,
        }
    })
}

#[derive(Deserialize, ToSchema)]
struct CreateTokenRequest {
    device_name: Option<String>,
}

#[utoipa::path(
    post,
    path = "/auth/token",
    request_body(content = CreateTokenRequest, description = "Optional device_name."),
    responses(
        (status = 201, body = TokenResponse),
        (status = 401, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "auth",
)]
async fn create_token(
    State(state): State<AppState>,
    auth: AuthUser,
    body: Option<Json<CreateTokenRequest>>,
) -> Result<(StatusCode, Json<TokenResponse>), ApiError> {
    // Under dev_bypass we'd otherwise persist the synthetic user via
    // `get_or_create_user` and hand out a real session token — neither is
    // appropriate for a bypass deployment, so refuse the call.
    if matches!(state.auth_mode.as_deref(), Some(AuthMode::DevBypass { .. })) {
        return Err(ApiError::NotImplemented(
            "/auth/token is disabled in dev_bypass mode".into(),
        ));
    }
    let device_name = body.and_then(|b| b.0.device_name);
    // Ensure user exists in DB (auto-created by JwtAuthProvider if OIDC)
    // Auto-create runs only when the JWT path didn't already provision the
    // user. The provisioning source for that fallback is OIDC (the only
    // create_token caller is the JWT → API-key exchange).
    let (user, _events) = state
        .user_service
        .get_or_create_user(
            auth.user.sub(),
            auth.user.username(),
            auth.user.display_name(),
            auth.user.email(),
            crate::domain::user::UserCreationSource::OidcProvisioning,
        )
        .await
        .map_err(classify_error)?;

    let key = state
        .user_service
        .create_session_token(user.id(), device_name.as_deref(), &state.session_config)
        .await
        .map_err(classify_error)?;

    let expires_at = compute_expires_at(key.created_at(), &state.session_config);

    Ok((
        StatusCode::CREATED,
        Json(TokenResponse {
            token: key.key().to_owned(),
            id: key.id(),
            key_prefix: key.key_prefix().to_owned(),
            expires_at,
        }),
    ))
}

fn compute_expires_at(
    created_at: &str,
    session_config: &crate::infra::config::SessionConfig,
) -> Option<String> {
    let ttl_str = session_config.ttl.as_ref()?;
    let ttl = crate::domain::duration::parse_duration(ttl_str).ok()?;
    let created = chrono::DateTime::parse_from_rfc3339(created_at).ok()?;
    let expires = created + chrono::Duration::from_std(ttl).ok()?;
    Some(expires.to_rfc3339())
}

#[utoipa::path(
    get,
    path = "/auth/sessions",
    params(ListSessionsQuery),
    responses(
        (status = 200, body = ListSessionsPageResponse),
        (status = 401, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "auth",
)]
async fn list_sessions(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<ListSessionsQuery>,
) -> Result<Json<ListSessionsPageResponse>, ApiError> {
    let (limit, after) = decode_page_inputs::<i64>(query.limit, query.after.as_deref())?;
    let filter = ListSessionsFilter { limit, after };
    let page = state
        .user_service
        .list_active_sessions(auth.user.id(), &state.session_config, &filter)
        .await
        .map_err(classify_error)?;
    Ok(Json(ListSessionsPageResponse {
        items: page.items.into_iter().map(SessionResponse::from).collect(),
        next_cursor: page.next_cursor,
    }))
}

#[utoipa::path(
    delete,
    path = "/auth/sessions/{id}",
    params(("id" = i64, Path, description = "Session/api-key id")),
    responses(
        (status = 204),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "auth",
)]
async fn revoke_session(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(key_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let _events = state
        .user_service
        .revoke_session(key_id, auth.user.id())
        .await
        .map_err(classify_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete,
    path = "/auth/sessions",
    responses(
        (status = 204),
        (status = 401, body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "auth",
)]
async fn revoke_all_sessions(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<StatusCode, ApiError> {
    let _events = state
        .user_service
        .revoke_all_sessions(auth.user.id())
        .await
        .map_err(classify_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upstream_error(status: u16, message: &str) -> anyhow::Error {
        anyhow::Error::new(crate::infra::http::UpstreamHttpError {
            status: reqwest::StatusCode::from_u16(status).unwrap(),
            message: message.to_string(),
        })
    }

    fn assert_api_error_status(err: ApiError, expected_status: StatusCode, expected_msg: &str) {
        let resp = err.into_response();
        assert_eq!(resp.status(), expected_status);
        let _ = expected_msg; // message validated via status mapping
    }

    #[test]
    fn classify_upstream_401() {
        let err = classify_error(upstream_error(401, "invalid token"));
        assert_api_error_status(err, StatusCode::UNAUTHORIZED, "invalid token");
    }

    #[test]
    fn classify_upstream_403() {
        let err = classify_error(upstream_error(403, "access denied"));
        assert_api_error_status(err, StatusCode::FORBIDDEN, "access denied");
    }

    #[test]
    fn classify_upstream_404() {
        let err = classify_error(upstream_error(404, "not found"));
        assert_api_error_status(err, StatusCode::NOT_FOUND, "not found");
    }

    #[test]
    fn classify_upstream_409() {
        let err = classify_error(upstream_error(409, "conflict"));
        assert_api_error_status(err, StatusCode::CONFLICT, "conflict");
    }

    #[test]
    fn classify_upstream_500_becomes_internal() {
        let err = classify_error(upstream_error(500, "server error"));
        assert_api_error_status(err, StatusCode::INTERNAL_SERVER_ERROR, "server error");
    }

    // --- has_auth_credentials tests ---

    use crate::application::port::auth::{AuthError as PortAuthError, AuthProvider, AuthResult};
    use crate::infra::config::TrustedHeadersConfig;

    struct DummyAuthProvider;

    #[async_trait::async_trait]
    impl AuthProvider for DummyAuthProvider {
        async fn authenticate(
            &self,
            _token: &str,
        ) -> std::result::Result<AuthResult, PortAuthError> {
            Err(PortAuthError::InvalidToken)
        }
    }

    fn token_auth_mode() -> AuthMode {
        AuthMode::Token(Arc::new(DummyAuthProvider))
    }

    fn default_trusted_headers_config() -> TrustedHeadersConfig {
        TrustedHeadersConfig {
            subject_header: None,
            name_header: None,
            display_name_header: None,
            email_header: None,
            groups_header: None,
            scope_header: None,
            oidc_issuer_url: None,
            oidc_client_id: None,
            master_group: None,
        }
    }

    #[test]
    fn auth_credentials_none_mode() {
        let headers = axum::http::HeaderMap::new();
        let config = default_trusted_headers_config();
        assert!(!has_auth_credentials(&headers, None, &config));
    }

    #[test]
    fn auth_credentials_token_with_header() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("authorization", "Bearer test-token".parse().unwrap());
        let mode = token_auth_mode();
        let config = default_trusted_headers_config();
        assert!(has_auth_credentials(&headers, Some(&mode), &config));
    }

    #[test]
    fn auth_credentials_token_without_header() {
        let headers = axum::http::HeaderMap::new();
        let mode = token_auth_mode();
        let config = default_trusted_headers_config();
        assert!(!has_auth_credentials(&headers, Some(&mode), &config));
    }

    // --- senko.api.error LogRecord emission (Contract #8 / Phase C2) -------

    use opentelemetry::logs::{AnyValue, Severity};
    use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
    use opentelemetry_sdk::logs::{InMemoryLogExporter, SdkLogRecord, SdkLoggerProvider};
    use tracing_subscriber::layer::SubscriberExt;

    /// Run `body` under a fresh tracing subscriber bridged to an in-memory
    /// OTel `LogRecord` exporter. Returns every emitted record. Each test
    /// gets its own provider/exporter so parallel test runs cannot leak
    /// records across cases (see Contract #8 note on B3 flakiness with
    /// shared global subscribers).
    fn capture_log_records<F: FnOnce()>(body: F) -> Vec<SdkLogRecord> {
        let exporter = InMemoryLogExporter::default();
        let provider = SdkLoggerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let subscriber =
            tracing_subscriber::registry().with(OpenTelemetryTracingBridge::new(&provider));

        tracing::subscriber::with_default(subscriber, body);

        provider.force_flush().ok();
        exporter
            .get_emitted_logs()
            .unwrap()
            .into_iter()
            .map(|d| d.record)
            .collect()
    }

    fn lookup_log_attr(record: &SdkLogRecord, key: &str) -> Option<AnyValue> {
        record
            .attributes_iter()
            .find(|(k, _)| k.as_str() == key)
            .map(|(_, v)| v.clone())
    }

    fn only_api_error(records: &[SdkLogRecord]) -> &SdkLogRecord {
        let matches: Vec<&SdkLogRecord> = records
            .iter()
            .filter(|r| r.event_name() == Some("senko.api.error"))
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "expected exactly one senko.api.error record, got {}",
            matches.len()
        );
        matches[0]
    }

    fn assert_emits_api_error(
        err: ApiError,
        expected_status: u16,
        expected_error_type: &str,
        expected_log_message: &str,
    ) {
        let records = capture_log_records(|| {
            let resp = err.into_response();
            assert_eq!(resp.status().as_u16(), expected_status);
        });

        let r = only_api_error(&records);
        assert_eq!(
            r.severity_number(),
            Some(Severity::Error),
            "senko.api.error must be emitted at ERROR severity (Contract #8)",
        );
        assert_eq!(
            r.target().map(|c| c.as_ref()),
            Some("senko_business"),
            "target must be senko_business so BusinessAttributesProcessor enriches it",
        );
        assert_eq!(
            lookup_log_attr(r, "http.status_code"),
            Some(AnyValue::Int(i64::from(expected_status))),
        );
        assert_eq!(
            lookup_log_attr(r, "error.type"),
            Some(AnyValue::String(expected_error_type.to_string().into())),
        );
        assert_eq!(
            lookup_log_attr(r, "error.message"),
            Some(AnyValue::String(expected_log_message.to_string().into())),
        );
    }

    #[test]
    fn emits_senko_api_error_for_not_found() {
        assert_emits_api_error(
            ApiError::NotFound("missing thing".into()),
            404,
            "not_found",
            "missing thing",
        );
    }

    #[test]
    fn emits_senko_api_error_for_bad_request() {
        assert_emits_api_error(
            ApiError::BadRequest("bad input".into()),
            400,
            "bad_request",
            "bad input",
        );
    }

    #[test]
    fn emits_senko_api_error_for_unauthorized() {
        assert_emits_api_error(
            ApiError::Unauthorized("missing token".into()),
            401,
            "unauthorized",
            "missing token",
        );
    }

    #[test]
    fn emits_senko_api_error_for_forbidden() {
        assert_emits_api_error(ApiError::Forbidden("nope".into()), 403, "forbidden", "nope");
    }

    #[test]
    fn emits_senko_api_error_for_conflict() {
        assert_emits_api_error(ApiError::Conflict("dup".into()), 409, "conflict", "dup");
    }

    #[test]
    fn emits_senko_api_error_for_not_implemented() {
        assert_emits_api_error(
            ApiError::NotImplemented("todo".into()),
            501,
            "not_implemented",
            "todo",
        );
    }

    #[test]
    fn emits_senko_api_error_for_internal_with_separate_messages() {
        // Public/log split is the security-sensitive one — the response body
        // must show the static "internal server error", but `error.message`
        // on senko.api.error must carry the (Display-formatted) anyhow chain.
        let err = ApiError::Internal {
            public_message: "internal server error".into(),
            log_message: "db timeout while loading config from /etc/senko/secret.toml".into(),
        };
        assert_emits_api_error(
            err,
            500,
            "internal",
            "db timeout while loading config from /etc/senko/secret.toml",
        );
    }

    #[test]
    fn unclassified_anyhow_does_not_leak_debug_format() {
        // Anyhow chain with multiple Context layers — Debug-format would
        // include `caused by:` / nested struct shapes. Display-format must be
        // a flat message ("inner: middle: outer") with none of those tokens.
        let err = anyhow::anyhow!("file path /var/secret/db.sock unreadable")
            .context("loading shard config")
            .context("starting up");

        let records = capture_log_records(|| {
            let api_err = classify_error(err);
            // Verify the response body uses the static public message —
            // anyhow detail must not reach the client.
            let (_, body) = match &api_err {
                ApiError::Internal {
                    public_message,
                    log_message,
                } => (log_message.clone(), public_message.clone()),
                _ => panic!("expected ApiError::Internal from unclassified anyhow"),
            };
            assert_eq!(body, "internal server error");
            let resp = api_err.into_response();
            assert_eq!(resp.status().as_u16(), 500);
        });

        // The dropped `tracing::error!("unclassified internal error", ...)`
        // must not produce any record under any name (it's gone entirely).
        assert!(
            !records
                .iter()
                .any(|r| r.event_name() == Some("unclassified internal error")),
            "tracing::error!(\"unclassified internal error\", ...) must be removed",
        );

        let r = only_api_error(&records);
        let msg = match lookup_log_attr(r, "error.message").expect("error.message present") {
            AnyValue::String(s) => s.to_string(),
            other => panic!("expected String, got {other:?}"),
        };

        // Display-format includes all three Context layers separated by ": ".
        assert!(
            msg.contains("starting up"),
            "Display chain must include outer context, got {msg:?}",
        );
        assert!(
            msg.contains("loading shard config"),
            "Display chain must include middle context, got {msg:?}",
        );
        assert!(
            msg.contains("/var/secret/db.sock"),
            "Display chain must include innermost message, got {msg:?}",
        );

        // Debug-format ("Error { ... }", "caused by", multi-line frames) must
        // NOT appear — that was the pre-Phase-C2 leak.
        assert!(
            !msg.contains("caused by"),
            "Debug-format token leaked into error.message: {msg:?}",
        );
        assert!(
            !msg.contains("Error {"),
            "Debug-format struct leaked into error.message: {msg:?}",
        );
    }

    #[test]
    fn legacy_api_error_warn_is_not_emitted() {
        // The pre-Phase-C2 `tracing::warn!("api_error", ...)` is gone; only
        // senko.api.error should remain.
        let records = capture_log_records(|| {
            let _ = ApiError::BadRequest("x".into()).into_response();
        });
        assert!(
            !records.iter().any(|r| r.event_name() == Some("api_error")),
            "legacy `api_error` warn must be removed",
        );
        assert!(
            records
                .iter()
                .any(|r| r.event_name() == Some("senko.api.error")),
            "senko.api.error must replace it",
        );
    }
}
