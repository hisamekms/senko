use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use chrono::Utc;
use uuid::Uuid;

use crate::db::TaskBackend;
use crate::models::{Priority, Task, TaskStatus};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub hooks: HooksConfig,
    #[serde(default)]
    pub workflow: WorkflowConfig,
    #[serde(default)]
    pub backend: BackendConfig,
    #[serde(default)]
    pub log: LogConfig,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct LogConfig {
    pub dir: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct BackendConfig {
    pub api_url: Option<String>,
    #[serde(default)]
    pub hook_mode: HookMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HookMode {
    #[default]
    Server,
    Client,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionMode {
    MergeThenComplete,
    PrThenComplete,
}

impl Default for CompletionMode {
    fn default() -> Self {
        CompletionMode::MergeThenComplete
    }
}

impl std::fmt::Display for CompletionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompletionMode::MergeThenComplete => write!(f, "merge_then_complete"),
            CompletionMode::PrThenComplete => write!(f, "pr_then_complete"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkflowConfig {
    #[serde(default)]
    pub completion_mode: CompletionMode,
    #[serde(default = "default_true")]
    pub auto_merge: bool,
}

fn default_true() -> bool {
    true
}

impl Default for WorkflowConfig {
    fn default() -> Self {
        Self {
            completion_mode: CompletionMode::default(),
            auto_merge: true,
        }
    }
}

mod string_or_vec {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum StringOrVec {
            String(String),
            Vec(Vec<String>),
        }
        match StringOrVec::deserialize(deserializer)? {
            StringOrVec::String(s) => Ok(vec![s]),
            StringOrVec::Vec(v) => Ok(v),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    #[serde(default, deserialize_with = "string_or_vec::deserialize")]
    pub on_task_added: Vec<String>,
    #[serde(default, deserialize_with = "string_or_vec::deserialize")]
    pub on_task_ready: Vec<String>,
    #[serde(default, deserialize_with = "string_or_vec::deserialize")]
    pub on_task_started: Vec<String>,
    #[serde(default, deserialize_with = "string_or_vec::deserialize")]
    pub on_task_completed: Vec<String>,
    #[serde(default, deserialize_with = "string_or_vec::deserialize")]
    pub on_task_canceled: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct UnblockedTask {
    pub id: i64,
    pub title: String,
    pub priority: Priority,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct HookEvent {
    pub event_id: String,
    pub event: String,
    pub timestamp: String,
    pub task: Task,
    pub stats: HashMap<String, i64>,
    pub ready_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unblocked_tasks: Option<Vec<UnblockedTask>>,
}

pub fn load_config(project_root: &Path) -> Result<Config> {
    let config_path = project_root.join(".localflow").join("config.toml");
    let config = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)
            .context("failed to read .localflow/config.toml")?;
        toml::from_str(&content).context("failed to parse config.toml")?
    } else {
        Config::default()
    };
    Ok(apply_env_overrides(config))
}

fn apply_env_overrides(mut config: Config) -> Config {
    // Workflow settings
    if let Ok(val) = std::env::var("LOCALFLOW_COMPLETION_MODE") {
        match val.as_str() {
            "merge_then_complete" => {
                config.workflow.completion_mode = CompletionMode::MergeThenComplete
            }
            "pr_then_complete" => {
                config.workflow.completion_mode = CompletionMode::PrThenComplete
            }
            other => eprintln!("warning: unknown LOCALFLOW_COMPLETION_MODE={other}, ignoring"),
        }
    }
    if let Ok(val) = std::env::var("LOCALFLOW_AUTO_MERGE") {
        match val.to_lowercase().as_str() {
            "true" | "1" | "yes" => config.workflow.auto_merge = true,
            "false" | "0" | "no" => config.workflow.auto_merge = false,
            other => eprintln!("warning: unknown LOCALFLOW_AUTO_MERGE={other}, ignoring"),
        }
    }

    // Backend settings
    if let Ok(val) = std::env::var("LOCALFLOW_API_URL") {
        if !val.is_empty() {
            config.backend.api_url = Some(val);
        }
    }
    if let Ok(val) = std::env::var("LOCALFLOW_HOOK_MODE") {
        match val.to_lowercase().as_str() {
            "server" => config.backend.hook_mode = HookMode::Server,
            "client" => config.backend.hook_mode = HookMode::Client,
            "both" => config.backend.hook_mode = HookMode::Both,
            other => eprintln!("warning: unknown LOCALFLOW_HOOK_MODE={other}, ignoring"),
        }
    }

    // Hook commands (append to existing config.toml entries)
    if let Ok(val) = std::env::var("LOCALFLOW_HOOK_ON_TASK_ADDED") {
        if !val.is_empty() {
            config.hooks.on_task_added.push(val);
        }
    }
    if let Ok(val) = std::env::var("LOCALFLOW_HOOK_ON_TASK_READY") {
        if !val.is_empty() {
            config.hooks.on_task_ready.push(val);
        }
    }
    if let Ok(val) = std::env::var("LOCALFLOW_HOOK_ON_TASK_STARTED") {
        if !val.is_empty() {
            config.hooks.on_task_started.push(val);
        }
    }
    if let Ok(val) = std::env::var("LOCALFLOW_HOOK_ON_TASK_COMPLETED") {
        if !val.is_empty() {
            config.hooks.on_task_completed.push(val);
        }
    }
    if let Ok(val) = std::env::var("LOCALFLOW_HOOK_ON_TASK_CANCELED") {
        if !val.is_empty() {
            config.hooks.on_task_canceled.push(val);
        }
    }

    // Log settings
    if let Ok(val) = std::env::var("LOCALFLOW_LOG_DIR") {
        if !val.is_empty() {
            config.log.dir = Some(val);
        }
    }

    config
}

pub fn build_event(
    event_name: &str,
    task: &Task,
    backend: &dyn TaskBackend,
    from_status: Option<TaskStatus>,
    unblocked: Option<Vec<UnblockedTask>>,
) -> HookEvent {
    let stats = backend.task_stats().unwrap_or_default();
    let ready_count = backend.ready_count().unwrap_or(0);
    HookEvent {
        event_id: Uuid::new_v4().to_string(),
        event: event_name.into(),
        timestamp: Utc::now().to_rfc3339(),
        task: task.clone(),
        stats,
        ready_count,
        from_status: from_status.map(|s| s.to_string()),
        unblocked_tasks: unblocked,
    }
}

/// Return the hook log file path, optionally using a custom log directory.
/// Priority: `log_dir` override > `$XDG_STATE_HOME/localflow` > `~/.local/state/localflow`
pub fn log_file_path_with_dir(log_dir: Option<&str>) -> Option<PathBuf> {
    let dir = if let Some(d) = log_dir {
        PathBuf::from(d)
    } else {
        let state_dir = std::env::var("XDG_STATE_HOME")
            .map(PathBuf::from)
            .ok()
            .filter(|p| p.is_absolute())
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| PathBuf::from(h).join(".local").join("state"))
            })?;
        state_dir.join("localflow")
    };
    Some(dir.join("hooks.log"))
}

/// Return the hook log file path following XDG Base Directory specification.
/// `$XDG_STATE_HOME/localflow/hooks.log` (default: `~/.local/state/localflow/hooks.log`)
pub fn log_file_path() -> Option<PathBuf> {
    log_file_path_with_dir(None)
}

fn log_to_file(path: &Path, level: &str, message: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        // Build the full line in a single buffer so the O_APPEND write is atomic.
        let ts = Utc::now().to_rfc3339();
        let line = format!("[{}] [{}] {}\n", ts, level, message);
        let _ = f.write_all(line.as_bytes());
    }
}

fn execute_hook(command: &str, event_name: &str, json: &str, log_path: Option<&Path>) {
    let mut child = match std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("hook spawn error ({}): {}: {:#}", event_name, command, e);
            eprintln!("{msg}");
            if let Some(p) = log_path {
                log_to_file(p, "ERROR", &msg);
            }
            return;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(json.as_bytes()) {
            let msg = format!("hook stdin error ({}): {}: {:#}", event_name, command, e);
            eprintln!("{msg}");
            if let Some(p) = log_path {
                log_to_file(p, "ERROR", &msg);
            }
            return;
        }
    }

    // Spawn a thread to wait for exit and log the result.
    // The CLI returns immediately; the thread outlives the main function
    // but Rust waits for non-daemon threads before process exit.
    let cmd = command.to_owned();
    let event = event_name.to_owned();
    let log = log_path.map(|p| p.to_owned());
    std::thread::spawn(move || {
        match child.wait() {
            Ok(status) if status.success() => {
                if let Some(p) = log {
                    log_to_file(&p, "INFO", &format!("hook ok ({}): {} (exit: {})", event, cmd, status));
                }
            }
            Ok(status) => {
                let msg = format!("hook failed ({}): {} (exit: {})", event, cmd, status);
                eprintln!("{msg}");
                if let Some(p) = log {
                    log_to_file(&p, "WARN", &msg);
                }
            }
            Err(e) => {
                let msg = format!("hook wait error ({}): {}: {:#}", event, cmd, e);
                eprintln!("{msg}");
                if let Some(p) = log {
                    log_to_file(&p, "ERROR", &msg);
                }
            }
        }
    });
}

/// Fire hooks for the given event, spawning each hook command as a
/// fire-and-forget child process. Returns immediately.
/// Results are logged to `$XDG_STATE_HOME/localflow/hooks.log`.
pub fn fire_hooks(
    config: &Config,
    event_name: &str,
    task: &Task,
    backend: &dyn TaskBackend,
    from_status: Option<TaskStatus>,
    unblocked: Option<Vec<UnblockedTask>>,
) {
    let commands = match event_name {
        "task_added" => &config.hooks.on_task_added,
        "task_ready" => &config.hooks.on_task_ready,
        "task_started" => &config.hooks.on_task_started,
        "task_completed" => &config.hooks.on_task_completed,
        "task_canceled" => &config.hooks.on_task_canceled,
        _ => return,
    };
    if commands.is_empty() {
        return;
    }

    let log_path = log_file_path_with_dir(config.log.dir.as_deref());

    let event = build_event(event_name, task, backend, from_status, unblocked);
    let json = match serde_json::to_string(&event) {
        Ok(j) => j,
        Err(e) => {
            let msg = format!("hook error: failed to serialize event: {e}");
            eprintln!("{msg}");
            if let Some(ref p) = log_path {
                log_to_file(p, "ERROR", &msg);
            }
            return;
        }
    };

    for cmd in commands {
        execute_hook(cmd, event_name, &json, log_path.as_deref());
    }
}

/// Return the hook commands configured for the given event name.
/// Returns `None` if the event name is not recognized.
pub fn get_commands_for_event<'a>(config: &'a Config, event_name: &str) -> Option<&'a Vec<String>> {
    match event_name {
        "task_added" => Some(&config.hooks.on_task_added),
        "task_ready" => Some(&config.hooks.on_task_ready),
        "task_started" => Some(&config.hooks.on_task_started),
        "task_completed" => Some(&config.hooks.on_task_completed),
        "task_canceled" => Some(&config.hooks.on_task_canceled),
        _ => None,
    }
}

/// Execute a hook command synchronously, inheriting stdout/stderr.
/// Returns the exit status of the child process.
pub fn execute_hook_sync(command: &str, json: &str) -> Result<std::process::ExitStatus> {
    let mut child = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .with_context(|| format!("failed to spawn hook: {command}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(json.as_bytes())
            .with_context(|| format!("failed to write to hook stdin: {command}"))?;
    }

    child
        .wait()
        .with_context(|| format!("failed to wait for hook: {command}"))
}

/// Compute newly unblocked tasks after a task completion.
/// Call this after `db::complete_task` with the set of ready task IDs
/// captured before the completion.
pub fn compute_unblocked(
    backend: &dyn TaskBackend,
    prev_ready_ids: &std::collections::HashSet<i64>,
) -> Vec<UnblockedTask> {
    let curr_ready = backend.list_ready_tasks().unwrap_or_default();
    curr_ready
        .iter()
        .filter(|t| !prev_ready_ids.contains(&t.id))
        .map(|t| UnblockedTask {
            id: t.id,
            title: t.title.clone(),
            priority: t.priority,
            metadata: t.metadata.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::SqliteBackend;

    fn setup_db() -> (tempfile::TempDir, SqliteBackend) {
        let dir = tempfile::tempdir().unwrap();
        let backend = SqliteBackend::new(dir.path()).unwrap();
        (dir, backend)
    }

    #[test]
    fn load_config_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = load_config(dir.path()).unwrap();
        assert!(config.hooks.on_task_added.is_empty());
        assert!(config.hooks.on_task_completed.is_empty());
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
        assert_eq!(config.hooks.on_task_added, vec!["echo added"]);
        assert_eq!(config.hooks.on_task_completed, vec!["echo completed"]);
    }

    #[test]
    fn load_config_empty_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let localflow_dir = dir.path().join(".localflow");
        std::fs::create_dir_all(&localflow_dir).unwrap();
        std::fs::write(localflow_dir.join("config.toml"), "[hooks]\n").unwrap();

        let config = load_config(dir.path()).unwrap();
        assert!(config.hooks.on_task_added.is_empty());
        assert!(config.hooks.on_task_completed.is_empty());
    }

    #[test]
    fn hook_event_serialization() {
        let (_dir, backend) = setup_db();
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
            pr_url: None,
            metadata: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        let event = build_event("task_added", &task, &backend, None, None);
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
        let (_dir, backend) = setup_db();
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
            pr_url: None,
            metadata: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        let event = build_event("task_added", &task, &backend, None, None);
        assert!(Uuid::parse_str(&event.event_id).is_ok());
        assert!(chrono::DateTime::parse_from_rfc3339(&event.timestamp).is_ok());
    }

    #[test]
    fn event_has_stats() {
        let (_dir, backend) = setup_db();
        backend.create_task(
            &crate::models::CreateTaskParams {
                title: "Task1".into(),
                background: None,
                description: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                branch: None,
                pr_url: None,
                metadata: None,
                tags: vec![],
                dependencies: vec![],
            },
        )
        .unwrap();
        let task = backend.get_task(1).unwrap();
        let event = build_event("task_added", &task, &backend, None, None);
        assert!(event.stats.contains_key("draft"));
        assert_eq!(*event.stats.get("draft").unwrap(), 1);
    }

    #[test]
    fn event_has_ready_count() {
        let (_dir, backend) = setup_db();
        backend.create_task(
            &crate::models::CreateTaskParams {
                title: "Ready".into(),
                background: None,
                description: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                branch: None,
                pr_url: None,
                metadata: None,
                tags: vec![],
                dependencies: vec![],
            },
        )
        .unwrap();
        backend.ready_task(1).unwrap();
        let task = backend.get_task(1).unwrap();
        let event = build_event("task_added", &task, &backend, None, None);
        assert_eq!(event.ready_count, 1);
    }

    #[test]
    fn compute_unblocked_finds_newly_ready() {
        let (_dir, backend) = setup_db();

        // Create task 1 (will be completed) and task 2 (depends on task 1)
        backend.create_task(
            &crate::models::CreateTaskParams {
                title: "Dependency".into(),
                background: None,
                description: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                branch: None,
                pr_url: None,
                metadata: None,
                tags: vec![],
                dependencies: vec![],
            },
        )
        .unwrap();
        backend.ready_task(1).unwrap();
        backend.start_task(1, None, "2025-01-01T00:00:00Z").unwrap();

        backend.create_task(
            &crate::models::CreateTaskParams {
                title: "Blocked".into(),
                background: None,
                description: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                branch: None,
                pr_url: None,
                metadata: None,
                tags: vec![],
                dependencies: vec![],
            },
        )
        .unwrap();
        backend.ready_task(2).unwrap();
        backend.add_dependency(2, 1).unwrap();

        // Capture ready tasks before completion
        let prev_ready: std::collections::HashSet<i64> =
            backend.list_ready_tasks().unwrap().iter().map(|t| t.id).collect();

        // Complete task 1
        backend.complete_task(1, "2025-01-01T00:00:00Z").unwrap();

        let unblocked = compute_unblocked(&backend, &prev_ready);
        assert_eq!(unblocked.len(), 1);
        assert_eq!(unblocked[0].id, 2);
        assert_eq!(unblocked[0].title, "Blocked");
    }

    #[test]
    fn fire_hooks_executes_multiple_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let marker1 = dir.path().join("hook1.txt");
        let marker2 = dir.path().join("hook2.txt");
        let cmd1 = format!("echo hook1 > {}", marker1.display());
        let cmd2 = format!("echo hook2 > {}", marker2.display());

        let config = Config {
            hooks: HooksConfig {
                on_task_added: vec![cmd1, cmd2],
                on_task_completed: vec![],
                ..Default::default()
            },
            ..Default::default()
        };

        let (_db_dir, backend) = setup_db();
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
            pr_url: None,
            metadata: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        fire_hooks(&config, "task_added", &task, &backend, None, None);

        // Give child processes a moment to complete
        std::thread::sleep(std::time::Duration::from_millis(200));

        assert!(marker1.exists(), "first hook should have run");
        assert!(marker2.exists(), "second hook should have run");
    }

    #[test]
    fn fire_hooks_noop_when_no_commands() {
        let (_db_dir, backend) = setup_db();
        let config = Config::default();
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
            pr_url: None,
            metadata: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        // Should not panic
        fire_hooks(&config, "task_added", &task, &backend, None, None);
    }

    #[test]
    fn log_file_path_uses_xdg_state_home() {
        unsafe {
            let orig = std::env::var("XDG_STATE_HOME").ok();
            std::env::set_var("XDG_STATE_HOME", "/tmp/test-xdg-state");
            let path = log_file_path().unwrap();
            assert_eq!(
                path,
                PathBuf::from("/tmp/test-xdg-state/localflow/hooks.log")
            );
            match orig {
                Some(v) => std::env::set_var("XDG_STATE_HOME", v),
                None => std::env::remove_var("XDG_STATE_HOME"),
            }
        }
    }

    #[test]
    fn log_file_path_falls_back_to_home() {
        unsafe {
            let orig_xdg = std::env::var("XDG_STATE_HOME").ok();
            let orig_home = std::env::var("HOME").ok();
            std::env::remove_var("XDG_STATE_HOME");
            std::env::set_var("HOME", "/tmp/test-home");
            let path = log_file_path().unwrap();
            assert_eq!(
                path,
                PathBuf::from("/tmp/test-home/.local/state/localflow/hooks.log")
            );
            match orig_xdg {
                Some(v) => std::env::set_var("XDG_STATE_HOME", v),
                None => {}
            }
            match orig_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[test]
    fn log_to_file_creates_and_appends() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("nested").join("hooks.log");
        log_to_file(&log_path, "INFO", "first message");
        log_to_file(&log_path, "WARN", "second message");
        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("[INFO] first message"));
        assert!(content.contains("[WARN] second message"));
    }

    #[test]
    fn hook_failure_logged_to_file() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("hooks.log");

        // Run a hook that exits with non-zero
        let config = Config {
            hooks: HooksConfig {
                on_task_added: vec!["exit 1".into()],
                ..Default::default()
            },
            ..Default::default()
        };

        let (_db_dir, backend) = setup_db();
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
            pr_url: None,
            metadata: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };

        // Call execute_hook directly with our log path
        let json = serde_json::to_string(&build_event("task_added", &task, &backend, None, None)).unwrap();
        execute_hook("exit 1", "task_added", &json, Some(&log_path));

        // Wait for the thread to finish logging
        std::thread::sleep(std::time::Duration::from_millis(300));

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("[WARN]"), "should log failure: {content}");
        assert!(content.contains("hook failed"), "should contain hook failed: {content}");
    }

    #[test]
    fn parse_hooks_string_value() {
        let toml_str = r#"
[hooks]
on_task_added = "echo added"
on_task_completed = "echo completed"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.hooks.on_task_added, vec!["echo added"]);
        assert_eq!(config.hooks.on_task_completed, vec!["echo completed"]);
    }

    #[test]
    fn parse_hooks_array_value() {
        let toml_str = r#"
[hooks]
on_task_added = ["echo first", "echo second"]
on_task_completed = ["notify", "log"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.hooks.on_task_added,
            vec!["echo first", "echo second"]
        );
        assert_eq!(config.hooks.on_task_completed, vec!["notify", "log"]);
    }

    #[test]
    fn parse_hooks_missing_fields() {
        let toml_str = "[hooks]\n";
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.hooks.on_task_added.is_empty());
        assert!(config.hooks.on_task_completed.is_empty());
    }

    #[test]
    fn hook_receives_json_on_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let output_file = dir.path().join("stdin_capture.json");
        let cmd = format!("cat > {}", output_file.display());

        let config = Config {
            hooks: HooksConfig {
                on_task_added: vec![cmd],
                ..Default::default()
            },
            ..Default::default()
        };

        let (_db_dir, backend) = setup_db();
        let task = Task {
            id: 42,
            title: "Hook stdin test".into(),
            background: None,
            description: None,
            plan: None,
            priority: crate::models::Priority::P1,
            status: TaskStatus::Draft,
            assignee_session_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            started_at: None,
            completed_at: None,
            canceled_at: None,
            cancel_reason: None,
            branch: None,
            pr_url: None,
            metadata: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        fire_hooks(&config, "task_added", &task, &backend, None, None);

        std::thread::sleep(std::time::Duration::from_millis(200));

        let content = std::fs::read_to_string(&output_file).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(json["event"], "task_added");
        assert_eq!(json["task"]["id"], 42);
        assert_eq!(json["task"]["title"], "Hook stdin test");
    }

    #[test]
    fn env_override_completion_mode() {
        unsafe {
            let orig = std::env::var("LOCALFLOW_COMPLETION_MODE").ok();
            std::env::set_var("LOCALFLOW_COMPLETION_MODE", "pr_then_complete");
            let config = apply_env_overrides(Config::default());
            assert_eq!(config.workflow.completion_mode, CompletionMode::PrThenComplete);
            match orig {
                Some(v) => std::env::set_var("LOCALFLOW_COMPLETION_MODE", v),
                None => std::env::remove_var("LOCALFLOW_COMPLETION_MODE"),
            }
        }
    }

    #[test]
    fn env_override_auto_merge() {
        unsafe {
            let orig = std::env::var("LOCALFLOW_AUTO_MERGE").ok();
            std::env::set_var("LOCALFLOW_AUTO_MERGE", "false");
            let config = apply_env_overrides(Config::default());
            assert!(!config.workflow.auto_merge);
            std::env::set_var("LOCALFLOW_AUTO_MERGE", "0");
            let config = apply_env_overrides(Config::default());
            assert!(!config.workflow.auto_merge);
            match orig {
                Some(v) => std::env::set_var("LOCALFLOW_AUTO_MERGE", v),
                None => std::env::remove_var("LOCALFLOW_AUTO_MERGE"),
            }
        }
    }

    #[test]
    fn env_override_hook_mode() {
        unsafe {
            let orig = std::env::var("LOCALFLOW_HOOK_MODE").ok();
            std::env::set_var("LOCALFLOW_HOOK_MODE", "client");
            let config = apply_env_overrides(Config::default());
            assert_eq!(config.backend.hook_mode, HookMode::Client);
            std::env::set_var("LOCALFLOW_HOOK_MODE", "both");
            let config = apply_env_overrides(Config::default());
            assert_eq!(config.backend.hook_mode, HookMode::Both);
            match orig {
                Some(v) => std::env::set_var("LOCALFLOW_HOOK_MODE", v),
                None => std::env::remove_var("LOCALFLOW_HOOK_MODE"),
            }
        }
    }

    #[test]
    fn env_override_api_url() {
        unsafe {
            let orig = std::env::var("LOCALFLOW_API_URL").ok();
            std::env::set_var("LOCALFLOW_API_URL", "http://remote:3142");
            let config = apply_env_overrides(Config::default());
            assert_eq!(config.backend.api_url, Some("http://remote:3142".to_string()));
            match orig {
                Some(v) => std::env::set_var("LOCALFLOW_API_URL", v),
                None => std::env::remove_var("LOCALFLOW_API_URL"),
            }
        }
    }

    #[test]
    fn env_override_hooks_append() {
        unsafe {
            let orig = std::env::var("LOCALFLOW_HOOK_ON_TASK_ADDED").ok();
            std::env::set_var("LOCALFLOW_HOOK_ON_TASK_ADDED", "env-hook");
            // Start with a config that already has a hook from config.toml
            let mut config = Config::default();
            config.hooks.on_task_added = vec!["toml-hook".into()];
            let config = apply_env_overrides(config);
            assert_eq!(config.hooks.on_task_added, vec!["toml-hook", "env-hook"]);
            match orig {
                Some(v) => std::env::set_var("LOCALFLOW_HOOK_ON_TASK_ADDED", v),
                None => std::env::remove_var("LOCALFLOW_HOOK_ON_TASK_ADDED"),
            }
        }
    }

    #[test]
    fn env_override_empty_values_ignored() {
        unsafe {
            let orig_url = std::env::var("LOCALFLOW_API_URL").ok();
            let orig_hook = std::env::var("LOCALFLOW_HOOK_ON_TASK_ADDED").ok();
            std::env::set_var("LOCALFLOW_API_URL", "");
            std::env::set_var("LOCALFLOW_HOOK_ON_TASK_ADDED", "");
            let config = apply_env_overrides(Config::default());
            assert_eq!(config.backend.api_url, None);
            assert!(config.hooks.on_task_added.is_empty());
            match orig_url {
                Some(v) => std::env::set_var("LOCALFLOW_API_URL", v),
                None => std::env::remove_var("LOCALFLOW_API_URL"),
            }
            match orig_hook {
                Some(v) => std::env::set_var("LOCALFLOW_HOOK_ON_TASK_ADDED", v),
                None => std::env::remove_var("LOCALFLOW_HOOK_ON_TASK_ADDED"),
            }
        }
    }

    #[test]
    fn load_config_no_file_with_env_overrides() {
        unsafe {
            let orig = std::env::var("LOCALFLOW_COMPLETION_MODE").ok();
            std::env::set_var("LOCALFLOW_COMPLETION_MODE", "pr_then_complete");
            let tmp = tempfile::tempdir().unwrap();
            let config = load_config(tmp.path()).unwrap();
            assert_eq!(config.workflow.completion_mode, CompletionMode::PrThenComplete);
            match orig {
                Some(v) => std::env::set_var("LOCALFLOW_COMPLETION_MODE", v),
                None => std::env::remove_var("LOCALFLOW_COMPLETION_MODE"),
            }
        }
    }

    #[test]
    fn log_file_path_with_custom_dir() {
        let path = log_file_path_with_dir(Some("/custom/log/dir")).unwrap();
        assert_eq!(path, PathBuf::from("/custom/log/dir/hooks.log"));
    }

    #[test]
    fn log_file_path_with_dir_none_uses_default() {
        // When None is passed, it should behave like the original log_file_path()
        let with_dir = log_file_path_with_dir(None);
        let original = log_file_path();
        assert_eq!(with_dir, original);
    }

    #[test]
    fn env_override_log_dir() {
        unsafe {
            let orig = std::env::var("LOCALFLOW_LOG_DIR").ok();
            std::env::set_var("LOCALFLOW_LOG_DIR", "/tmp/custom-logs");
            let config = apply_env_overrides(Config::default());
            assert_eq!(config.log.dir, Some("/tmp/custom-logs".into()));
            match orig {
                Some(v) => std::env::set_var("LOCALFLOW_LOG_DIR", v),
                None => std::env::remove_var("LOCALFLOW_LOG_DIR"),
            }
        }
    }

    #[test]
    fn env_override_log_dir_empty_ignored() {
        unsafe {
            let orig = std::env::var("LOCALFLOW_LOG_DIR").ok();
            std::env::set_var("LOCALFLOW_LOG_DIR", "");
            let config = apply_env_overrides(Config::default());
            assert_eq!(config.log.dir, None);
            match orig {
                Some(v) => std::env::set_var("LOCALFLOW_LOG_DIR", v),
                None => std::env::remove_var("LOCALFLOW_LOG_DIR"),
            }
        }
    }

    #[test]
    fn log_config_deserialization() {
        let toml_str = r#"
[log]
dir = "/var/log/localflow"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.log.dir, Some("/var/log/localflow".into()));
    }

    #[test]
    fn log_config_deserialization_missing_section() {
        let toml_str = r#"
[hooks]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.log.dir, None);
    }
}
