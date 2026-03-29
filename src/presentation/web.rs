use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::{Path, State};
use axum_extra::extract::Query;
use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use tower_http::trace::TraceLayer;

use pulldown_cmark::{Options, Parser};

use crate::domain::repository::TaskBackend;
use crate::bootstrap;
use crate::infra::hook as hooks;
use crate::domain::task::{DodItem, Priority, Task, TaskStatus};

#[derive(Clone)]
struct AppState {
    project_root: Arc<PathBuf>,
    backend: Arc<dyn TaskBackend>,
}

#[derive(serde::Deserialize)]
struct ListQuery {
    #[serde(default)]
    status: Vec<String>,
    #[serde(default)]
    tag: Vec<String>,
}

pub async fn serve(
    project_root: PathBuf,
    port: u16,
    port_is_explicit: bool,
    host: Option<String>,
    config_path: Option<PathBuf>,
    backend: Arc<dyn TaskBackend>,
) -> Result<()> {
    let config = hooks::load_config(&project_root, config_path.as_deref())
        .unwrap_or_default();
    bootstrap::init_tracing(&config.log);

    let state = AppState {
        project_root: Arc::new(project_root),
        backend,
    };

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/tasks/{id}", get(task_handler))
        .route("/graph", get(graph_handler))
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    let bind_addr_str = host
        .or_else(|| std::env::var("SENKO_HOST").ok().filter(|v| !v.is_empty()))
        .unwrap_or_else(|| "127.0.0.1".to_string());
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

async fn index_handler(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Html<String>, StatusCode> {
    let statuses = query.status
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<TaskStatus>())
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let tags: Vec<String> = query.tag.iter().filter(|t| !t.is_empty()).cloned().collect();
    let filter = crate::domain::task::ListTasksFilter {
        statuses,
        tags,
        ..Default::default()
    };
    let project_id = 1; // Default project for web viewer
    let tasks = state.backend.list_tasks(project_id, &filter).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Collect all tags from all tasks (unfiltered) for the filter UI
    let all_tasks = state.backend.list_tasks(project_id, &crate::domain::task::ListTasksFilter::default())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut all_tags: Vec<String> = all_tasks
        .iter()
        .flat_map(|t| t.tags().iter().cloned())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    all_tags.sort();

    let body = render_task_list(&tasks, &query, &all_tags);
    Ok(Html(layout("Tasks", &body)))
}

async fn task_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Html<String>, StatusCode> {
    let task = state.backend.get_task(1, id).await.map_err(|_| StatusCode::NOT_FOUND)?;

    let body = render_task_detail(&task);
    let title = format!("#{} {}", task.id(), escape_html(task.title()));
    Ok(Html(layout(&title, &body)))
}

async fn graph_handler(
    State(state): State<AppState>,
) -> Result<Html<String>, StatusCode> {
    let tasks = state.backend.list_tasks(1, &crate::domain::task::ListTasksFilter::default())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let body = render_graph_page(&tasks);
    Ok(Html(layout("Dependency Graph", &body)))
}

fn render_graph_page(tasks: &[Task]) -> String {
    let mut mermaid = String::from("graph TD\n");

    for task in tasks {
        let title = escape_mermaid(task.title());
        let priority = task.priority();
        mermaid.push_str(&format!(
            "    node_{}[\"#{} {} ({})\"]\n",
            task.id(), task.id(), title, priority
        ));
    }

    // Dependency edges: dep_id --> task_id
    for task in tasks {
        for dep in task.dependencies() {
            mermaid.push_str(&format!("    node_{} --> node_{}\n", dep, task.id()));
        }
    }

    // Click handlers
    for task in tasks {
        mermaid.push_str(&format!(
            "    click node_{} \"/tasks/{}\"\n",
            task.id(), task.id()
        ));
    }

    // Status-based styles
    let mut completed = Vec::new();
    let mut in_progress = Vec::new();
    let mut todo = Vec::new();
    let mut draft = Vec::new();
    let mut canceled = Vec::new();

    for task in tasks {
        let node = format!("node_{}", task.id());
        match task.status() {
            TaskStatus::Completed => completed.push(node),
            TaskStatus::InProgress => in_progress.push(node),
            TaskStatus::Todo => todo.push(node),
            TaskStatus::Draft => draft.push(node),
            TaskStatus::Canceled => canceled.push(node),
        }
    }

    if !completed.is_empty() {
        mermaid.push_str(&format!(
            "    classDef completed fill:#d1fae5,stroke:#065f46,color:#065f46\n"
        ));
        mermaid.push_str(&format!(
            "    class {} completed\n",
            completed.join(",")
        ));
    }
    if !in_progress.is_empty() {
        mermaid.push_str(&format!(
            "    classDef in_progress fill:#fef3c7,stroke:#92400e,color:#92400e\n"
        ));
        mermaid.push_str(&format!(
            "    class {} in_progress\n",
            in_progress.join(",")
        ));
    }
    if !todo.is_empty() {
        mermaid.push_str(&format!(
            "    classDef todo fill:#dbeafe,stroke:#1e40af,color:#1e40af\n"
        ));
        mermaid.push_str(&format!("    class {} todo\n", todo.join(",")));
    }
    if !draft.is_empty() {
        mermaid.push_str(&format!(
            "    classDef draft fill:#e0e0e0,stroke:#555,color:#555\n"
        ));
        mermaid.push_str(&format!("    class {} draft\n", draft.join(",")));
    }
    if !canceled.is_empty() {
        mermaid.push_str(&format!(
            "    classDef canceled fill:#fee2e2,stroke:#991b1b,color:#991b1b\n"
        ));
        mermaid.push_str(&format!(
            "    class {} canceled\n",
            canceled.join(",")
        ));
    }

    format!(
        r#"<h1>Dependency Graph</h1>
<div class="graph-container">
<pre class="mermaid">
{mermaid}
</pre>
</div>
<script src="https://cdn.jsdelivr.net/npm/mermaid/dist/mermaid.min.js"></script>
<script>mermaid.initialize({{ startOnLoad: true, securityLevel: 'loose' }});</script>"#
    )
}

fn escape_mermaid(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn layout(title: &str, body: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} - senko</title>
<style>
*, *::before, *::after {{ box-sizing: border-box; }}
body {{
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
    margin: 0;
    padding: 0;
    color: #1a1a1a;
    background: #fafafa;
    line-height: 1.5;
}}
a {{ color: #0066cc; text-decoration: none; }}
a:hover {{ text-decoration: underline; }}
h1 {{ font-size: 1.5rem; margin: 0 0 1rem; }}
h2 {{ font-size: 1.2rem; margin: 1.5rem 0 0.5rem; }}
.app {{ display: flex; min-height: 100vh; }}
.sidebar {{ width: 200px; background: #fff; border-right: 1px solid #e0e0e0; padding: 1rem 0; display: flex; flex-direction: column; flex-shrink: 0; transition: width 0.2s; }}
.sidebar-collapsed .sidebar {{ width: 48px; }}
.sidebar .logo {{ padding: 0.5rem 1rem; font-weight: 700; font-size: 1.1rem; color: #1a1a1a; text-decoration: none; display: block; white-space: nowrap; overflow: hidden; }}
.sidebar-collapsed .logo span {{ display: none; }}
.sidebar .nav-links {{ list-style: none; padding: 0; margin: 1rem 0; }}
.sidebar .nav-links a {{ display: flex; align-items: center; gap: 0.5rem; padding: 0.5rem 1rem; color: #555; text-decoration: none; font-size: 0.9rem; white-space: nowrap; overflow: hidden; }}
.sidebar .nav-links a:hover {{ background: #f0f0f0; color: #1a1a1a; }}
.sidebar-collapsed .nav-links a span {{ display: none; }}
.sidebar-toggle {{ background: none; border: none; cursor: pointer; padding: 0.5rem 1rem; color: #888; font-size: 1rem; text-align: left; }}
.sidebar-toggle:hover {{ color: #1a1a1a; }}
.main {{ flex: 1; max-width: 900px; padding: 1rem 2rem; }}
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
.filter-form-multi {{ margin-bottom: 1rem; display: flex; flex-direction: column; gap: 0.5rem; }}
.filter-group {{ display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap; }}
.filter-label {{ font-weight: 600; font-size: 0.85rem; color: #555; min-width: 60px; }}
.filter-options {{ display: flex; gap: 0.75rem; flex-wrap: wrap; }}
.filter-checkbox {{ font-size: 0.85rem; cursor: pointer; display: flex; align-items: center; gap: 0.25rem; }}
.filter-badge {{ position: relative; display: inline-block; }}
.filter-badge input[type="checkbox"] {{ position: absolute; opacity: 0; pointer-events: none; }}
.filter-badge .badge {{ cursor: pointer; opacity: 0.4; background: #e0e0e0; color: #888; transition: opacity 0.15s; }}
.filter-badge input[type="checkbox"]:checked + .badge {{ opacity: 1; }}
.filter-badge input[type="checkbox"]:checked + .status-draft {{ background: #e0e0e0; color: #555; }}
.filter-badge input[type="checkbox"]:checked + .status-todo {{ background: #dbeafe; color: #1e40af; }}
.filter-badge input[type="checkbox"]:checked + .status-in_progress {{ background: #fef3c7; color: #92400e; }}
.filter-badge input[type="checkbox"]:checked + .status-completed {{ background: #d1fae5; color: #065f46; }}
.filter-badge input[type="checkbox"]:checked + .status-canceled {{ background: #fee2e2; color: #991b1b; }}
.tag-pill {{ display: inline-block; background: #e8e8e8; padding: 0.1rem 0.4rem; border-radius: 4px; font-size: 0.8rem; }}
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
.mermaid {{ background: #fff; padding: 1rem; border-radius: 6px; border: 1px solid #e0e0e0; overflow-x: auto; }}
.graph-container {{ max-width: 100%; }}
</style>
</head>
<body>
<div class="app">
  <aside class="sidebar">
    <a href="/" class="logo">&#x1f504; <span>senko</span></a>
    <ul class="nav-links">
      <li><a href="/">&#x1f3e0; <span>Home</span></a></li>
      <li><a href="/graph">&#x1f4ca; <span>Graph</span></a></li>
    </ul>
    <button class="sidebar-toggle" onclick="document.body.classList.toggle('sidebar-collapsed')">&#x2630;</button>
  </aside>
  <main class="main">{body}</main>
</div>
</body>
</html>"#
    )
}

fn render_task_list(tasks: &[Task], query: &ListQuery, all_tags: &[String]) -> String {
    let statuses = ["draft", "todo", "in_progress", "completed", "canceled"];
    let labels = ["Draft", "Todo", "In Progress", "Completed", "Canceled"];

    let status_checkboxes: String = statuses
        .iter()
        .zip(labels.iter())
        .map(|(val, label)| {
            let checked = if query.status.iter().any(|s| s == val) {
                " checked"
            } else {
                ""
            };
            format!(
                r#"<label class="filter-badge"><input type="checkbox" name="status" value="{val}" onchange="this.form.submit()"{checked}><span class="badge status-{val}">{label}</span></label>"#
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let tag_checkboxes: String = if all_tags.is_empty() {
        String::new()
    } else {
        let cbs: String = all_tags
            .iter()
            .map(|tag| {
                let checked = if query.tag.iter().any(|t| t == tag) {
                    " checked"
                } else {
                    ""
                };
                let escaped = escape_html(tag);
                format!(
                    r#"<label class="filter-checkbox"><input type="checkbox" name="tag" value="{escaped}" onchange="this.form.submit()"{checked}> {escaped}</label>"#
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            r#"<div class="filter-group"><span class="filter-label">Tags:</span>
<div class="filter-options">{cbs}</div></div>"#
        )
    };

    let filter = format!(
        r#"<form class="filter-form-multi" method="get" action="/">
<div class="filter-group"><span class="filter-label">Status:</span>
<div class="filter-options">{status_checkboxes}</div></div>
{tag_checkboxes}
</form>"#
    );

    if tasks.is_empty() {
        return format!("{filter}<p class=\"empty\">No tasks found.</p>");
    }

    let rows: String = tasks
        .iter()
        .map(|t| {
            let title = escape_html(t.title());
            let status = status_badge(t.status());
            let priority = priority_badge(t.priority());
            let created = escape_html(&t.created_at().split('T').next().unwrap_or(t.created_at()));
            let tags_html = if t.tags().is_empty() {
                String::new()
            } else {
                t.tags()
                    .iter()
                    .map(|tag| format!(r#"<span class="tag-pill">{}</span>"#, escape_html(tag)))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            format!(
                r#"<tr>
<td>{}</td>
<td><a href="/tasks/{}">{}</a></td>
<td>{}</td>
<td>{}</td>
<td>{}</td>
<td>{}</td>
</tr>"#,
                t.id(), t.id(), title, status, priority, tags_html, created
            )
        })
        .collect();

    format!(
        r#"{filter}
<table>
<thead><tr><th>ID</th><th>Title</th><th>Status</th><th>Priority</th><th>Tags</th><th>Created</th></tr></thead>
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
        task.id(),
        escape_html(task.title())
    ));

    // Meta grid
    html.push_str("<div class=\"meta\">");
    html.push_str(&meta_item("Status", &status_badge(task.status())));
    html.push_str(&meta_item("Priority", &priority_badge(task.priority())));
    html.push_str(&meta_item("Created", &escape_html(task.created_at())));
    html.push_str(&meta_item("Updated", &escape_html(task.updated_at())));
    if let Some(started) = task.started_at() {
        html.push_str(&meta_item("Started", &escape_html(started)));
    }
    if let Some(completed) = task.completed_at() {
        html.push_str(&meta_item("Completed", &escape_html(completed)));
    }
    if let Some(canceled) = task.canceled_at() {
        html.push_str(&meta_item("Canceled", &escape_html(canceled)));
    }
    if let Some(session) = task.assignee_session_id() {
        html.push_str(&meta_item("Assignee (session)", &escape_html(session)));
    }
    if let Some(uid) = task.assignee_user_id() {
        html.push_str(&meta_item("Assignee (user)", &format!("#{uid}")));
    }
    if let Some(branch) = task.branch() {
        html.push_str(&meta_item("Branch", &escape_html(branch)));
    }
    html.push_str("</div>");

    // Background
    if let Some(bg) = task.background() {
        html.push_str("<h2>Background</h2>");
        html.push_str(&format!(
            "<div class=\"section\"><pre>{}</pre></div>",
            escape_html(bg)
        ));
    }

    // Description
    if let Some(description) = task.description() {
        html.push_str("<h2>Description</h2>");
        html.push_str(&format!(
            "<div class=\"section markdown\">{}</div>",
            render_markdown(description)
        ));
    }

    // Plan
    if let Some(plan) = task.plan() {
        html.push_str("<h2>Plan</h2>");
        html.push_str(&format!(
            "<div class=\"section markdown\">{}</div>",
            render_markdown(plan)
        ));
    }

    // Cancel reason
    if let Some(reason) = task.cancel_reason() {
        html.push_str("<h2>Cancel Reason</h2>");
        html.push_str(&format!(
            "<div class=\"section\"><pre>{}</pre></div>",
            escape_html(reason)
        ));
    }

    // Definition of Done
    if !task.definition_of_done().is_empty() {
        html.push_str("<h2>Definition of Done</h2>");
        html.push_str("<div class=\"section\">");
        for (i, item) in task.definition_of_done().iter().enumerate() {
            html.push_str(&render_dod_item(i + 1, item));
        }
        html.push_str("</div>");
    }

    // Tags
    if !task.tags().is_empty() {
        html.push_str("<h2>Tags</h2>");
        html.push_str("<ul class=\"tag-list\">");
        for tag in task.tags() {
            html.push_str(&format!("<li>{}</li>", escape_html(tag)));
        }
        html.push_str("</ul>");
    }

    // Dependencies
    if !task.dependencies().is_empty() {
        html.push_str("<h2>Dependencies</h2>");
        html.push_str("<div class=\"section\">");
        for dep in task.dependencies() {
            html.push_str(&format!(
                "<div><a href=\"/tasks/{dep}\">#{dep}</a></div>"
            ));
        }
        html.push_str("</div>");
    }

    // In scope
    if !task.in_scope().is_empty() {
        html.push_str("<h2>In Scope</h2>");
        html.push_str("<div class=\"section\"><ul>");
        for item in task.in_scope() {
            html.push_str(&format!("<li>{}</li>", escape_html(item)));
        }
        html.push_str("</ul></div>");
    }

    // Out of scope
    if !task.out_of_scope().is_empty() {
        html.push_str("<h2>Out of Scope</h2>");
        html.push_str("<div class=\"section\"><ul>");
        for item in task.out_of_scope() {
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
    let (class, icon) = if item.checked() {
        ("dod-checked", "&#9745;")
    } else {
        ("dod-unchecked", "&#9744;")
    };
    format!(
        r#"<div class="dod-item {class}">{icon} {index}. {}</div>"#,
        escape_html(item.content())
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
