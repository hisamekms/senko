use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::db;
use crate::models::{ListTasksFilter, Task, TaskStatus};

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

#[derive(Debug, Serialize)]
pub struct WatchEvent {
    pub event: String,
    pub task: Task,
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

/// Detect events by comparing previous state with current tasks.
fn detect_events(
    known_ids: &HashSet<i64>,
    statuses: &HashMap<i64, TaskStatus>,
    tasks: &[Task],
    config: &Config,
) -> Vec<WatchEvent> {
    let mut events = Vec::new();
    for task in tasks {
        if !known_ids.contains(&task.id) {
            if config.hooks.on_task_added.is_some() {
                events.push(WatchEvent {
                    event: "task_added".into(),
                    task: task.clone(),
                });
            }
        }
        if task.status == TaskStatus::Completed {
            if let Some(prev) = statuses.get(&task.id) {
                if *prev != TaskStatus::Completed && config.hooks.on_task_completed.is_some() {
                    events.push(WatchEvent {
                        event: "task_completed".into(),
                        task: task.clone(),
                    });
                }
            }
        }
    }
    events
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
                    let events = detect_events(&known_ids, &statuses, &tasks, &config);

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
    if pid_path.exists() {
        let pid_str = std::fs::read_to_string(&pid_path)?;
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            if is_process_alive(pid) {
                bail!("watch daemon already running (PID {})", pid);
            }
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
    std::fs::write(&pid_path, pid.to_string())?;
    eprintln!("Watch daemon started (PID {})", pid);
    Ok(())
}

pub fn stop_daemon(project_root: &Path) -> Result<()> {
    let pid_path = pid_file_path(project_root);
    if !pid_path.exists() {
        bail!("no watch daemon running (PID file not found)");
    }
    let pid_str = std::fs::read_to_string(&pid_path)?;
    let pid: u32 = pid_str
        .trim()
        .parse()
        .context("invalid PID in watch.pid")?;

    let status = std::process::Command::new("kill")
        .arg(pid.to_string())
        .status()
        .context("failed to send signal to daemon")?;

    std::fs::remove_file(&pid_path)?;

    if status.success() {
        eprintln!("Watch daemon stopped (PID {})", pid);
    } else {
        eprintln!(
            "Watch daemon (PID {}) may have already exited, PID file removed",
            pid
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        let event = WatchEvent {
            event: "task_added".into(),
            task,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"task_added\""));
        assert!(json.contains("\"id\":1"));
    }

    #[test]
    fn detect_events_new_task() {
        let known_ids = HashSet::new();
        let statuses = HashMap::new();
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
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        let events = detect_events(&known_ids, &statuses, &[task], &config);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "task_added");
    }

    #[test]
    fn detect_events_completed() {
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
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        let events = detect_events(&known_ids, &statuses, &[task], &config);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "task_completed");
    }

    #[test]
    fn detect_events_no_change() {
        let mut known_ids = HashSet::new();
        known_ids.insert(1);
        let mut statuses = HashMap::new();
        statuses.insert(1, TaskStatus::Todo);
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
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        let events = detect_events(&known_ids, &statuses, &[task], &config);
        assert_eq!(events.len(), 0);
    }
}
