use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use axum_extra::extract::Query;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::backend::TaskBackend;
use crate::hooks;
use crate::hooks::LogFormat;
use crate::models::{
    CreateProjectParams, CreateTaskParams, ListTasksFilter, Priority, Task, TaskStatus,
    UpdateTaskArrayParams, UpdateTaskParams,
};

#[derive(Clone)]
struct AppState {
    project_root: Arc<PathBuf>,
    config_path: Option<Arc<PathBuf>>,
    backend: Arc<dyn TaskBackend>,
}

// --- Error handling ---

enum ApiError {
    NotFound(String),
    BadRequest(String),
    Conflict(String),
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
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
        };
        let error_type = match self {
            ApiError::NotFound(_) => "not_found",
            ApiError::BadRequest(_) => "bad_request",
            ApiError::Conflict(_) => "conflict",
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

pub fn init_tracing(config: &hooks::LogConfig) {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.level));

    let registry = tracing_subscriber::registry().with(env_filter);

    match config.format {
        LogFormat::Json => {
            registry
                .with(tracing_subscriber::fmt::layer().json())
                .init();
        }
        LogFormat::Pretty => {
            registry.with(tracing_subscriber::fmt::layer()).init();
        }
    }
}

fn classify_error(e: anyhow::Error) -> ApiError {
    let msg = e.to_string();
    if msg.contains("not found") {
        ApiError::NotFound(msg)
    } else if msg.contains("invalid status transition") || msg.contains("cannot complete task") {
        ApiError::Conflict(msg)
    } else if msg.contains("invalid")
        || msg.contains("cannot depend on itself")
        || msg.contains("cycle")
        || msg.contains("out of range")
    {
        ApiError::BadRequest(msg)
    } else {
        ApiError::Internal(msg)
    }
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
}

#[derive(Deserialize)]
struct AddDepBody {
    dep_id: i64,
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
    host: Option<String>,
    config_path: Option<PathBuf>,
    backend: Arc<dyn TaskBackend>,
) -> Result<()> {
    let config = hooks::load_config(&project_root, config_path.as_deref())
        .unwrap_or_default();
    init_tracing(&config.log);

    let state = AppState {
        project_root: Arc::new(project_root),
        config_path: config_path.map(Arc::new),
        backend,
    };

    let app = Router::new()
        // Project CRUD
        .route("/api/v1/projects", get(list_projects).post(create_project))
        .route(
            "/api/v1/projects/{project_id}",
            get(get_project).delete(delete_project),
        )
        // Task next (static path before wildcard)
        .route(
            "/api/v1/projects/{project_id}/tasks/next",
            post(next_task),
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

    let bind_addr_str = host
        .or_else(|| std::env::var("LOCALFLOW_HOST").ok().filter(|v| !v.is_empty()))
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let bind_ip: std::net::IpAddr = bind_addr_str
        .parse()
        .with_context(|| format!("invalid bind address: {bind_addr_str}"))?;
    let addr = std::net::SocketAddr::new(bind_ip, port);
    if bind_ip.is_unspecified() {
        let device_ip = get_local_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "0.0.0.0".to_string());
        tracing::info!(port, "Listening on http://localhost:{port}");
        tracing::info!(port, addr = %device_ip, "Listening on http://{device_ip}:{port}");
    } else {
        tracing::info!(port, addr = %bind_ip, "Listening on http://{bind_ip}:{port}");
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
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
) -> Result<Json<Vec<crate::models::Project>>, ApiError> {
    let projects = state.backend.list_projects().await.map_err(classify_error)?;
    Ok(Json(projects))
}

// POST /api/v1/projects
async fn create_project(
    State(state): State<AppState>,
    Json(params): Json<CreateProjectParams>,
) -> Result<(StatusCode, Json<crate::models::Project>), ApiError> {
    let project = state.backend.create_project(&params).await.map_err(classify_error)?;
    Ok((StatusCode::CREATED, Json(project)))
}

// GET /api/v1/projects/{project_id}
async fn get_project(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
) -> Result<Json<crate::models::Project>, ApiError> {
    let project = state.backend.get_project(project_id).await.map_err(classify_error)?;
    Ok(Json(project))
}

// DELETE /api/v1/projects/{project_id}
async fn delete_project(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    state.backend.delete_project(project_id).await.map_err(classify_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// --- Task Handlers ---

// GET /api/v1/projects/{project_id}/tasks
async fn list_tasks(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
    Query(query): Query<ListTasksQuery>,
) -> Result<Json<Vec<Task>>, ApiError> {
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
    let tasks = state.backend.list_tasks(project_id, &filter).await.map_err(classify_error)?;
    Ok(Json(tasks))
}

// POST /api/v1/projects/{project_id}/tasks
async fn create_task(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
    Json(params): Json<CreateTaskParams>,
) -> Result<(StatusCode, Json<Task>), ApiError> {
    let needs_template = params
        .branch
        .as_ref()
        .is_some_and(|b| b.contains("${task_id}"));

    let task = if needs_template {
        let branch_template = params.branch.clone();
        let mut params_without_branch = params;
        params_without_branch.branch = None;
        let created =
            state.backend.create_task(project_id, &params_without_branch).await.map_err(classify_error)?;
        let expanded =
            branch_template.as_deref().unwrap().replace("${task_id}", &created.id.to_string());
        state.backend.update_task(
            project_id,
            created.id,
            &UpdateTaskParams {
                title: None,
                background: None,
                description: None,
                plan: None,
                priority: None,
                assignee_session_id: None,
                started_at: None,
                completed_at: None,
                canceled_at: None,
                cancel_reason: None,
                branch: Some(Some(expanded)),
                pr_url: None,
                metadata: None,
            },
        )
        .await
        .map_err(classify_error)?
    } else {
        state.backend.create_task(project_id, &params).await.map_err(classify_error)?
    };

    let config = hooks::load_config(&state.project_root, state.config_path.as_deref().map(|p| p.as_path())).map_err(classify_error)?;
    hooks::fire_hooks(&config, "task_added", &task, state.backend.as_ref(), None, None).await;

    Ok((StatusCode::CREATED, Json(task)))
}

// GET /api/v1/projects/{project_id}/tasks/{id}
async fn get_task(
    State(state): State<AppState>,
    Path((project_id, id)): Path<(i64, i64)>,
) -> Result<Json<Task>, ApiError> {
    let task = state.backend.get_task(project_id, id).await.map_err(classify_error)?;
    Ok(Json(task))
}

// PUT /api/v1/projects/{project_id}/tasks/{id}
async fn edit_task(
    State(state): State<AppState>,
    Path((project_id, id)): Path<(i64, i64)>,
    Json(body): Json<EditTaskBody>,
) -> Result<Json<Task>, ApiError> {
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

    state.backend.update_task(project_id, id, &scalar_params).await.map_err(classify_error)?;
    state.backend.update_task_arrays(project_id, id, &array_params).await.map_err(classify_error)?;
    let task = state.backend.get_task(project_id, id).await.map_err(classify_error)?;
    Ok(Json(task))
}

// DELETE /api/v1/projects/{project_id}/tasks/{id}
async fn delete_task(
    State(state): State<AppState>,
    Path((project_id, id)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    state.backend.delete_task(project_id, id).await.map_err(classify_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// POST /api/v1/projects/{project_id}/tasks/{id}/ready
async fn ready_task(
    State(state): State<AppState>,
    Path((project_id, id)): Path<(i64, i64)>,
) -> Result<Json<Task>, ApiError> {
    let updated = state.backend.ready_task(project_id, id).await.map_err(classify_error)?;

    let config = hooks::load_config(&state.project_root, state.config_path.as_deref().map(|p| p.as_path())).map_err(classify_error)?;
    hooks::fire_hooks(
        &config,
        "task_ready",
        &updated,
        state.backend.as_ref(),
        Some(TaskStatus::Draft),
        None,
    ).await;

    Ok(Json(updated))
}

// POST /api/v1/projects/{project_id}/tasks/{id}/start
async fn start_task(
    State(state): State<AppState>,
    Path((project_id, id)): Path<(i64, i64)>,
    Json(body): Json<StartBody>,
) -> Result<Json<Task>, ApiError> {
    let task = state.backend.get_task(project_id, id).await.map_err(classify_error)?;
    let prev_status = task.status;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let updated =
        state.backend.start_task(project_id, id, body.session_id, &now).await.map_err(classify_error)?;

    let config = hooks::load_config(&state.project_root, state.config_path.as_deref().map(|p| p.as_path())).map_err(classify_error)?;
    hooks::fire_hooks(
        &config,
        "task_started",
        &updated,
        state.backend.as_ref(),
        Some(prev_status),
        None,
    ).await;

    Ok(Json(updated))
}

// POST /api/v1/projects/{project_id}/tasks/{id}/complete
async fn complete_task(
    State(state): State<AppState>,
    Path((project_id, id)): Path<(i64, i64)>,
    body: Option<Json<CompleteBody>>,
) -> Result<Json<Task>, ApiError> {
    let skip_pr_check = body.map(|b| b.skip_pr_check).unwrap_or(false);
    let config = hooks::load_config(&state.project_root, state.config_path.as_deref().map(|p| p.as_path())).map_err(classify_error)?;

    let task = state.backend.get_task(project_id, id).await.map_err(classify_error)?;
    task.status
        .transition_to(TaskStatus::Completed)
        .map_err(classify_error)?;

    let unchecked: Vec<_> = task
        .definition_of_done
        .iter()
        .filter(|d| !d.checked)
        .collect();
    if !unchecked.is_empty() {
        return Err(ApiError::Conflict(format!(
            "cannot complete task #{}: {} unchecked DoD item(s)",
            id,
            unchecked.len()
        )));
    }

    if !skip_pr_check
        && config.workflow.completion_mode == hooks::CompletionMode::PrThenComplete
    {
        let pr_url = task.pr_url.as_deref().ok_or_else(|| {
            ApiError::Conflict(format!(
                "cannot complete task #{}: completion_mode is pr_then_complete but no pr_url is set",
                id
            ))
        })?;
        verify_pr_status(pr_url, config.workflow.auto_merge)?;
    }

    let prev_ready_ids: std::collections::HashSet<i64> = state.backend.list_ready_tasks(project_id)
        .await
        .map_err(classify_error)?
        .iter()
        .map(|t| t.id)
        .collect();

    let prev_status = task.status;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let updated = state.backend.complete_task(project_id, id, &now).await.map_err(classify_error)?;

    let unblocked = hooks::compute_unblocked(state.backend.as_ref(), project_id, &prev_ready_ids).await;
    let unblocked_opt = if unblocked.is_empty() {
        None
    } else {
        Some(unblocked)
    };
    hooks::fire_hooks(
        &config,
        "task_completed",
        &updated,
        state.backend.as_ref(),
        Some(prev_status),
        unblocked_opt,
    ).await;

    Ok(Json(updated))
}

// POST /api/v1/projects/{project_id}/tasks/{id}/cancel
async fn cancel_task(
    State(state): State<AppState>,
    Path((project_id, id)): Path<(i64, i64)>,
    body: Option<Json<CancelBody>>,
) -> Result<Json<Task>, ApiError> {
    let reason = body.and_then(|b| b.0.reason);
    let task = state.backend.get_task(project_id, id).await.map_err(classify_error)?;
    task.status
        .transition_to(TaskStatus::Canceled)
        .map_err(classify_error)?;

    let prev_status = task.status;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let updated = state.backend.cancel_task(project_id, id, &now, reason).await.map_err(classify_error)?;

    let config = hooks::load_config(&state.project_root, state.config_path.as_deref().map(|p| p.as_path())).map_err(classify_error)?;
    hooks::fire_hooks(
        &config,
        "task_canceled",
        &updated,
        state.backend.as_ref(),
        Some(prev_status),
        None,
    ).await;

    Ok(Json(updated))
}

// POST /api/v1/projects/{project_id}/tasks/next
async fn next_task(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
    body: Option<Json<NextBody>>,
) -> Result<Json<Task>, ApiError> {
    let session_id = body.and_then(|b| b.0.session_id);
    let task = match state.backend.next_task(project_id).await.map_err(classify_error)? {
        Some(t) => t,
        None => {
            let config = hooks::load_config(&state.project_root, state.config_path.as_deref().map(|p| p.as_path())).map_err(classify_error)?;
            hooks::fire_no_eligible_task_hooks(&config, state.backend.as_ref(), project_id).await;
            return Err(ApiError::NotFound("no eligible task found".to_string()));
        }
    };

    let prev_status = task.status;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let updated =
        state.backend.start_task(project_id, task.id, session_id, &now).await.map_err(classify_error)?;

    let config = hooks::load_config(&state.project_root, state.config_path.as_deref().map(|p| p.as_path())).map_err(classify_error)?;
    hooks::fire_hooks(
        &config,
        "task_started",
        &updated,
        state.backend.as_ref(),
        Some(prev_status),
        None,
    ).await;

    Ok(Json(updated))
}

// GET /api/v1/projects/{project_id}/tasks/{id}/deps
async fn list_deps(
    State(state): State<AppState>,
    Path((project_id, id)): Path<(i64, i64)>,
) -> Result<Json<Vec<Task>>, ApiError> {
    let deps = state.backend.list_dependencies(project_id, id).await.map_err(classify_error)?;
    Ok(Json(deps))
}

// POST /api/v1/projects/{project_id}/tasks/{id}/deps
async fn add_dep(
    State(state): State<AppState>,
    Path((project_id, id)): Path<(i64, i64)>,
    Json(body): Json<AddDepBody>,
) -> Result<Json<Task>, ApiError> {
    let task = state.backend.add_dependency(project_id, id, body.dep_id).await.map_err(classify_error)?;
    Ok(Json(task))
}

// DELETE /api/v1/projects/{project_id}/tasks/{id}/deps/{dep_id}
async fn remove_dep(
    State(state): State<AppState>,
    Path((project_id, id, dep_id)): Path<(i64, i64, i64)>,
) -> Result<Json<Task>, ApiError> {
    let task = state.backend.remove_dependency(project_id, id, dep_id).await.map_err(classify_error)?;
    Ok(Json(task))
}

// POST /api/v1/projects/{project_id}/tasks/{id}/dod/{index}/check
async fn check_dod(
    State(state): State<AppState>,
    Path((project_id, id, index)): Path<(i64, i64, usize)>,
) -> Result<Json<Task>, ApiError> {
    let task = state.backend.check_dod(project_id, id, index).await.map_err(classify_error)?;
    Ok(Json(task))
}

// POST /api/v1/projects/{project_id}/tasks/{id}/dod/{index}/uncheck
async fn uncheck_dod(
    State(state): State<AppState>,
    Path((project_id, id, index)): Path<(i64, i64, usize)>,
) -> Result<Json<Task>, ApiError> {
    let task = state.backend.uncheck_dod(project_id, id, index).await.map_err(classify_error)?;
    Ok(Json(task))
}

// GET /api/v1/config
async fn get_config(
    State(state): State<AppState>,
) -> Result<Json<hooks::Config>, ApiError> {
    let config = hooks::load_config(&state.project_root, state.config_path.as_deref().map(|p| p.as_path())).map_err(classify_error)?;
    Ok(Json(config))
}

// GET /api/v1/projects/{project_id}/stats
async fn get_stats(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
) -> Result<Json<HashMap<String, i64>>, ApiError> {
    let stats = state.backend.task_stats(project_id).await.map_err(classify_error)?;
    Ok(Json(stats))
}

// --- Helpers ---

fn verify_pr_status(pr_url: &str, auto_merge: bool) -> Result<(), ApiError> {
    let mut args = vec!["pr", "view", pr_url, "--json", "state"];
    if !auto_merge {
        args[4] = "state,reviewDecision";
    }

    let output = std::process::Command::new("gh")
        .args(&args)
        .output()
        .map_err(|e| {
            ApiError::Internal(format!(
                "failed to run 'gh' CLI: {e}. gh is required for pr_then_complete mode."
            ))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ApiError::Internal(format!(
            "gh pr view failed: {}",
            stderr.trim()
        )));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| ApiError::Internal(format!("failed to parse gh output: {e}")))?;

    let state = json["state"].as_str().unwrap_or("");
    if state != "MERGED" {
        return Err(ApiError::Conflict(format!(
            "cannot complete task: PR is not merged (current state: {state})"
        )));
    }

    if !auto_merge {
        let decision = json["reviewDecision"].as_str().unwrap_or("");
        if decision != "APPROVED" {
            return Err(ApiError::Conflict(format!(
                "cannot complete task: PR has not been approved (reviewDecision: {})",
                if decision.is_empty() {
                    "none"
                } else {
                    decision
                }
            )));
        }
    }

    Ok(())
}
