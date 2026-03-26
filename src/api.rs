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

use crate::db::{self, TaskBackend};
use crate::hooks;
use crate::models::{
    CreateTaskParams, ListTasksFilter, Priority, Task, TaskStatus, UpdateTaskArrayParams,
    UpdateTaskParams,
};

#[derive(Clone)]
struct AppState {
    project_root: Arc<PathBuf>,
    config_path: Option<Arc<PathBuf>>,
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
        let (status, message) = match self {
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, msg),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };
        (status, Json(ErrorBody { error: message })).into_response()
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

fn join_error(e: tokio::task::JoinError) -> ApiError {
    ApiError::Internal(format!("task join error: {e}"))
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
) -> Result<()> {
    let state = AppState {
        project_root: Arc::new(project_root),
        config_path: config_path.map(Arc::new),
    };

    let app = Router::new()
        // Static path before wildcard to avoid capture
        .route("/api/v1/tasks/next", post(next_task))
        // Task CRUD
        .route("/api/v1/tasks", get(list_tasks).post(create_task))
        .route(
            "/api/v1/tasks/{id}",
            get(get_task).put(edit_task).delete(delete_task),
        )
        // Status transitions
        .route("/api/v1/tasks/{id}/ready", post(ready_task))
        .route("/api/v1/tasks/{id}/start", post(start_task))
        .route("/api/v1/tasks/{id}/complete", post(complete_task))
        .route("/api/v1/tasks/{id}/cancel", post(cancel_task))
        // Dependencies
        .route("/api/v1/tasks/{id}/deps", get(list_deps).post(add_dep))
        .route("/api/v1/tasks/{id}/deps/{dep_id}", delete(remove_dep))
        // DoD
        .route("/api/v1/tasks/{id}/dod/{index}/check", post(check_dod))
        .route(
            "/api/v1/tasks/{id}/dod/{index}/uncheck",
            post(uncheck_dod),
        )
        // Other
        .route("/api/v1/config", get(get_config))
        .route("/api/v1/stats", get(get_stats))
        .with_state(state);

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
        eprintln!("Listening on http://localhost:{port}");
        eprintln!("             http://{device_ip}:{port}");
    } else {
        eprintln!("Listening on http://{bind_ip}:{port}");
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

// --- Handlers ---

// GET /api/v1/tasks
async fn list_tasks(
    State(state): State<AppState>,
    Query(query): Query<ListTasksQuery>,
) -> Result<Json<Vec<Task>>, ApiError> {
    let root = state.project_root.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db::open_db(&root).map_err(classify_error)?;
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
        let tasks = db::list_tasks(&conn, &filter).map_err(classify_error)?;
        Ok(Json(tasks))
    })
    .await
    .map_err(join_error)?
}

// POST /api/v1/tasks
async fn create_task(
    State(state): State<AppState>,
    Json(params): Json<CreateTaskParams>,
) -> Result<(StatusCode, Json<Task>), ApiError> {
    let root = state.project_root.clone();
    tokio::task::spawn_blocking(move || {
        let backend = db::SqliteBackend::new(&root).map_err(classify_error)?;

        // Handle branch template expansion
        let needs_template = params
            .branch
            .as_ref()
            .is_some_and(|b| b.contains("${task_id}"));

        let task = if needs_template {
            let branch_template = params.branch.clone();
            let mut params_without_branch = params;
            params_without_branch.branch = None;
            let created =
                backend.create_task(&params_without_branch).map_err(classify_error)?;
            let expanded =
                branch_template.as_deref().unwrap().replace("${task_id}", &created.id.to_string());
            backend.update_task(
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
            .map_err(classify_error)?
        } else {
            backend.create_task(&params).map_err(classify_error)?
        };

        // Fire hooks
        let config = hooks::load_config(&root, state.config_path.as_deref().map(|p| p.as_path())).map_err(classify_error)?;
        hooks::fire_hooks(&config, "task_added", &task, &backend, None, None);

        Ok((StatusCode::CREATED, Json(task)))
    })
    .await
    .map_err(join_error)?
}

// GET /api/v1/tasks/{id}
async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Task>, ApiError> {
    let root = state.project_root.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db::open_db(&root).map_err(classify_error)?;
        let task = db::get_task(&conn, id).map_err(classify_error)?;
        Ok(Json(task))
    })
    .await
    .map_err(join_error)?
}

// PUT /api/v1/tasks/{id}
async fn edit_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<EditTaskBody>,
) -> Result<Json<Task>, ApiError> {
    let root = state.project_root.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db::open_db(&root).map_err(classify_error)?;

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

        db::update_task(&conn, id, &scalar_params).map_err(classify_error)?;
        db::update_task_arrays(&conn, id, &array_params).map_err(classify_error)?;
        let task = db::get_task(&conn, id).map_err(classify_error)?;
        Ok(Json(task))
    })
    .await
    .map_err(join_error)?
}

// DELETE /api/v1/tasks/{id}
async fn delete_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let root = state.project_root.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db::open_db(&root).map_err(classify_error)?;
        db::delete_task(&conn, id).map_err(classify_error)?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
    .map_err(join_error)?
}

// POST /api/v1/tasks/{id}/ready
async fn ready_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Task>, ApiError> {
    let root = state.project_root.clone();
    tokio::task::spawn_blocking(move || {
        let backend = db::SqliteBackend::new(&root).map_err(classify_error)?;
        let updated = backend.ready_task(id).map_err(classify_error)?;

        let config = hooks::load_config(&root, state.config_path.as_deref().map(|p| p.as_path())).map_err(classify_error)?;
        hooks::fire_hooks(
            &config,
            "task_ready",
            &updated,
            &backend,
            Some(TaskStatus::Draft),
            None,
        );

        Ok(Json(updated))
    })
    .await
    .map_err(join_error)?
}

// POST /api/v1/tasks/{id}/start
async fn start_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<StartBody>,
) -> Result<Json<Task>, ApiError> {
    let root = state.project_root.clone();
    tokio::task::spawn_blocking(move || {
        let backend = db::SqliteBackend::new(&root).map_err(classify_error)?;
        let task = backend.get_task(id).map_err(classify_error)?;
        let prev_status = task.status;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let updated =
            backend.start_task(id, body.session_id, &now).map_err(classify_error)?;

        let config = hooks::load_config(&root, state.config_path.as_deref().map(|p| p.as_path())).map_err(classify_error)?;
        hooks::fire_hooks(
            &config,
            "task_started",
            &updated,
            &backend,
            Some(prev_status),
            None,
        );

        Ok(Json(updated))
    })
    .await
    .map_err(join_error)?
}

// POST /api/v1/tasks/{id}/complete
async fn complete_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    body: Option<Json<CompleteBody>>,
) -> Result<Json<Task>, ApiError> {
    let root = state.project_root.clone();
    let skip_pr_check = body.map(|b| b.skip_pr_check).unwrap_or(false);
    tokio::task::spawn_blocking(move || {
        let backend = db::SqliteBackend::new(&root).map_err(classify_error)?;
        let config = hooks::load_config(&root, state.config_path.as_deref().map(|p| p.as_path())).map_err(classify_error)?;

        let task = backend.get_task(id).map_err(classify_error)?;
        task.status
            .transition_to(TaskStatus::Completed)
            .map_err(classify_error)?;

        // Check DoD items
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

        // PR workflow checks
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

        let prev_ready_ids: std::collections::HashSet<i64> = backend.list_ready_tasks()
            .map_err(classify_error)?
            .iter()
            .map(|t| t.id)
            .collect();

        let prev_status = task.status;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let updated = backend.complete_task(id, &now).map_err(classify_error)?;

        let unblocked = hooks::compute_unblocked(&backend, &prev_ready_ids);
        let unblocked_opt = if unblocked.is_empty() {
            None
        } else {
            Some(unblocked)
        };
        hooks::fire_hooks(
            &config,
            "task_completed",
            &updated,
            &backend,
            Some(prev_status),
            unblocked_opt,
        );

        Ok(Json(updated))
    })
    .await
    .map_err(join_error)?
}

// POST /api/v1/tasks/{id}/cancel
async fn cancel_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    body: Option<Json<CancelBody>>,
) -> Result<Json<Task>, ApiError> {
    let root = state.project_root.clone();
    let reason = body.and_then(|b| b.0.reason);
    tokio::task::spawn_blocking(move || {
        let backend = db::SqliteBackend::new(&root).map_err(classify_error)?;
        let task = backend.get_task(id).map_err(classify_error)?;
        task.status
            .transition_to(TaskStatus::Canceled)
            .map_err(classify_error)?;

        let prev_status = task.status;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let updated = backend.cancel_task(id, &now, reason).map_err(classify_error)?;

        let config = hooks::load_config(&root, state.config_path.as_deref().map(|p| p.as_path())).map_err(classify_error)?;
        hooks::fire_hooks(
            &config,
            "task_canceled",
            &updated,
            &backend,
            Some(prev_status),
            None,
        );

        Ok(Json(updated))
    })
    .await
    .map_err(join_error)?
}

// POST /api/v1/tasks/next
async fn next_task(
    State(state): State<AppState>,
    body: Option<Json<NextBody>>,
) -> Result<Json<Task>, ApiError> {
    let root = state.project_root.clone();
    let session_id = body.and_then(|b| b.0.session_id);
    tokio::task::spawn_blocking(move || {
        let backend = db::SqliteBackend::new(&root).map_err(classify_error)?;
        let task = backend.next_task()
            .map_err(classify_error)?
            .ok_or_else(|| ApiError::NotFound("no eligible task found".to_string()))?;

        let prev_status = task.status;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let updated =
            backend.start_task(task.id, session_id, &now).map_err(classify_error)?;

        let config = hooks::load_config(&root, state.config_path.as_deref().map(|p| p.as_path())).map_err(classify_error)?;
        hooks::fire_hooks(
            &config,
            "task_started",
            &updated,
            &backend,
            Some(prev_status),
            None,
        );

        Ok(Json(updated))
    })
    .await
    .map_err(join_error)?
}

// GET /api/v1/tasks/{id}/deps
async fn list_deps(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<Task>>, ApiError> {
    let root = state.project_root.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db::open_db(&root).map_err(classify_error)?;
        let deps = db::list_dependencies(&conn, id).map_err(classify_error)?;
        Ok(Json(deps))
    })
    .await
    .map_err(join_error)?
}

// POST /api/v1/tasks/{id}/deps
async fn add_dep(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<AddDepBody>,
) -> Result<Json<Task>, ApiError> {
    let root = state.project_root.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db::open_db(&root).map_err(classify_error)?;
        let task = db::add_dependency(&conn, id, body.dep_id).map_err(classify_error)?;
        Ok(Json(task))
    })
    .await
    .map_err(join_error)?
}

// DELETE /api/v1/tasks/{id}/deps/{dep_id}
async fn remove_dep(
    State(state): State<AppState>,
    Path((id, dep_id)): Path<(i64, i64)>,
) -> Result<Json<Task>, ApiError> {
    let root = state.project_root.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db::open_db(&root).map_err(classify_error)?;
        let task = db::remove_dependency(&conn, id, dep_id).map_err(classify_error)?;
        Ok(Json(task))
    })
    .await
    .map_err(join_error)?
}

// POST /api/v1/tasks/{id}/dod/{index}/check
async fn check_dod(
    State(state): State<AppState>,
    Path((id, index)): Path<(i64, usize)>,
) -> Result<Json<Task>, ApiError> {
    let root = state.project_root.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db::open_db(&root).map_err(classify_error)?;
        let task = db::check_dod(&conn, id, index).map_err(classify_error)?;
        Ok(Json(task))
    })
    .await
    .map_err(join_error)?
}

// POST /api/v1/tasks/{id}/dod/{index}/uncheck
async fn uncheck_dod(
    State(state): State<AppState>,
    Path((id, index)): Path<(i64, usize)>,
) -> Result<Json<Task>, ApiError> {
    let root = state.project_root.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db::open_db(&root).map_err(classify_error)?;
        let task = db::uncheck_dod(&conn, id, index).map_err(classify_error)?;
        Ok(Json(task))
    })
    .await
    .map_err(join_error)?
}

// GET /api/v1/config
async fn get_config(
    State(state): State<AppState>,
) -> Result<Json<hooks::Config>, ApiError> {
    let root = state.project_root.clone();
    tokio::task::spawn_blocking(move || {
        let config = hooks::load_config(&root, state.config_path.as_deref().map(|p| p.as_path())).map_err(classify_error)?;
        Ok(Json(config))
    })
    .await
    .map_err(join_error)?
}

// GET /api/v1/stats
async fn get_stats(
    State(state): State<AppState>,
) -> Result<Json<HashMap<String, i64>>, ApiError> {
    let root = state.project_root.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db::open_db(&root).map_err(classify_error)?;
        let stats = db::task_stats(&conn).map_err(classify_error)?;
        Ok(Json(stats))
    })
    .await
    .map_err(join_error)?
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
