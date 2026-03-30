use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use axum_extra::extract::Query;
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;

mod auth;

use crate::domain::error::DomainError;
use crate::application::{ProjectService, TaskOperations, TaskService, UserService};
use crate::application::auth as app_auth;
use crate::application::auth::Permission;
use crate::application::port::auth::{AuthError, AuthProvider};
use crate::application::port::TaskBackend;
use self::auth::{HasAuth, OptionalAuthUser};
use crate::bootstrap;
use crate::infra::config::Config;
use crate::domain::project::CreateProjectParams;
use crate::domain::task::{
    CompletionPolicy, CreateTaskParams, ListTasksFilter, Priority, Task, TaskStatus,
    UpdateTaskArrayParams, UpdateTaskParams,
};
use crate::domain::user::{
    AddProjectMemberParams, CreateApiKeyParams, CreateUserParams, Role,
};
use super::dto::{
    ApiKeyResponse, ApiKeyWithSecretResponse, ConfigResponse, PreviewTransitionResponse,
    ProjectMemberResponse, ProjectResponse, TaskResponse, UserResponse,
};

#[derive(Clone)]
struct AppState {
    project_root: Arc<PathBuf>,
    config_path: Option<Arc<PathBuf>>,
    backend: Arc<dyn TaskBackend>,
    task_service: Arc<TaskService>,
    project_service: Arc<ProjectService>,
    user_service: Arc<UserService>,
    auth_provider: Option<Arc<dyn AuthProvider>>,
}

impl HasAuth for AppState {
    fn auth_provider(&self) -> Option<&dyn AuthProvider> {
        self.auth_provider.as_deref()
    }
}

impl AppState {
    fn auth_enabled(&self) -> bool {
        self.auth_provider.is_some()
    }
}

/// Check project-level authorization. No-op when auth is disabled.
async fn check_project_permission(
    state: &AppState,
    auth: &OptionalAuthUser,
    project_id: i64,
    permission: Permission,
) -> Result<(), ApiError> {
    if let Some(user) = require_auth_user(auth, state.auth_enabled())? {
        app_auth::require_project_role(state.backend.as_ref(), user.id(), project_id, permission)
            .await
            .map_err(ApiError::from)?;
    }
    Ok(())
}

/// For endpoints that require authentication: returns the user or 401.
fn require_auth_user(auth: &OptionalAuthUser, auth_enabled: bool) -> Result<Option<&crate::domain::user::User>, ApiError> {
    if !auth_enabled {
        return Ok(None);
    }
    match &auth.0 {
        Some(a) => Ok(Some(&a.user)),
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
    Internal(String),
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            ApiError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            ApiError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            ApiError::NotImplemented(msg) => (StatusCode::NOT_IMPLEMENTED, msg.clone()),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
        };
        let error_type = match self {
            ApiError::NotFound(_) => "not_found",
            ApiError::BadRequest(_) => "bad_request",
            ApiError::Unauthorized(_) => "unauthorized",
            ApiError::Forbidden(_) => "forbidden",
            ApiError::Conflict(_) => "conflict",
            ApiError::NotImplemented(_) => "not_implemented",
            ApiError::Internal(_) => "internal",
        };
        tracing::warn!(
            status = status.as_u16(),
            error_type,
            error = %message,
            "api_error"
        );
        (status, Json(ErrorBody { error: message })).into_response()
    }
}

impl From<AuthError> for ApiError {
    fn from(e: AuthError) -> Self {
        match e {
            AuthError::MissingToken => ApiError::Unauthorized("missing authorization header".into()),
            AuthError::InvalidToken => ApiError::Unauthorized("invalid api key".into()),
            AuthError::Forbidden(msg) => ApiError::Forbidden(msg),
        }
    }
}

fn classify_error(e: anyhow::Error) -> ApiError {
    if e.downcast_ref::<crate::application::port::auth::AuthError>().is_some() {
        return ApiError::Forbidden(e.to_string());
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
            | DomainError::NoEligibleTask => ApiError::NotFound(msg),

            DomainError::InvalidTaskStatus { .. }
            | DomainError::InvalidPriority { .. }
            | DomainError::InvalidRole { .. }
            | DomainError::SelfDependency
            | DomainError::DependencyCycle { .. }
            | DomainError::DodIndexOutOfRange { .. } => ApiError::BadRequest(msg),

            DomainError::InvalidStatusTransition { .. }
            | DomainError::CannotCompleteTask { .. }
            | DomainError::CannotDeleteDefaultProject
            | DomainError::CannotDeleteProjectWithTasks { .. } => ApiError::Conflict(msg),

            DomainError::UnsupportedOperation { .. } => ApiError::NotImplemented(msg),
        };
    }
    tracing::error!(error = ?e, "unclassified internal error");
    ApiError::Internal("internal server error".into())
}

// --- Request types ---

#[derive(Deserialize)]
struct ListTasksQuery {
    #[serde(default)]
    status: Vec<String>,
    #[serde(default)]
    tag: Vec<String>,
    #[serde(default)]
    depends_on: Option<i64>,
    #[serde(default)]
    ready: Option<bool>,
}

#[derive(Deserialize)]
struct StartBody {
    session_id: Option<String>,
    user_id: Option<i64>,
}

#[derive(Deserialize)]
struct CompleteBody {
    #[serde(default)]
    skip_pr_check: bool,
}

#[derive(Deserialize)]
struct CancelBody {
    reason: Option<String>,
}

#[derive(Deserialize)]
struct NextBody {
    session_id: Option<String>,
    user_id: Option<i64>,
}

#[derive(Deserialize)]
struct AddDepBody {
    dep_id: i64,
}

#[derive(Deserialize)]
struct PreviewTransitionQuery {
    target: String,
}

#[derive(Deserialize, Default)]
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
    metadata: Option<serde_json::Value>,
    #[serde(default)]
    clear_metadata: bool,
    assignee_user_id: Option<i64>,
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

pub async fn serve(
    project_root: PathBuf,
    port: u16,
    port_is_explicit: bool,
    config: &Config,
    config_path: Option<PathBuf>,
    backend: Arc<dyn TaskBackend>,
    auth_provider: Option<Arc<dyn AuthProvider>>,
) -> Result<()> {
    bootstrap::init_tracing(&config.log);

    // Server always fires hooks (should_fire = true)
    let backend_info = bootstrap::resolve_backend_info(config, &project_root);
    let hook_executor = bootstrap::create_api_hook_executor(config.clone(), backend_info, backend.clone());
    let pr_verifier = bootstrap::create_pr_verifier();
    let completion_policy = CompletionPolicy::new(config.workflow.completion_mode, config.workflow.auto_merge);
    let task_service = Arc::new(TaskService::new(
        backend.clone(),
        hook_executor,
        pr_verifier,
        completion_policy,
    ));
    let project_service = Arc::new(ProjectService::new(backend.clone()));
    let user_service = Arc::new(UserService::new(backend.clone()));

    let state = AppState {
        project_root: Arc::new(project_root),
        config_path: config_path.map(Arc::new),
        backend,
        task_service,
        project_service,
        user_service,
        auth_provider,
    };

    let app = Router::new()
        // User CRUD
        .route("/api/v1/users", get(list_users).post(create_user))
        .route(
            "/api/v1/users/{user_id}",
            get(get_user).delete(delete_user),
        )
        // API key management
        .route(
            "/api/v1/users/{user_id}/api-keys",
            get(list_api_keys).post(create_api_key),
        )
        .route(
            "/api/v1/users/{user_id}/api-keys/{key_id}",
            delete(delete_api_key),
        )
        // Project CRUD
        .route("/api/v1/projects", get(list_projects).post(create_project))
        .route(
            "/api/v1/projects/{project_id}",
            get(get_project).delete(delete_project),
        )
        // Project members
        .route(
            "/api/v1/projects/{project_id}/members",
            get(list_members).post(add_member),
        )
        .route(
            "/api/v1/projects/{project_id}/members/{user_id}",
            get(get_member).put(update_member_role).delete(remove_member),
        )
        // Task next + preview (static paths before wildcard)
        .route(
            "/api/v1/projects/{project_id}/tasks/next",
            post(next_task),
        )
        .route(
            "/api/v1/projects/{project_id}/tasks/preview-next",
            get(preview_next),
        )
        // Task CRUD
        .route(
            "/api/v1/projects/{project_id}/tasks",
            get(list_tasks).post(create_task),
        )
        .route(
            "/api/v1/projects/{project_id}/tasks/{id}",
            get(get_task).put(edit_task).delete(delete_task),
        )
        .route(
            "/api/v1/projects/{project_id}/tasks/{id}/_save",
            put(save_task_handler),
        )
        // Preview transition (read-only)
        .route(
            "/api/v1/projects/{project_id}/tasks/{id}/preview-transition",
            get(preview_transition),
        )
        // Status transitions
        .route(
            "/api/v1/projects/{project_id}/tasks/{id}/ready",
            post(ready_task),
        )
        .route(
            "/api/v1/projects/{project_id}/tasks/{id}/start",
            post(start_task),
        )
        .route(
            "/api/v1/projects/{project_id}/tasks/{id}/complete",
            post(complete_task),
        )
        .route(
            "/api/v1/projects/{project_id}/tasks/{id}/cancel",
            post(cancel_task),
        )
        // Dependencies
        .route(
            "/api/v1/projects/{project_id}/tasks/{id}/deps",
            get(list_deps).post(add_dep),
        )
        .route(
            "/api/v1/projects/{project_id}/tasks/{id}/deps/{dep_id}",
            delete(remove_dep),
        )
        // DoD
        .route(
            "/api/v1/projects/{project_id}/tasks/{id}/dod/{index}/check",
            post(check_dod),
        )
        .route(
            "/api/v1/projects/{project_id}/tasks/{id}/dod/{index}/uncheck",
            post(uncheck_dod),
        )
        // Project stats
        .route(
            "/api/v1/projects/{project_id}/stats",
            get(get_stats),
        )
        // Server-wide
        .route("/api/v1/health", get(health_check))
        .route("/api/v1/config", get(get_config))
        .with_state(state)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<_>| {
                    tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        uri = %request.uri(),
                    )
                })
                .on_response(
                    |response: &axum::http::Response<_>,
                     latency: std::time::Duration,
                     _span: &tracing::Span| {
                        tracing::info!(
                            status = response.status().as_u16(),
                            latency_ms = latency.as_millis() as u64,
                            "response"
                        );
                    },
                )
                .on_failure(
                    |error: tower_http::classify::ServerErrorsFailureClass,
                     latency: std::time::Duration,
                     _span: &tracing::Span| {
                        tracing::error!(
                            latency_ms = latency.as_millis() as u64,
                            error = %error,
                            "request failed"
                        );
                    },
                ),
        );

    let bind_addr_str = config.effective_host();
    let bind_ip: std::net::IpAddr = bind_addr_str
        .parse()
        .with_context(|| format!("invalid bind address: {bind_addr_str}"))?;

    let (listener, actual_port) = super::bind_with_retry(bind_ip, port, port_is_explicit).await?;

    if bind_ip.is_unspecified() {
        let device_ip = get_local_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "0.0.0.0".to_string());
        tracing::info!(port = actual_port, "Listening on http://localhost:{actual_port}");
        tracing::info!(port = actual_port, addr = %device_ip, "Listening on http://{device_ip}:{actual_port}");
    } else {
        tracing::info!(port = actual_port, addr = %bind_ip, "Listening on http://{bind_ip}:{actual_port}");
    }

    axum::serve(listener, app).await?;
    Ok(())
}

fn get_local_ip() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|a| a.ip())
}

// --- Project Handlers ---

// GET /api/v1/projects
async fn list_projects(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
) -> Result<Json<Vec<ProjectResponse>>, ApiError> {
    require_auth_user(&auth, state.auth_enabled())?;
    let projects = state.project_service.list_projects().await.map_err(classify_error)?;
    Ok(Json(projects.into_iter().map(ProjectResponse::from).collect()))
}

// POST /api/v1/projects
async fn create_project(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Json(params): Json<CreateProjectParams>,
) -> Result<(StatusCode, Json<ProjectResponse>), ApiError> {
    require_auth_user(&auth, state.auth_enabled())?;
    let project = state.project_service.create_project(&params).await.map_err(classify_error)?;
    Ok((StatusCode::CREATED, Json(ProjectResponse::from(project))))
}

// GET /api/v1/projects/{project_id}
async fn get_project(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<i64>,
) -> Result<Json<ProjectResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let project = state.project_service.get_project(project_id).await.map_err(classify_error)?;
    Ok(Json(ProjectResponse::from(project)))
}

// DELETE /api/v1/projects/{project_id}
async fn delete_project(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Admin).await?;
    let caller_user_id = auth.0.as_ref().map(|a| a.user.id());
    state.project_service.delete_project(project_id, caller_user_id).await.map_err(classify_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// --- Task Handlers ---

// GET /api/v1/projects/{project_id}/tasks
async fn list_tasks(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<i64>,
    Query(query): Query<ListTasksQuery>,
) -> Result<Json<Vec<TaskResponse>>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let statuses: Vec<TaskStatus> = query
        .status
        .iter()
        .map(|s| s.parse::<TaskStatus>())
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(classify_error)?;
    let filter = ListTasksFilter {
        statuses,
        tags: query.tag,
        depends_on: query.depends_on,
        ready: query.ready.unwrap_or(false),
    };
    let tasks = state.task_service.list_tasks(project_id, &filter).await.map_err(classify_error)?;
    Ok(Json(tasks.into_iter().map(TaskResponse::from).collect()))
}

// POST /api/v1/projects/{project_id}/tasks
async fn create_task(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<i64>,
    Json(params): Json<CreateTaskParams>,
) -> Result<(StatusCode, Json<TaskResponse>), ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let task = state.task_service.create_task(project_id, &params).await.map_err(classify_error)?;
    Ok((StatusCode::CREATED, Json(TaskResponse::from(task))))
}

// GET /api/v1/projects/{project_id}/tasks/{id}
async fn get_task(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(i64, i64)>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let task = state.task_service.get_task(project_id, id).await.map_err(classify_error)?;
    Ok(Json(TaskResponse::from(task)))
}

// PUT /api/v1/projects/{project_id}/tasks/{id}
async fn edit_task(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(i64, i64)>,
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
            body.assignee_user_id.map(Some)
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
        metadata: if body.clear_metadata {
            Some(None)
        } else {
            body.metadata.map(Some)
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

    state.task_service.edit_task(project_id, id, &scalar_params).await.map_err(classify_error)?;
    state.task_service.edit_task_arrays(project_id, id, &array_params).await.map_err(classify_error)?;
    let task = state.task_service.get_task(project_id, id).await.map_err(classify_error)?;
    Ok(Json(TaskResponse::from(task)))
}

// PUT /api/v1/projects/{project_id}/tasks/{id}/_save
async fn save_task_handler(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(i64, i64)>,
    Json(task): Json<Task>,
) -> Result<StatusCode, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    if task.id() != id || task.project_id() != project_id {
        return Err(classify_error(anyhow::anyhow!("task ID or project ID mismatch")));
    }
    state.backend.save(&task).await.map_err(classify_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// DELETE /api/v1/projects/{project_id}/tasks/{id}
async fn delete_task(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Admin).await?;
    state.task_service.delete_task(project_id, id).await.map_err(classify_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// POST /api/v1/projects/{project_id}/tasks/{id}/ready
async fn ready_task(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(i64, i64)>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let updated = state.task_service.ready_task(project_id, id).await.map_err(classify_error)?;
    Ok(Json(TaskResponse::from(updated)))
}

// POST /api/v1/projects/{project_id}/tasks/{id}/start
async fn start_task(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(i64, i64)>,
    Json(body): Json<StartBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let updated = state.task_service.start_task(project_id, id, body.session_id, body.user_id).await.map_err(classify_error)?;
    Ok(Json(TaskResponse::from(updated)))
}

// POST /api/v1/projects/{project_id}/tasks/{id}/complete
async fn complete_task(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(i64, i64)>,
    body: Option<Json<CompleteBody>>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let skip_pr_check = body.map(|b| b.skip_pr_check).unwrap_or(false);
    let updated = state.task_service.complete_task(project_id, id, skip_pr_check).await.map_err(classify_error)?;
    Ok(Json(TaskResponse::from(updated)))
}

// POST /api/v1/projects/{project_id}/tasks/{id}/cancel
async fn cancel_task(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(i64, i64)>,
    body: Option<Json<CancelBody>>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let reason = body.and_then(|b| b.0.reason);
    let updated = state.task_service.cancel_task(project_id, id, reason).await.map_err(classify_error)?;
    Ok(Json(TaskResponse::from(updated)))
}

// POST /api/v1/projects/{project_id}/tasks/next
async fn next_task(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<i64>,
    body: Option<Json<NextBody>>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let (session_id, user_id) = body
        .map(|b| (b.0.session_id, b.0.user_id))
        .unwrap_or((None, None));
    let updated = state.task_service.next_task(project_id, session_id, user_id).await.map_err(classify_error)?;
    Ok(Json(TaskResponse::from(updated)))
}

// GET /api/v1/projects/{project_id}/tasks/{id}/preview-transition?target=todo
async fn preview_transition(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(i64, i64)>,
    Query(query): Query<PreviewTransitionQuery>,
) -> Result<Json<PreviewTransitionResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let target: TaskStatus = query.target.parse().map_err(classify_error)?;
    let result = state.task_service
        .preview_transition(project_id, id, target)
        .await
        .map_err(classify_error)?;
    Ok(Json(PreviewTransitionResponse::from(result)))
}

// GET /api/v1/projects/{project_id}/tasks/preview-next
async fn preview_next(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<i64>,
) -> Result<Json<PreviewTransitionResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let result = state.task_service
        .preview_next(project_id)
        .await
        .map_err(classify_error)?;
    Ok(Json(PreviewTransitionResponse::from(result)))
}

// GET /api/v1/projects/{project_id}/tasks/{id}/deps
async fn list_deps(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(i64, i64)>,
) -> Result<Json<Vec<TaskResponse>>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let deps = state.task_service.list_dependencies(project_id, id).await.map_err(classify_error)?;
    Ok(Json(deps.into_iter().map(TaskResponse::from).collect()))
}

// POST /api/v1/projects/{project_id}/tasks/{id}/deps
async fn add_dep(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id)): Path<(i64, i64)>,
    Json(body): Json<AddDepBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let task = state.task_service.add_dependency(project_id, id, body.dep_id).await.map_err(classify_error)?;
    Ok(Json(TaskResponse::from(task)))
}

// DELETE /api/v1/projects/{project_id}/tasks/{id}/deps/{dep_id}
async fn remove_dep(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id, dep_id)): Path<(i64, i64, i64)>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let task = state.task_service.remove_dependency(project_id, id, dep_id).await.map_err(classify_error)?;
    Ok(Json(TaskResponse::from(task)))
}

// POST /api/v1/projects/{project_id}/tasks/{id}/dod/{index}/check
async fn check_dod(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id, index)): Path<(i64, i64, usize)>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let task = state.task_service.check_dod(project_id, id, index).await.map_err(classify_error)?;
    Ok(Json(TaskResponse::from(task)))
}

// POST /api/v1/projects/{project_id}/tasks/{id}/dod/{index}/uncheck
async fn uncheck_dod(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, id, index)): Path<(i64, i64, usize)>,
) -> Result<Json<TaskResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Edit).await?;
    let task = state.task_service.uncheck_dod(project_id, id, index).await.map_err(classify_error)?;
    Ok(Json(TaskResponse::from(task)))
}

// GET /api/v1/config
// GET /api/v1/health
async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

async fn get_config(
    State(state): State<AppState>,
) -> Result<Json<ConfigResponse>, ApiError> {
    let config = crate::bootstrap::load_config(&state.project_root, state.config_path.as_deref().map(|p| p.as_path())).map_err(classify_error)?;
    Ok(Json(ConfigResponse::from(config)))
}

// GET /api/v1/projects/{project_id}/stats
async fn get_stats(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<i64>,
) -> Result<Json<HashMap<String, i64>>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let stats = state.task_service.task_stats(project_id).await.map_err(classify_error)?;
    Ok(Json(stats))
}

// --- User Handlers ---

// GET /api/v1/users
async fn list_users(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
) -> Result<Json<Vec<UserResponse>>, ApiError> {
    require_auth_user(&auth, state.auth_enabled())?;
    let users = state.user_service.list_users().await.map_err(classify_error)?;
    Ok(Json(users.into_iter().map(UserResponse::from).collect()))
}

// POST /api/v1/users
async fn create_user(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Json(params): Json<CreateUserParams>,
) -> Result<(StatusCode, Json<UserResponse>), ApiError> {
    require_auth_user(&auth, state.auth_enabled())?;
    let user = state.user_service.create_user(&params).await.map_err(classify_error)?;
    Ok((StatusCode::CREATED, Json(UserResponse::from(user))))
}

// GET /api/v1/users/{user_id}
async fn get_user(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(user_id): Path<i64>,
) -> Result<Json<UserResponse>, ApiError> {
    require_auth_user(&auth, state.auth_enabled())?;
    let user = state.user_service.get_user(user_id).await.map_err(classify_error)?;
    Ok(Json(UserResponse::from(user)))
}

// DELETE /api/v1/users/{user_id}
async fn delete_user(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(user_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    require_auth_user(&auth, state.auth_enabled())?;
    state.user_service.delete_user(user_id).await.map_err(classify_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// --- Member Handlers ---

#[derive(Deserialize)]
struct AddMemberBody {
    user_id: i64,
    role: Option<Role>,
}

#[derive(Deserialize)]
struct UpdateRoleBody {
    role: Role,
}

// GET /api/v1/projects/{project_id}/members
async fn list_members(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<ProjectMemberResponse>>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let members = state.project_service.list_project_members(project_id).await.map_err(classify_error)?;
    Ok(Json(members.into_iter().map(ProjectMemberResponse::from).collect()))
}

// POST /api/v1/projects/{project_id}/members
async fn add_member(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(project_id): Path<i64>,
    Json(body): Json<AddMemberBody>,
) -> Result<(StatusCode, Json<ProjectMemberResponse>), ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Admin).await?;
    let caller_user_id = auth.0.as_ref().map(|a| a.user.id());
    let params = AddProjectMemberParams::new(body.user_id, body.role);
    let member = state.project_service.add_project_member(project_id, &params, caller_user_id).await.map_err(classify_error)?;
    Ok((StatusCode::CREATED, Json(ProjectMemberResponse::from(member))))
}

// GET /api/v1/projects/{project_id}/members/{user_id}
async fn get_member(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, user_id)): Path<(i64, i64)>,
) -> Result<Json<ProjectMemberResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::View).await?;
    let member = state.project_service.get_project_member(project_id, user_id).await.map_err(classify_error)?;
    Ok(Json(ProjectMemberResponse::from(member)))
}

// PUT /api/v1/projects/{project_id}/members/{user_id}
async fn update_member_role(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, user_id)): Path<(i64, i64)>,
    Json(body): Json<UpdateRoleBody>,
) -> Result<Json<ProjectMemberResponse>, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Admin).await?;
    let caller_user_id = auth.0.as_ref().map(|a| a.user.id());
    let member = state.project_service.update_member_role(project_id, user_id, body.role, caller_user_id).await.map_err(classify_error)?;
    Ok(Json(ProjectMemberResponse::from(member)))
}

// DELETE /api/v1/projects/{project_id}/members/{user_id}
async fn remove_member(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((project_id, user_id)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    check_project_permission(&state, &auth, project_id, Permission::Admin).await?;
    let caller_user_id = auth.0.as_ref().map(|a| a.user.id());
    state.project_service.remove_project_member(project_id, user_id, caller_user_id).await.map_err(classify_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// --- API Key Handlers ---

// GET /api/v1/users/{user_id}/api-keys
async fn list_api_keys(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(user_id): Path<i64>,
) -> Result<Json<Vec<ApiKeyResponse>>, ApiError> {
    require_auth_user(&auth, state.auth_enabled())?;
    let keys = state.user_service.list_api_keys(user_id).await.map_err(classify_error)?;
    Ok(Json(keys.into_iter().map(ApiKeyResponse::from).collect()))
}

// POST /api/v1/users/{user_id}/api-keys
async fn create_api_key(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(user_id): Path<i64>,
    body: Option<Json<CreateApiKeyParams>>,
) -> Result<(StatusCode, Json<ApiKeyWithSecretResponse>), ApiError> {
    require_auth_user(&auth, state.auth_enabled())?;
    let name = body.and_then(|b| b.0.name).unwrap_or_default();
    let key = state.user_service.create_api_key(user_id, &name).await.map_err(classify_error)?;
    Ok((StatusCode::CREATED, Json(ApiKeyWithSecretResponse::from(key))))
}

// DELETE /api/v1/users/{user_id}/api-keys/{key_id}
async fn delete_api_key(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path((_user_id, key_id)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    require_auth_user(&auth, state.auth_enabled())?;
    state.user_service.delete_api_key(key_id).await.map_err(classify_error)?;
    Ok(StatusCode::NO_CONTENT)
}
