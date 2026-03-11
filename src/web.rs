use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::get;
use axum::Router;

use pulldown_cmark::{Options, Parser};

use crate::db;
use crate::models::{DodItem, Priority, Task, TaskStatus};

#[derive(Clone)]
struct AppState {
    project_root: Arc<PathBuf>,
}

#[derive(serde::Deserialize)]
struct ListQuery {
    status: Option<String>,
}

pub async fn serve(project_root: PathBuf, port: u16, host: bool) -> Result<()> {
    let state = AppState {
        project_root: Arc::new(project_root),
    };

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/tasks/{id}", get(task_handler))
        .with_state(state);

    let expose = host
        || std::env::var("LOCALFLOW_WEB_HOST")
            .map(|v| !v.is_empty() && v != "0" && v != "false")
            .unwrap_or(false);
    let ip = if expose {
        [0, 0, 0, 0]
    } else {
        [127, 0, 0, 1]
    };
    let addr = std::net::SocketAddr::from((ip, port));
    if expose {
        let device_ip = get_local_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "0.0.0.0".to_string());
        eprintln!("Listening on http://localhost:{port}");
        eprintln!("             http://{device_ip}:{port}");
    } else {
        eprintln!("Listening on http://localhost:{port}");
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

async fn index_handler(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Html<String>, StatusCode> {
    let root = state.project_root.clone();
    let status_filter = query.status.clone();

    let tasks = tokio::task::spawn_blocking(move || {
        let conn = db::open_db(&root).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let filter = crate::models::ListTasksFilter {
            status: status_filter
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|s| s.parse::<TaskStatus>())
                .transpose()
                .map_err(|_| StatusCode::BAD_REQUEST)?,
            ..Default::default()
        };
        db::list_tasks(&conn, &filter).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;

    let body = render_task_list(&tasks, &query);
    Ok(Html(layout("Tasks", &body)))
}

async fn task_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Html<String>, StatusCode> {
    let root = state.project_root.clone();

    let task = tokio::task::spawn_blocking(move || {
        let conn = db::open_db(&root).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        db::get_task(&conn, id).map_err(|_| StatusCode::NOT_FOUND)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;

    let body = render_task_detail(&task);
    let title = format!("#{} {}", task.id, escape_html(&task.title));
    Ok(Html(layout(&title, &body)))
}

fn layout(title: &str, body: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} - localflow</title>
<style>
*, *::before, *::after {{ box-sizing: border-box; }}
body {{
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
    max-width: 900px;
    margin: 0 auto;
    padding: 1rem;
    color: #1a1a1a;
    background: #fafafa;
    line-height: 1.5;
}}
a {{ color: #0066cc; text-decoration: none; }}
a:hover {{ text-decoration: underline; }}
h1 {{ font-size: 1.5rem; margin: 0 0 1rem; }}
h2 {{ font-size: 1.2rem; margin: 1.5rem 0 0.5rem; }}
nav {{ margin-bottom: 1rem; }}
table {{ width: 100%; border-collapse: collapse; }}
th, td {{ text-align: left; padding: 0.5rem; border-bottom: 1px solid #e0e0e0; }}
th {{ font-weight: 600; color: #555; font-size: 0.85rem; text-transform: uppercase; }}
tr:hover {{ background: #f0f0f0; }}
.badge {{
    display: inline-block;
    padding: 0.15rem 0.5rem;
    border-radius: 999px;
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
}}
.status-draft {{ background: #e0e0e0; color: #555; }}
.status-todo {{ background: #dbeafe; color: #1e40af; }}
.status-in_progress {{ background: #fef3c7; color: #92400e; }}
.status-completed {{ background: #d1fae5; color: #065f46; }}
.status-canceled {{ background: #fee2e2; color: #991b1b; }}
.priority-p0 {{ background: #fee2e2; color: #991b1b; }}
.priority-p1 {{ background: #ffedd5; color: #9a3412; }}
.priority-p2 {{ background: #dbeafe; color: #1e40af; }}
.priority-p3 {{ background: #e0e0e0; color: #555; }}
.meta {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 0.5rem; margin: 1rem 0; }}
.meta-item {{ background: #fff; padding: 0.5rem 0.75rem; border-radius: 6px; border: 1px solid #e0e0e0; }}
.meta-label {{ font-size: 0.75rem; color: #888; text-transform: uppercase; }}
.section {{ background: #fff; padding: 0.75rem 1rem; border-radius: 6px; border: 1px solid #e0e0e0; margin-bottom: 1rem; }}
.dod-item {{ padding: 0.25rem 0; }}
.dod-checked {{ color: #065f46; }}
.dod-unchecked {{ color: #555; }}
ul.tag-list {{ list-style: none; padding: 0; display: flex; gap: 0.5rem; flex-wrap: wrap; }}
ul.tag-list li {{ background: #e8e8e8; padding: 0.15rem 0.5rem; border-radius: 4px; font-size: 0.85rem; }}
select {{ padding: 0.35rem 0.5rem; border-radius: 4px; border: 1px solid #ccc; }}
.filter-form {{ margin-bottom: 1rem; display: flex; align-items: center; gap: 0.5rem; }}
.empty {{ text-align: center; padding: 2rem; color: #888; }}
pre {{ white-space: pre-wrap; word-break: break-word; }}
.markdown h1, .markdown h2, .markdown h3, .markdown h4 {{ margin: 0.75rem 0 0.5rem; }}
.markdown h1 {{ font-size: 1.3rem; }}
.markdown h2 {{ font-size: 1.15rem; }}
.markdown h3 {{ font-size: 1.05rem; }}
.markdown ul, .markdown ol {{ padding-left: 1.5rem; margin: 0.5rem 0; }}
.markdown li {{ margin: 0.25rem 0; }}
.markdown code {{ background: #f0f0f0; padding: 0.1rem 0.3rem; border-radius: 3px; font-size: 0.9em; }}
.markdown pre code {{ display: block; padding: 0.75rem; overflow-x: auto; background: #f5f5f5; border-radius: 4px; }}
.markdown table {{ width: 100%; border-collapse: collapse; margin: 0.5rem 0; }}
.markdown th, .markdown td {{ padding: 0.4rem 0.6rem; border: 1px solid #ddd; }}
.markdown th {{ background: #f5f5f5; font-weight: 600; }}
.markdown blockquote {{ margin: 0.5rem 0; padding: 0.25rem 0.75rem; border-left: 3px solid #ddd; color: #666; }}
.markdown p {{ margin: 0.5rem 0; }}
.markdown p:first-child {{ margin-top: 0; }}
.markdown p:last-child {{ margin-bottom: 0; }}
</style>
</head>
<body>
<nav><a href="/">localflow</a></nav>
{body}
</body>
</html>"#
    )
}

fn render_task_list(tasks: &[Task], query: &ListQuery) -> String {
    let current_status = query.status.as_deref().unwrap_or("");
    let statuses = ["", "draft", "todo", "in_progress", "completed", "canceled"];
    let labels = ["All", "Draft", "Todo", "In Progress", "Completed", "Canceled"];

    let options: String = statuses
        .iter()
        .zip(labels.iter())
        .map(|(val, label)| {
            let selected = if *val == current_status {
                " selected"
            } else {
                ""
            };
            format!(r#"<option value="{val}"{selected}>{label}</option>"#)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let filter = format!(
        r#"<form class="filter-form" method="get" action="/">
<label for="status">Status:</label>
<select name="status" id="status" onchange="this.form.submit()">
{options}
</select>
</form>"#
    );

    if tasks.is_empty() {
        return format!("{filter}<p class=\"empty\">No tasks found.</p>");
    }

    let rows: String = tasks
        .iter()
        .map(|t| {
            let title = escape_html(&t.title);
            let status = status_badge(t.status);
            let priority = priority_badge(t.priority);
            let created = escape_html(&t.created_at.split('T').next().unwrap_or(&t.created_at));
            format!(
                r#"<tr>
<td>{}</td>
<td><a href="/tasks/{}">{}</a></td>
<td>{}</td>
<td>{}</td>
<td>{}</td>
</tr>"#,
                t.id, t.id, title, status, priority, created
            )
        })
        .collect();

    format!(
        r#"{filter}
<table>
<thead><tr><th>ID</th><th>Title</th><th>Status</th><th>Priority</th><th>Created</th></tr></thead>
<tbody>
{rows}
</tbody>
</table>"#
    )
}

fn render_task_detail(task: &Task) -> String {
    let mut html = String::new();

    // Header
    html.push_str(&format!(
        "<h1>#{} {}</h1>",
        task.id,
        escape_html(&task.title)
    ));

    // Meta grid
    html.push_str("<div class=\"meta\">");
    html.push_str(&meta_item("Status", &status_badge(task.status)));
    html.push_str(&meta_item("Priority", &priority_badge(task.priority)));
    html.push_str(&meta_item("Created", &escape_html(&task.created_at)));
    html.push_str(&meta_item("Updated", &escape_html(&task.updated_at)));
    if let Some(ref started) = task.started_at {
        html.push_str(&meta_item("Started", &escape_html(started)));
    }
    if let Some(ref completed) = task.completed_at {
        html.push_str(&meta_item("Completed", &escape_html(completed)));
    }
    if let Some(ref canceled) = task.canceled_at {
        html.push_str(&meta_item("Canceled", &escape_html(canceled)));
    }
    if let Some(ref session) = task.assignee_session_id {
        html.push_str(&meta_item("Assignee", &escape_html(session)));
    }
    if let Some(ref branch) = task.branch {
        html.push_str(&meta_item("Branch", &escape_html(branch)));
    }
    html.push_str("</div>");

    // Background
    if let Some(ref bg) = task.background {
        html.push_str("<h2>Background</h2>");
        html.push_str(&format!(
            "<div class=\"section\"><pre>{}</pre></div>",
            escape_html(bg)
        ));
    }

    // Details
    if let Some(ref details) = task.details {
        html.push_str("<h2>Details</h2>");
        html.push_str(&format!(
            "<div class=\"section markdown\">{}</div>",
            render_markdown(details)
        ));
    }

    // Cancel reason
    if let Some(ref reason) = task.cancel_reason {
        html.push_str("<h2>Cancel Reason</h2>");
        html.push_str(&format!(
            "<div class=\"section\"><pre>{}</pre></div>",
            escape_html(reason)
        ));
    }

    // Definition of Done
    if !task.definition_of_done.is_empty() {
        html.push_str("<h2>Definition of Done</h2>");
        html.push_str("<div class=\"section\">");
        for (i, item) in task.definition_of_done.iter().enumerate() {
            html.push_str(&render_dod_item(i + 1, item));
        }
        html.push_str("</div>");
    }

    // Tags
    if !task.tags.is_empty() {
        html.push_str("<h2>Tags</h2>");
        html.push_str("<ul class=\"tag-list\">");
        for tag in &task.tags {
            html.push_str(&format!("<li>{}</li>", escape_html(tag)));
        }
        html.push_str("</ul>");
    }

    // Dependencies
    if !task.dependencies.is_empty() {
        html.push_str("<h2>Dependencies</h2>");
        html.push_str("<div class=\"section\">");
        for dep in &task.dependencies {
            html.push_str(&format!(
                "<div><a href=\"/tasks/{dep}\">#{dep}</a></div>"
            ));
        }
        html.push_str("</div>");
    }

    // In scope
    if !task.in_scope.is_empty() {
        html.push_str("<h2>In Scope</h2>");
        html.push_str("<div class=\"section\"><ul>");
        for item in &task.in_scope {
            html.push_str(&format!("<li>{}</li>", escape_html(item)));
        }
        html.push_str("</ul></div>");
    }

    // Out of scope
    if !task.out_of_scope.is_empty() {
        html.push_str("<h2>Out of Scope</h2>");
        html.push_str("<div class=\"section\"><ul>");
        for item in &task.out_of_scope {
            html.push_str(&format!("<li>{}</li>", escape_html(item)));
        }
        html.push_str("</ul></div>");
    }

    html
}

fn meta_item(label: &str, value: &str) -> String {
    format!(
        r#"<div class="meta-item"><div class="meta-label">{label}</div><div>{value}</div></div>"#
    )
}

fn render_dod_item(index: usize, item: &DodItem) -> String {
    let (class, icon) = if item.checked {
        ("dod-checked", "&#9745;")
    } else {
        ("dod-unchecked", "&#9744;")
    };
    format!(
        r#"<div class="dod-item {class}">{icon} {index}. {}</div>"#,
        escape_html(&item.content)
    )
}

fn status_badge(status: TaskStatus) -> String {
    let class = match status {
        TaskStatus::Draft => "status-draft",
        TaskStatus::Todo => "status-todo",
        TaskStatus::InProgress => "status-in_progress",
        TaskStatus::Completed => "status-completed",
        TaskStatus::Canceled => "status-canceled",
    };
    format!(r#"<span class="badge {class}">{status}</span>"#)
}

fn priority_badge(priority: Priority) -> String {
    let class = match priority {
        Priority::P0 => "priority-p0",
        Priority::P1 => "priority-p1",
        Priority::P2 => "priority-p2",
        Priority::P3 => "priority-p3",
    };
    format!(r#"<span class="badge {class}">{priority}</span>"#)
}

fn render_markdown(input: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(input, options);
    let mut raw_html = String::new();
    pulldown_cmark::html::push_html(&mut raw_html, parser);
    ammonia::clean(&raw_html)
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}
