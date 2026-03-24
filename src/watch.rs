use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use chrono::Utc;
use rusqlite::Connection;
use uuid::Uuid;

use crate::db;
use crate::models::{ListTasksFilter, Priority, Task, TaskStatus};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DaemonInfo {
    pub pid: u32,
    pub interval: u64,
    pub started_at: String,
}

fn write_daemon_info(project_root: &Path, info: &DaemonInfo) -> Result<()> {
    let pid_path = pid_file_path(project_root);
    let json = serde_json::to_string(info)?;
    std::fs::write(&pid_path, json)?;
    Ok(())
}

fn read_daemon_info(project_root: &Path) -> Result<Option<DaemonInfo>> {
    let pid_path = pid_file_path(project_root);
    if !pid_path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&pid_path)?;
    let trimmed = content.trim();
    // Try JSON format first, fall back to plain PID (legacy)
    if let Ok(info) = serde_json::from_str::<DaemonInfo>(trimmed) {
        return Ok(Some(info));
    }
    // Legacy: plain PID number
    if let Ok(pid) = trimmed.parse::<u32>() {
        return Ok(Some(DaemonInfo {
            pid,
            interval: 0,
            started_at: String::new(),
        }));
    }
    bail!("invalid content in watch.pid");
}

#[derive(Debug, Serialize)]
pub struct DaemonStatus {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime_seconds: Option<i64>,
}

pub fn daemon_status(project_root: &Path, output_json: bool) -> Result<()> {
    let info = read_daemon_info(project_root)?;
    let status = match info {
        None => DaemonStatus {
            status: "stopped".into(),
            pid: None,
            interval: None,
            started_at: None,
            uptime_seconds: None,
        },
        Some(info) => {
            if is_process_alive(info.pid) {
                let uptime = if !info.started_at.is_empty() {
                    chrono::DateTime::parse_from_rfc3339(&info.started_at)
                        .map(|started| {
                            let now = Utc::now();
                            (now - started.with_timezone(&chrono::Utc)).num_seconds()
                        })
                        .ok()
                } else {
                    None
                };
                DaemonStatus {
                    status: "running".into(),
                    pid: Some(info.pid),
                    interval: if info.interval > 0 {
                        Some(info.interval)
                    } else {
                        None
                    },
                    started_at: if info.started_at.is_empty() {
                        None
                    } else {
                        Some(info.started_at)
                    },
                    uptime_seconds: uptime,
                }
            } else {
                // Stale PID file — clean up
                let pid_path = pid_file_path(project_root);
                let _ = std::fs::remove_file(&pid_path);
                DaemonStatus {
                    status: "stopped".into(),
                    pid: None,
                    interval: None,
                    started_at: None,
                    uptime_seconds: None,
                }
            }
        }
    };

    if output_json {
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else {
        println!("Status: {}", status.status);
        if let Some(pid) = status.pid {
            println!("PID: {}", pid);
        }
        if let Some(interval) = status.interval {
            println!("Interval: {}s", interval);
        }
        if let Some(ref started) = status.started_at {
            println!("Started: {}", started);
        }
        if let Some(uptime) = status.uptime_seconds {
            let hours = uptime / 3600;
            let minutes = (uptime % 3600) / 60;
            let seconds = uptime % 60;
            println!("Uptime: {}h {}m {}s", hours, minutes, seconds);
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub hooks: HooksConfig,
}

#[derive(Debug, Deserialize, Default)]
pub struct HooksConfig {
    pub on_task_added: Option<String>,
    pub on_task_completed: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct UnblockedTask {
    pub id: i64,
    pub title: String,
    pub priority: Priority,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct WatchEvent {
    pub event_id: String,
    pub event: String,
    pub timestamp: String,
    pub task: Task,
    pub stats: HashMap<String, i64>,
    pub ready_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unblocked_tasks: Option<Vec<UnblockedTask>>,
}

pub fn load_config(project_root: &Path) -> Result<Config> {
    let config_path = project_root.join(".localflow").join("config.toml");
    if !config_path.exists() {
        return Ok(Config::default());
    }
    let content =
        std::fs::read_to_string(&config_path).context("failed to read .localflow/config.toml")?;
    let config: Config = toml::from_str(&content).context("failed to parse config.toml")?;
    Ok(config)
}

fn pid_file_path(project_root: &Path) -> PathBuf {
    project_root.join(".localflow").join("watch.pid")
}

fn execute_hook(command: &str, event: &WatchEvent) -> Result<()> {
    let json = serde_json::to_string(event)?;
    let mut child = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn hook command")?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(json.as_bytes())?;
    }
    let status = child.wait()?;
    if !status.success() {
        eprintln!("hook command exited with status: {}", status);
    }
    Ok(())
}

fn is_process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn build_event(
    event_name: &str,
    task: &Task,
    conn: &Connection,
    unblocked: Option<Vec<UnblockedTask>>,
) -> WatchEvent {
    let stats = db::task_stats(conn).unwrap_or_default();
    let ready_count = db::ready_count(conn).unwrap_or(0);
    WatchEvent {
        event_id: Uuid::new_v4().to_string(),
        event: event_name.into(),
        timestamp: Utc::now().to_rfc3339(),
        task: task.clone(),
        stats,
        ready_count,
        unblocked_tasks: unblocked,
    }
}

/// Detect events by comparing previous state with current tasks.
fn detect_events(
    known_ids: &HashSet<i64>,
    statuses: &HashMap<i64, TaskStatus>,
    tasks: &[Task],
    config: &Config,
    conn: &Connection,
    prev_ready_ids: &HashSet<i64>,
) -> Vec<WatchEvent> {
    let mut has_completed = false;
    let mut raw_events: Vec<(&str, &Task, bool)> = Vec::new();

    for task in tasks {
        if !known_ids.contains(&task.id) {
            if config.hooks.on_task_added.is_some() {
                raw_events.push(("task_added", task, false));
            }
        }
        if task.status == TaskStatus::Completed {
            if let Some(prev) = statuses.get(&task.id) {
                if *prev != TaskStatus::Completed && config.hooks.on_task_completed.is_some() {
                    raw_events.push(("task_completed", task, true));
                    has_completed = true;
                }
            }
        }
    }

    let unblocked = if has_completed {
        let curr_ready = db::list_ready_tasks(conn).unwrap_or_default();
        let unblocked_list: Vec<UnblockedTask> = curr_ready
            .iter()
            .filter(|t| !prev_ready_ids.contains(&t.id))
            .map(|t| UnblockedTask {
                id: t.id,
                title: t.title.clone(),
                priority: t.priority,
                metadata: t.metadata.clone(),
            })
            .collect();
        Some(unblocked_list)
    } else {
        None
    };

    raw_events
        .into_iter()
        .map(|(name, task, is_completed)| {
            let ub = if is_completed {
                unblocked.clone()
            } else {
                None
            };
            build_event(name, task, conn, ub)
        })
        .collect()
}

fn fire_event(config: &Config, event: &WatchEvent) {
    let command = match event.event.as_str() {
        "task_added" => config.hooks.on_task_added.as_deref(),
        "task_completed" => config.hooks.on_task_completed.as_deref(),
        _ => None,
    };
    if let Some(cmd) = command {
        if let Err(e) = execute_hook(cmd, event) {
            eprintln!("hook error ({}): {:#}", event.event, e);
        }
    }
}

pub fn run_watch_loop(project_root: &Path, interval_secs: u64) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let config = load_config(project_root)?;
        if config.hooks.on_task_added.is_none() && config.hooks.on_task_completed.is_none() {
            eprintln!("warning: no hooks configured in .localflow/config.toml");
        }

        let conn = db::open_db(project_root)?;
        let filter = ListTasksFilter::default();

        // Initial snapshot
        let mut known_ids = HashSet::new();
        let mut statuses = HashMap::new();
        let tasks = db::list_tasks(&conn, &filter)?;
        for task in &tasks {
            known_ids.insert(task.id);
            statuses.insert(task.id, task.status);
        }

        eprintln!(
            "Watching for task events (interval: {}s, Ctrl+C to stop)...",
            interval_secs
        );

        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
        interval.tick().await; // consume first immediate tick

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let tasks = db::list_tasks(&conn, &filter)?;
                    let prev_ready = db::list_ready_tasks(&conn)?;
                    let prev_ready_ids: HashSet<i64> = prev_ready.iter().map(|t| t.id).collect();
                    let events = detect_events(&known_ids, &statuses, &tasks, &config, &conn, &prev_ready_ids);

                    for event in &events {
                        fire_event(&config, event);
                    }

                    // Update state
                    for task in &tasks {
                        known_ids.insert(task.id);
                        statuses.insert(task.id, task.status);
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    eprintln!("\nStopping watch...");
                    // Clean up PID file if running as daemon
                    let pid_path = pid_file_path(project_root);
                    if pid_path.exists() {
                        let _ = std::fs::remove_file(&pid_path);
                    }
                    break;
                }
            }
        }
        Ok(())
    })
}

pub fn start_daemon(project_root: &Path, interval_secs: u64) -> Result<()> {
    let pid_path = pid_file_path(project_root);
    if let Some(info) = read_daemon_info(project_root)? {
        if is_process_alive(info.pid) {
            bail!("watch daemon already running (PID {})", info.pid);
        }
        // Stale PID file
        std::fs::remove_file(&pid_path)?;
    }

    let exe = std::env::current_exe().context("failed to get current executable path")?;
    let root = project_root
        .canonicalize()
        .context("failed to canonicalize project root")?;

    let child = std::process::Command::new(&exe)
        .args([
            "--project-root",
            &root.to_string_lossy(),
            "watch",
            "--interval",
            &interval_secs.to_string(),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("failed to spawn daemon process")?;

    let pid = child.id();
    let info = DaemonInfo {
        pid,
        interval: interval_secs,
        started_at: Utc::now().to_rfc3339(),
    };
    write_daemon_info(project_root, &info)?;
    eprintln!("Watch daemon started (PID {})", pid);
    Ok(())
}

pub fn stop_daemon(project_root: &Path) -> Result<()> {
    let pid_path = pid_file_path(project_root);
    let info = read_daemon_info(project_root)?;
    let info = match info {
        Some(info) => info,
        None => bail!("no watch daemon running (PID file not found)"),
    };

    let status = std::process::Command::new("kill")
        .arg(info.pid.to_string())
        .status()
        .context("failed to send signal to daemon")?;

    std::fs::remove_file(&pid_path)?;

    if status.success() {
        eprintln!("Watch daemon stopped (PID {})", info.pid);
    } else {
        eprintln!(
            "Watch daemon (PID {}) may have already exited, PID file removed",
            info.pid
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open_db(dir.path()).unwrap();
        (dir, conn)
    }

    #[test]
    fn load_config_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = load_config(dir.path()).unwrap();
        assert!(config.hooks.on_task_added.is_none());
        assert!(config.hooks.on_task_completed.is_none());
    }

    #[test]
    fn load_config_valid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let localflow_dir = dir.path().join(".localflow");
        std::fs::create_dir_all(&localflow_dir).unwrap();
        std::fs::write(
            localflow_dir.join("config.toml"),
            r#"
[hooks]
on_task_added = "echo added"
on_task_completed = "echo completed"
"#,
        )
        .unwrap();

        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.hooks.on_task_added.as_deref(), Some("echo added"));
        assert_eq!(
            config.hooks.on_task_completed.as_deref(),
            Some("echo completed")
        );
    }

    #[test]
    fn load_config_empty_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let localflow_dir = dir.path().join(".localflow");
        std::fs::create_dir_all(&localflow_dir).unwrap();
        std::fs::write(localflow_dir.join("config.toml"), "[hooks]\n").unwrap();

        let config = load_config(dir.path()).unwrap();
        assert!(config.hooks.on_task_added.is_none());
        assert!(config.hooks.on_task_completed.is_none());
    }

    #[test]
    fn watch_event_serialization() {
        let (_dir, conn) = setup_db();
        let task = Task {
            id: 1,
            title: "Test".into(),
            background: None,
            description: None,
            plan: None,
            priority: crate::models::Priority::P2,
            status: TaskStatus::Draft,
            assignee_session_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            started_at: None,
            completed_at: None,
            canceled_at: None,
            cancel_reason: None,
            branch: None,
            metadata: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        let event = build_event("task_added", &task, &conn, None);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"task_added\""));
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"event_id\""));
        assert!(json.contains("\"timestamp\""));
        assert!(json.contains("\"stats\""));
        assert!(json.contains("\"ready_count\""));
        // unblocked_tasks should be absent when None
        assert!(!json.contains("\"unblocked_tasks\""));
    }

    #[test]
    fn event_has_valid_uuid_and_timestamp() {
        let (_dir, conn) = setup_db();
        let task = Task {
            id: 1,
            title: "Test".into(),
            background: None,
            description: None,
            plan: None,
            priority: crate::models::Priority::P2,
            status: TaskStatus::Draft,
            assignee_session_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            started_at: None,
            completed_at: None,
            canceled_at: None,
            cancel_reason: None,
            branch: None,
            metadata: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        let event = build_event("task_added", &task, &conn, None);
        // Validate UUID v4 format
        assert!(Uuid::parse_str(&event.event_id).is_ok());
        // Validate ISO 8601 timestamp
        assert!(chrono::DateTime::parse_from_rfc3339(&event.timestamp).is_ok());
    }

    #[test]
    fn event_has_stats() {
        let (_dir, conn) = setup_db();
        // Add a task to have something in stats
        db::create_task(
            &conn,
            &crate::models::CreateTaskParams {
                title: "Task1".into(),
                background: None,
                description: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                branch: None,
                metadata: None,
                tags: vec![],
                dependencies: vec![],
            },
        )
        .unwrap();
        let task = db::get_task(&conn, 1).unwrap();
        let event = build_event("task_added", &task, &conn, None);
        assert!(event.stats.contains_key("draft"));
        assert_eq!(*event.stats.get("draft").unwrap(), 1);
    }

    #[test]
    fn event_has_ready_count() {
        let (_dir, conn) = setup_db();
        // Create a todo task with no dependencies → should be ready
        db::create_task(
            &conn,
            &crate::models::CreateTaskParams {
                title: "Ready".into(),
                background: None,
                description: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                branch: None,
                metadata: None,
                tags: vec![],
                dependencies: vec![],
            },
        )
        .unwrap();
        db::update_task(
            &conn,
            1,
            &crate::models::UpdateTaskParams {
                status: Some(TaskStatus::Todo),
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
                branch: None,
                metadata: None,
            },
        )
        .unwrap();
        let task = db::get_task(&conn, 1).unwrap();
        let event = build_event("task_added", &task, &conn, None);
        assert_eq!(event.ready_count, 1);
    }

    #[test]
    fn detect_events_new_task() {
        let (_dir, conn) = setup_db();
        let known_ids = HashSet::new();
        let statuses = HashMap::new();
        let prev_ready_ids = HashSet::new();
        let config = Config {
            hooks: HooksConfig {
                on_task_added: Some("echo".into()),
                on_task_completed: None,
            },
        };
        let task = Task {
            id: 1,
            title: "New".into(),
            background: None,
            description: None,
            plan: None,
            priority: crate::models::Priority::P2,
            status: TaskStatus::Draft,
            assignee_session_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            started_at: None,
            completed_at: None,
            canceled_at: None,
            cancel_reason: None,
            branch: None,
            metadata: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        let events = detect_events(&known_ids, &statuses, &[task], &config, &conn, &prev_ready_ids);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "task_added");
        assert!(events[0].unblocked_tasks.is_none());
    }

    #[test]
    fn detect_events_completed() {
        let (_dir, conn) = setup_db();
        let mut known_ids = HashSet::new();
        known_ids.insert(1);
        let mut statuses = HashMap::new();
        statuses.insert(1, TaskStatus::InProgress);
        let prev_ready_ids = HashSet::new();
        let config = Config {
            hooks: HooksConfig {
                on_task_added: None,
                on_task_completed: Some("echo".into()),
            },
        };
        let task = Task {
            id: 1,
            title: "Done".into(),
            background: None,
            description: None,
            plan: None,
            priority: crate::models::Priority::P2,
            status: TaskStatus::Completed,
            assignee_session_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            started_at: None,
            completed_at: None,
            canceled_at: None,
            cancel_reason: None,
            branch: None,
            metadata: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        let events = detect_events(&known_ids, &statuses, &[task], &config, &conn, &prev_ready_ids);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "task_completed");
        assert!(events[0].unblocked_tasks.is_some());
    }

    #[test]
    fn detect_events_no_change() {
        let (_dir, conn) = setup_db();
        let mut known_ids = HashSet::new();
        known_ids.insert(1);
        let mut statuses = HashMap::new();
        statuses.insert(1, TaskStatus::Todo);
        let prev_ready_ids = HashSet::new();
        let config = Config {
            hooks: HooksConfig {
                on_task_added: Some("echo".into()),
                on_task_completed: Some("echo".into()),
            },
        };
        let task = Task {
            id: 1,
            title: "Same".into(),
            background: None,
            description: None,
            plan: None,
            priority: crate::models::Priority::P2,
            status: TaskStatus::Todo,
            assignee_session_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            started_at: None,
            completed_at: None,
            canceled_at: None,
            cancel_reason: None,
            branch: None,
            metadata: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        let events = detect_events(&known_ids, &statuses, &[task], &config, &conn, &prev_ready_ids);
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn completed_event_has_unblocked_tasks() {
        let (_dir, conn) = setup_db();

        // Create task 1 (will be completed) and task 2 (depends on task 1)
        db::create_task(
            &conn,
            &crate::models::CreateTaskParams {
                title: "Dependency".into(),
                background: None,
                description: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                branch: None,
                metadata: None,
                tags: vec![],
                dependencies: vec![],
            },
        )
        .unwrap();
        let update_none = |status| crate::models::UpdateTaskParams {
            status: Some(status),
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
            branch: None,
            metadata: None,
        };
        db::update_task(&conn, 1, &update_none(TaskStatus::Todo)).unwrap();
        db::update_task(&conn, 1, &update_none(TaskStatus::InProgress)).unwrap();
        db::update_task(&conn, 1, &update_none(TaskStatus::Completed)).unwrap();

        db::create_task(
            &conn,
            &crate::models::CreateTaskParams {
                title: "Blocked".into(),
                background: None,
                description: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                branch: None,
                metadata: None,
                tags: vec![],
                dependencies: vec![],
            },
        )
        .unwrap();
        db::update_task(
            &conn,
            2,
            &crate::models::UpdateTaskParams {
                status: Some(TaskStatus::Todo),
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
                branch: None,
                metadata: None,
            },
        )
        .unwrap();
        db::add_dependency(&conn, 2, 1).unwrap();

        // Before completion, task 2 was not ready (prev_ready_ids is empty)
        let prev_ready_ids = HashSet::new();
        let mut known_ids = HashSet::new();
        known_ids.insert(1);
        let mut statuses = HashMap::new();
        statuses.insert(1, TaskStatus::InProgress);
        let config = Config {
            hooks: HooksConfig {
                on_task_added: None,
                on_task_completed: Some("echo".into()),
            },
        };

        let task1 = db::get_task(&conn, 1).unwrap();
        let events = detect_events(
            &known_ids,
            &statuses,
            &[task1],
            &config,
            &conn,
            &prev_ready_ids,
        );

        assert_eq!(events.len(), 1);
        let unblocked = events[0].unblocked_tasks.as_ref().unwrap();
        assert_eq!(unblocked.len(), 1);
        assert_eq!(unblocked[0].id, 2);
        assert_eq!(unblocked[0].title, "Blocked");
        assert!(unblocked[0].metadata.is_none());
    }

    #[test]
    fn unblocked_task_includes_metadata() {
        let (_dir, conn) = setup_db();

        // Create task 1 (will be completed)
        db::create_task(
            &conn,
            &crate::models::CreateTaskParams {
                title: "Dependency".into(),
                background: None,
                description: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                branch: None,
                metadata: None,
                tags: vec![],
                dependencies: vec![],
            },
        )
        .unwrap();
        let update_none = |status| crate::models::UpdateTaskParams {
            status: Some(status),
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
            branch: None,
            metadata: None,
        };
        db::update_task(&conn, 1, &update_none(TaskStatus::Todo)).unwrap();
        db::update_task(&conn, 1, &update_none(TaskStatus::InProgress)).unwrap();
        db::update_task(&conn, 1, &update_none(TaskStatus::Completed)).unwrap();

        // Create task 2 (depends on task 1) with metadata
        let meta = serde_json::json!({"env": "staging", "priority_override": true});
        db::create_task(
            &conn,
            &crate::models::CreateTaskParams {
                title: "Blocked with meta".into(),
                background: None,
                description: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                branch: None,
                metadata: Some(meta.clone()),
                tags: vec![],
                dependencies: vec![],
            },
        )
        .unwrap();
        db::update_task(
            &conn,
            2,
            &crate::models::UpdateTaskParams {
                status: Some(TaskStatus::Todo),
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
                branch: None,
                metadata: None,
            },
        )
        .unwrap();
        db::add_dependency(&conn, 2, 1).unwrap();

        let prev_ready_ids = HashSet::new();
        let mut known_ids = HashSet::new();
        known_ids.insert(1);
        let mut statuses = HashMap::new();
        statuses.insert(1, TaskStatus::InProgress);
        let config = Config {
            hooks: HooksConfig {
                on_task_added: None,
                on_task_completed: Some("echo".into()),
            },
        };

        let task1 = db::get_task(&conn, 1).unwrap();
        let events = detect_events(
            &known_ids,
            &statuses,
            &[task1],
            &config,
            &conn,
            &prev_ready_ids,
        );

        assert_eq!(events.len(), 1);
        let unblocked = events[0].unblocked_tasks.as_ref().unwrap();
        assert_eq!(unblocked.len(), 1);
        assert_eq!(unblocked[0].id, 2);
        assert_eq!(unblocked[0].metadata, Some(meta));

        // Verify metadata appears in JSON serialization
        let json = serde_json::to_string(&events[0]).unwrap();
        assert!(json.contains("\"metadata\""));
        assert!(json.contains("staging"));
    }

    #[test]
    fn write_and_read_daemon_info() {
        let dir = tempfile::tempdir().unwrap();
        let localflow_dir = dir.path().join(".localflow");
        std::fs::create_dir_all(&localflow_dir).unwrap();

        let info = DaemonInfo {
            pid: 12345,
            interval: 10,
            started_at: "2026-03-24T03:30:00+00:00".into(),
        };
        write_daemon_info(dir.path(), &info).unwrap();

        let read = read_daemon_info(dir.path()).unwrap().unwrap();
        assert_eq!(read.pid, 12345);
        assert_eq!(read.interval, 10);
        assert_eq!(read.started_at, "2026-03-24T03:30:00+00:00");
    }

    #[test]
    fn read_daemon_info_legacy_plain_pid() {
        let dir = tempfile::tempdir().unwrap();
        let localflow_dir = dir.path().join(".localflow");
        std::fs::create_dir_all(&localflow_dir).unwrap();
        std::fs::write(localflow_dir.join("watch.pid"), "99999").unwrap();

        let read = read_daemon_info(dir.path()).unwrap().unwrap();
        assert_eq!(read.pid, 99999);
        assert_eq!(read.interval, 0);
        assert!(read.started_at.is_empty());
    }

    #[test]
    fn read_daemon_info_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let read = read_daemon_info(dir.path()).unwrap();
        assert!(read.is_none());
    }

    #[test]
    fn daemon_status_stopped_when_no_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        // Just verify it doesn't error and outputs "stopped"
        let result = daemon_status(dir.path(), true);
        assert!(result.is_ok());
    }

    #[test]
    fn daemon_status_cleans_stale_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        let localflow_dir = dir.path().join(".localflow");
        std::fs::create_dir_all(&localflow_dir).unwrap();

        // Write a PID that doesn't exist (use PID 1999999999 which won't exist)
        let info = DaemonInfo {
            pid: 1999999999,
            interval: 5,
            started_at: "2026-03-24T03:30:00+00:00".into(),
        };
        write_daemon_info(dir.path(), &info).unwrap();

        let result = daemon_status(dir.path(), true);
        assert!(result.is_ok());

        // PID file should have been cleaned up
        assert!(!pid_file_path(dir.path()).exists());
    }

    #[test]
    fn added_event_no_unblocked_tasks() {
        let (_dir, conn) = setup_db();
        let known_ids = HashSet::new();
        let statuses = HashMap::new();
        let prev_ready_ids = HashSet::new();
        let config = Config {
            hooks: HooksConfig {
                on_task_added: Some("echo".into()),
                on_task_completed: None,
            },
        };
        let task = Task {
            id: 1,
            title: "New".into(),
            background: None,
            description: None,
            plan: None,
            priority: crate::models::Priority::P2,
            status: TaskStatus::Draft,
            assignee_session_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            started_at: None,
            completed_at: None,
            canceled_at: None,
            cancel_reason: None,
            branch: None,
            metadata: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        let events = detect_events(&known_ids, &statuses, &[task], &config, &conn, &prev_ready_ids);
        assert_eq!(events.len(), 1);
        assert!(events[0].unblocked_tasks.is_none());
    }
}
