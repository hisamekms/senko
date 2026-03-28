pub mod executor;

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use chrono::Utc;
use uuid::Uuid;

use crate::domain::config::{
    CompletionMode, Config, HookEntry, HookMode, HooksConfig, LogFormat, RawConfig,
    RawLogConfig, RawWorkflowConfig,
};
use crate::domain::repository::TaskBackend;
use crate::domain::task::{Task, TaskStatus, UnblockedTask};

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

#[derive(Debug, Serialize)]
pub struct NoEligibleTaskEvent {
    pub event_id: String,
    pub event: String,
    pub timestamp: String,
    pub stats: HashMap<String, i64>,
    pub ready_count: i64,
}

pub fn load_config(project_root: &Path, explicit_config: Option<&Path>) -> Result<Config> {
    // 1. Load user config (lowest priority layer)
    let user_raw = load_user_config()?;

    // 2. Determine and load the project/explicit config
    let project_raw = if let Some(path) = explicit_config {
        // Explicit --config flag: must exist
        Some(load_config_file(path, true)?)
    } else if let Some(env_path) = env_config_path() {
        // LOCALFLOW_CONFIG env var: must exist
        Some(load_config_file(&env_path, true)?)
    } else {
        let default_path = project_root.join(".localflow").join("config.toml");
        if default_path.exists() {
            Some(load_config_file(&default_path, false)?)
        } else {
            None
        }
    };

    // 3. Merge: user config as base, project config as overlay
    let merged_raw = match (user_raw, project_raw) {
        (Some(base), Some(overlay)) => base.merge(overlay),
        (None, Some(overlay)) => overlay,
        (Some(base), None) => base,
        (None, None) => RawConfig::default(),
    };

    // 4. Resolve to final Config and apply env overrides
    let config = merged_raw.resolve();
    Ok(apply_env_overrides(config))
}

/// Return the user-level config path.
/// `$XDG_CONFIG_HOME/localflow/config.toml` or `~/.config/localflow/config.toml`
fn user_config_path() -> Option<PathBuf> {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .ok()
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".config"))
        })?;
    Some(config_dir.join("localflow").join("config.toml"))
}

/// Load user-level config if it exists.
fn load_user_config() -> Result<Option<RawConfig>> {
    let path = match user_config_path() {
        Some(p) if p.exists() => p,
        _ => return Ok(None),
    };
    let raw = load_config_file(&path, false)?;
    Ok(Some(raw))
}

/// Return the config path from the LOCALFLOW_CONFIG env var, if set.
fn env_config_path() -> Option<PathBuf> {
    std::env::var("LOCALFLOW_CONFIG")
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Load and parse a config file into RawConfig, with legacy hook format detection.
fn load_config_file(path: &Path, must_exist: bool) -> Result<RawConfig> {
    if !path.exists() {
        if must_exist {
            bail!("config file not found: {}", path.display());
        }
        return Ok(RawConfig::default());
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file: {}", path.display()))?;
    detect_legacy_hook_format(&content, path)?;
    toml::from_str(&content)
        .with_context(|| format!("failed to parse config file: {}", path.display()))
}

/// Check if the config uses the old array-based hook format and return a helpful error.
fn detect_legacy_hook_format(content: &str, path: &Path) -> Result<()> {
    let raw: toml::Value = match toml::from_str(content) {
        Ok(v) => v,
        Err(_) => return Ok(()), // let the real parser produce the error
    };
    if let Some(hooks) = raw.get("hooks").and_then(|v| v.as_table()) {
        for (key, val) in hooks {
            if val.is_str() || val.is_array() {
                bail!(
                    "Legacy hook format detected in {}.\n\
                     The array-based hook format is no longer supported.\n\
                     Please migrate to named hooks:\n\n\
                     Old format:\n  [hooks]\n  {} = \"command\"\n\n\
                     New format:\n  [hooks.{}.my-hook]\n  command = \"command\"\n",
                    path.display(),
                    key,
                    key,
                );
            }
        }
    }
    Ok(())
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

    // Hook commands (insert as named "_env" entry)
    fn insert_env_hook(map: &mut std::collections::BTreeMap<String, HookEntry>, val: String) {
        map.insert("_env".to_string(), HookEntry { command: val, enabled: true });
    }
    if let Ok(val) = std::env::var("LOCALFLOW_HOOK_ON_TASK_ADDED") {
        if !val.is_empty() {
            insert_env_hook(&mut config.hooks.on_task_added, val);
        }
    }
    if let Ok(val) = std::env::var("LOCALFLOW_HOOK_ON_TASK_READY") {
        if !val.is_empty() {
            insert_env_hook(&mut config.hooks.on_task_ready, val);
        }
    }
    if let Ok(val) = std::env::var("LOCALFLOW_HOOK_ON_TASK_STARTED") {
        if !val.is_empty() {
            insert_env_hook(&mut config.hooks.on_task_started, val);
        }
    }
    if let Ok(val) = std::env::var("LOCALFLOW_HOOK_ON_TASK_COMPLETED") {
        if !val.is_empty() {
            insert_env_hook(&mut config.hooks.on_task_completed, val);
        }
    }
    if let Ok(val) = std::env::var("LOCALFLOW_HOOK_ON_TASK_CANCELED") {
        if !val.is_empty() {
            insert_env_hook(&mut config.hooks.on_task_canceled, val);
        }
    }
    if let Ok(val) = std::env::var("LOCALFLOW_HOOK_ON_NO_ELIGIBLE_TASK") {
        if !val.is_empty() {
            insert_env_hook(&mut config.hooks.on_no_eligible_task, val);
        }
    }

    // User settings
    if let Ok(val) = std::env::var("LOCALFLOW_USER") {
        if !val.is_empty() {
            config.user.name = Some(val);
        }
    }

    // Log settings
    if let Ok(val) = std::env::var("LOCALFLOW_LOG_DIR") {
        if !val.is_empty() {
            config.log.dir = Some(val);
        }
    }
    if let Ok(val) = std::env::var("LOCALFLOW_LOG_LEVEL") {
        if !val.is_empty() {
            config.log.level = val;
        }
    }
    if let Ok(val) = std::env::var("LOCALFLOW_LOG_FORMAT") {
        match val.to_lowercase().as_str() {
            "json" => config.log.format = LogFormat::Json,
            "pretty" => config.log.format = LogFormat::Pretty,
            other => eprintln!("warning: unknown LOCALFLOW_LOG_FORMAT={other}, ignoring"),
        }
    }

    config
}

pub async fn build_event(
    event_name: &str,
    task: &Task,
    backend: &dyn TaskBackend,
    from_status: Option<TaskStatus>,
    unblocked: Option<Vec<UnblockedTask>>,
) -> HookEvent {
    let stats = backend.task_stats(task.project_id).await.unwrap_or_default();
    let ready_count = backend.ready_count(task.project_id).await.unwrap_or(0);
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
pub async fn fire_hooks(
    config: &Config,
    event_name: &str,
    task: &Task,
    backend: &dyn TaskBackend,
    from_status: Option<TaskStatus>,
    unblocked: Option<Vec<UnblockedTask>>,
) {
    let commands = config.hooks.commands_for_event(event_name);
    if commands.is_empty() {
        return;
    }

    let log_path = log_file_path_with_dir(config.log.dir.as_deref());

    let event = build_event(event_name, task, backend, from_status, unblocked).await;
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

    for cmd in &commands {
        execute_hook(cmd, event_name, &json, log_path.as_deref());
    }
}

/// Fire hooks for the `no_eligible_task` event (no task object in payload).
pub async fn fire_no_eligible_task_hooks(config: &Config, backend: &dyn TaskBackend, project_id: i64) {
    let commands = config.hooks.commands_for_event("no_eligible_task");
    if commands.is_empty() {
        return;
    }

    let log_path = log_file_path_with_dir(config.log.dir.as_deref());

    let stats = backend.task_stats(project_id).await.unwrap_or_default();
    let ready_count = backend.ready_count(project_id).await.unwrap_or(0);
    let event = NoEligibleTaskEvent {
        event_id: Uuid::new_v4().to_string(),
        event: "no_eligible_task".into(),
        timestamp: Utc::now().to_rfc3339(),
        stats,
        ready_count,
    };

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

    for cmd in &commands {
        execute_hook(cmd, "no_eligible_task", &json, log_path.as_deref());
    }
}

/// Return the hook commands configured for the given event name.
/// Returns `None` if the event name is not recognized.
pub fn get_commands_for_event(config: &Config, event_name: &str) -> Option<Vec<String>> {
    let commands = config.hooks.commands_for_event(event_name);
    // Return None only for unrecognized event names
    match event_name {
        "task_added" | "task_ready" | "task_started" | "task_completed" | "task_canceled"
        | "no_eligible_task" => Some(commands.into_iter().map(|s| s.to_string()).collect()),
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
pub async fn compute_unblocked(
    backend: &dyn TaskBackend,
    project_id: i64,
    prev_ready_ids: &std::collections::HashSet<i64>,
) -> Vec<UnblockedTask> {
    let curr_ready = backend.list_ready_tasks(project_id).await.unwrap_or_default();
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
    use crate::infra::sqlite::SqliteBackend;
    use crate::domain::repository::{ProjectRepository, TaskRepository};
    use std::sync::Mutex;

    /// Mutex to serialize tests that modify environment variables.
    /// `std::env::set_var` is not thread-safe, so env-var tests must not run concurrently.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    fn setup_db() -> (tempfile::TempDir, SqliteBackend) {
        let dir = tempfile::tempdir().unwrap();
        let backend = SqliteBackend::new(dir.path()).unwrap();
        (dir, backend)
    }

    #[test]
    fn load_config_missing_file() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let config = load_config(dir.path(), None).unwrap();
        assert!(config.hooks.on_task_added.is_empty());
        assert!(config.hooks.on_task_completed.is_empty());
    }

    #[test]
    fn load_config_valid_toml() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let localflow_dir = dir.path().join(".localflow");
        std::fs::create_dir_all(&localflow_dir).unwrap();
        std::fs::write(
            localflow_dir.join("config.toml"),
            r#"
[hooks.on_task_added.my-hook]
command = "echo added"

[hooks.on_task_completed.my-hook]
command = "echo completed"
"#,
        )
        .unwrap();

        let config = load_config(dir.path(), None).unwrap();
        assert_eq!(config.hooks.on_task_added.len(), 1);
        assert_eq!(config.hooks.on_task_added["my-hook"].command, "echo added");
        assert_eq!(config.hooks.on_task_completed.len(), 1);
        assert_eq!(config.hooks.on_task_completed["my-hook"].command, "echo completed");
    }

    #[test]
    fn load_config_empty_hooks() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let localflow_dir = dir.path().join(".localflow");
        std::fs::create_dir_all(&localflow_dir).unwrap();
        std::fs::write(localflow_dir.join("config.toml"), "[hooks]\n").unwrap();

        let config = load_config(dir.path(), None).unwrap();
        assert!(config.hooks.on_task_added.is_empty());
        assert!(config.hooks.on_task_completed.is_empty());
    }

    #[tokio::test]
    async fn hook_event_serialization() {
        let (_dir, backend) = setup_db();
        let task = Task {
            id: 1,
            project_id: 1,
            title: "Test".into(),
            background: None,
            description: None,
            plan: None,
            priority: crate::domain::task::Priority::P2,
            status: TaskStatus::Draft,
            assignee_session_id: None,
            assignee_user_id: None,
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
        let event = build_event("task_added", &task, &backend, None, None).await;
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

    #[tokio::test]
    async fn event_has_valid_uuid_and_timestamp() {
        let (_dir, backend) = setup_db();
        let task = Task {
            id: 1,
            project_id: 1,
            title: "Test".into(),
            background: None,
            description: None,
            plan: None,
            priority: crate::domain::task::Priority::P2,
            status: TaskStatus::Draft,
            assignee_session_id: None,
            assignee_user_id: None,
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
        let event = build_event("task_added", &task, &backend, None, None).await;
        assert!(Uuid::parse_str(&event.event_id).is_ok());
        assert!(chrono::DateTime::parse_from_rfc3339(&event.timestamp).is_ok());
    }

    #[tokio::test]
    async fn event_has_stats() {
        let (_dir, backend) = setup_db();
        backend.create_task(
            1,
            &crate::domain::task::CreateTaskParams {
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
        .await
        .unwrap();
        let task = backend.get_task(1, 1).await.unwrap();
        let event = build_event("task_added", &task, &backend, None, None).await;
        assert!(event.stats.contains_key("draft"));
        assert_eq!(*event.stats.get("draft").unwrap(), 1);
    }

    #[tokio::test]
    async fn event_has_ready_count() {
        let (_dir, backend) = setup_db();
        backend.create_task(
            1,
            &crate::domain::task::CreateTaskParams {
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
        .await
        .unwrap();
        let mut task = backend.get_task(1, 1).await.unwrap();
        task.ready("2025-01-01T00:00:00Z".to_string()).unwrap();
        backend.save(&task).await.unwrap();
        let task = backend.get_task(1, 1).await.unwrap();
        let event = build_event("task_added", &task, &backend, None, None).await;
        assert_eq!(event.ready_count, 1);
    }

    #[tokio::test]
    async fn compute_unblocked_finds_newly_ready() {
        let (_dir, backend) = setup_db();

        // Create task 1 (will be completed) and task 2 (depends on task 1)
        backend.create_task(
            1,
            &crate::domain::task::CreateTaskParams {
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
        .await
        .unwrap();
        let mut t1 = backend.get_task(1, 1).await.unwrap();
        t1.ready("2025-01-01T00:00:00Z".to_string()).unwrap();
        t1.start(None, None, "2025-01-01T00:00:00Z".to_string()).unwrap();
        backend.save(&t1).await.unwrap();

        backend.create_task(
            1,
            &crate::domain::task::CreateTaskParams {
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
        .await
        .unwrap();
        let mut t2 = backend.get_task(1, 2).await.unwrap();
        t2.ready("2025-01-01T00:00:00Z".to_string()).unwrap();
        backend.save(&t2).await.unwrap();
        backend.add_dependency(1, 2, 1).await.unwrap();

        // Capture ready tasks before completion
        let prev_ready: std::collections::HashSet<i64> =
            backend.list_ready_tasks(1).await.unwrap().iter().map(|t| t.id).collect();

        // Complete task 1
        let mut t1 = backend.get_task(1, 1).await.unwrap();
        t1.complete("2025-01-01T00:00:00Z".to_string()).unwrap();
        backend.save(&t1).await.unwrap();

        let unblocked = compute_unblocked(&backend, 1, &prev_ready).await;
        assert_eq!(unblocked.len(), 1);
        assert_eq!(unblocked[0].id, 2);
        assert_eq!(unblocked[0].title, "Blocked");
    }

    #[tokio::test]
    async fn fire_hooks_executes_multiple_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let marker1 = dir.path().join("hook1.txt");
        let marker2 = dir.path().join("hook2.txt");
        let cmd1 = format!("echo hook1 > {}", marker1.display());
        let cmd2 = format!("echo hook2 > {}", marker2.display());

        let mut on_task_added = std::collections::BTreeMap::new();
        on_task_added.insert("hook1".to_string(), HookEntry { command: cmd1, enabled: true });
        on_task_added.insert("hook2".to_string(), HookEntry { command: cmd2, enabled: true });

        let config = Config {
            hooks: HooksConfig {
                on_task_added,
                ..Default::default()
            },
            ..Default::default()
        };

        let (_db_dir, backend) = setup_db();
        let task = Task {
            id: 1,
            project_id: 1,
            title: "Test".into(),
            background: None,
            description: None,
            plan: None,
            priority: crate::domain::task::Priority::P2,
            status: TaskStatus::Draft,
            assignee_session_id: None,
            assignee_user_id: None,
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
        fire_hooks(&config, "task_added", &task, &backend, None, None).await;

        // Give child processes a moment to complete
        std::thread::sleep(std::time::Duration::from_millis(200));

        assert!(marker1.exists(), "first hook should have run");
        assert!(marker2.exists(), "second hook should have run");
    }

    #[tokio::test]
    async fn fire_hooks_noop_when_no_commands() {
        let (_db_dir, backend) = setup_db();
        let config = Config::default();
        let task = Task {
            id: 1,
            project_id: 1,
            title: "Test".into(),
            background: None,
            description: None,
            plan: None,
            priority: crate::domain::task::Priority::P2,
            status: TaskStatus::Draft,
            assignee_session_id: None,
            assignee_user_id: None,
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
        fire_hooks(&config, "task_added", &task, &backend, None, None).await;
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

    #[tokio::test]
    async fn hook_failure_logged_to_file() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("hooks.log");

        // Run a hook that exits with non-zero
        let mut on_task_added = std::collections::BTreeMap::new();
        on_task_added.insert("fail".to_string(), HookEntry { command: "exit 1".into(), enabled: true });
        let config = Config {
            hooks: HooksConfig {
                on_task_added,
                ..Default::default()
            },
            ..Default::default()
        };

        let (_db_dir, backend) = setup_db();
        let task = Task {
            id: 1,
            project_id: 1,
            title: "Test".into(),
            background: None,
            description: None,
            plan: None,
            priority: crate::domain::task::Priority::P2,
            status: TaskStatus::Draft,
            assignee_session_id: None,
            assignee_user_id: None,
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
        let json = serde_json::to_string(&build_event("task_added", &task, &backend, None, None).await).unwrap();
        execute_hook("exit 1", "task_added", &json, Some(&log_path));

        // Wait for the thread to finish logging
        std::thread::sleep(std::time::Duration::from_millis(300));

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("[WARN]"), "should log failure: {content}");
        assert!(content.contains("hook failed"), "should contain hook failed: {content}");
    }

    #[test]
    fn parse_named_hooks() {
        let toml_str = r#"
[hooks.on_task_added.notify]
command = "echo added"

[hooks.on_task_completed.log]
command = "echo completed"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.hooks.on_task_added.len(), 1);
        assert_eq!(config.hooks.on_task_added["notify"].command, "echo added");
        assert!(config.hooks.on_task_added["notify"].enabled);
        assert_eq!(config.hooks.on_task_completed["log"].command, "echo completed");
    }

    #[test]
    fn parse_named_hooks_multiple() {
        let toml_str = r#"
[hooks.on_task_added.first]
command = "echo first"

[hooks.on_task_added.second]
command = "echo second"

[hooks.on_task_completed.notify]
command = "notify"

[hooks.on_task_completed.log]
command = "log"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.hooks.on_task_added.len(), 2);
        assert_eq!(config.hooks.on_task_completed.len(), 2);
    }

    #[test]
    fn parse_hooks_with_enabled_false() {
        let toml_str = r#"
[hooks.on_task_added.disabled-hook]
command = "echo disabled"
enabled = false
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(!config.hooks.on_task_added["disabled-hook"].enabled);
        let commands = config.hooks.commands_for_event("task_added");
        assert!(commands.is_empty());
    }

    #[test]
    fn parse_hooks_missing_fields() {
        let toml_str = "[hooks]\n";
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.hooks.on_task_added.is_empty());
        assert!(config.hooks.on_task_completed.is_empty());
    }

    #[test]
    fn legacy_hook_format_rejected() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let localflow_dir = dir.path().join(".localflow");
        std::fs::create_dir_all(&localflow_dir).unwrap();
        std::fs::write(
            localflow_dir.join("config.toml"),
            r#"
[hooks]
on_task_added = "echo added"
"#,
        )
        .unwrap();

        let result = load_config(dir.path(), None);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Legacy hook format"), "error should mention legacy format: {err}");
        assert!(err.contains("named hooks"), "error should mention migration: {err}");
    }

    #[tokio::test]
    async fn hook_receives_json_on_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let output_file = dir.path().join("stdin_capture.json");
        let cmd = format!("cat > {}", output_file.display());

        let mut on_task_added = std::collections::BTreeMap::new();
        on_task_added.insert("capture".to_string(), HookEntry { command: cmd, enabled: true });
        let config = Config {
            hooks: HooksConfig {
                on_task_added,
                ..Default::default()
            },
            ..Default::default()
        };

        let (_db_dir, backend) = setup_db();
        let task = Task {
            id: 42,
            project_id: 1,
            title: "Hook stdin test".into(),
            background: None,
            description: None,
            plan: None,
            priority: crate::domain::task::Priority::P1,
            status: TaskStatus::Draft,
            assignee_session_id: None,
            assignee_user_id: None,
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
        fire_hooks(&config, "task_added", &task, &backend, None, None).await;

        std::thread::sleep(std::time::Duration::from_millis(200));

        let content = std::fs::read_to_string(&output_file).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(json["event"], "task_added");
        assert_eq!(json["task"]["id"], 42);
        assert_eq!(json["task"]["title"], "Hook stdin test");
    }

    #[test]
    fn env_override_completion_mode() {
        let _lock = ENV_MUTEX.lock().unwrap();
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
        let _lock = ENV_MUTEX.lock().unwrap();
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
        let _lock = ENV_MUTEX.lock().unwrap();
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
        let _lock = ENV_MUTEX.lock().unwrap();
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
    fn env_override_hooks_insert() {
        let _lock = ENV_MUTEX.lock().unwrap();
        unsafe {
            let orig = std::env::var("LOCALFLOW_HOOK_ON_TASK_ADDED").ok();
            std::env::set_var("LOCALFLOW_HOOK_ON_TASK_ADDED", "env-hook");
            // Start with a config that already has a hook from config.toml
            let mut config = Config::default();
            config.hooks.on_task_added.insert(
                "toml-hook".to_string(),
                HookEntry { command: "toml-hook".into(), enabled: true },
            );
            let config = apply_env_overrides(config);
            assert_eq!(config.hooks.on_task_added.len(), 2);
            assert_eq!(config.hooks.on_task_added["toml-hook"].command, "toml-hook");
            assert_eq!(config.hooks.on_task_added["_env"].command, "env-hook");
            match orig {
                Some(v) => std::env::set_var("LOCALFLOW_HOOK_ON_TASK_ADDED", v),
                None => std::env::remove_var("LOCALFLOW_HOOK_ON_TASK_ADDED"),
            }
        }
    }

    #[test]
    fn env_override_empty_values_ignored() {
        let _lock = ENV_MUTEX.lock().unwrap();
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
        let _lock = ENV_MUTEX.lock().unwrap();
        unsafe {
            let orig = std::env::var("LOCALFLOW_COMPLETION_MODE").ok();
            std::env::set_var("LOCALFLOW_COMPLETION_MODE", "pr_then_complete");
            let tmp = tempfile::tempdir().unwrap();
            let config = load_config(tmp.path(), None).unwrap();
            assert_eq!(config.workflow.completion_mode, CompletionMode::PrThenComplete);
            match orig {
                Some(v) => std::env::set_var("LOCALFLOW_COMPLETION_MODE", v),
                None => std::env::remove_var("LOCALFLOW_COMPLETION_MODE"),
            }
        }
    }

    #[test]
    fn load_config_explicit_path() {
        let tmp = tempfile::tempdir().unwrap();
        let config_file = tmp.path().join("custom-config.toml");
        std::fs::write(
            &config_file,
            r#"
[workflow]
completion_mode = "pr_then_complete"
auto_merge = false
"#,
        )
        .unwrap();
        let config = load_config(tmp.path(), Some(&config_file)).unwrap();
        assert_eq!(config.workflow.completion_mode, CompletionMode::PrThenComplete);
        assert!(!config.workflow.auto_merge);
    }

    #[test]
    fn load_config_explicit_path_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nonexistent.toml");
        let result = load_config(tmp.path(), Some(&missing));
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("config file not found"),
            "should report missing config file"
        );
    }

    #[test]
    fn load_config_env_var_path() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_file = tmp.path().join("env-config.toml");
        std::fs::write(
            &config_file,
            r#"
[workflow]
auto_merge = false
"#,
        )
        .unwrap();

        unsafe {
            let orig = std::env::var("LOCALFLOW_CONFIG").ok();
            std::env::set_var("LOCALFLOW_CONFIG", config_file.to_str().unwrap());
            let config = load_config(tmp.path(), None).unwrap();
            assert!(!config.workflow.auto_merge);
            match orig {
                Some(v) => std::env::set_var("LOCALFLOW_CONFIG", v),
                None => std::env::remove_var("LOCALFLOW_CONFIG"),
            }
        }
    }

    #[test]
    fn load_config_explicit_overrides_env_var() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();

        let env_config = tmp.path().join("env-config.toml");
        std::fs::write(
            &env_config,
            r#"
[workflow]
auto_merge = true
"#,
        )
        .unwrap();

        let cli_config = tmp.path().join("cli-config.toml");
        std::fs::write(
            &cli_config,
            r#"
[workflow]
auto_merge = false
"#,
        )
        .unwrap();

        unsafe {
            let orig = std::env::var("LOCALFLOW_CONFIG").ok();
            std::env::set_var("LOCALFLOW_CONFIG", env_config.to_str().unwrap());
            let config = load_config(tmp.path(), Some(&cli_config)).unwrap();
            // CLI flag should take priority over env var
            assert!(!config.workflow.auto_merge);
            match orig {
                Some(v) => std::env::set_var("LOCALFLOW_CONFIG", v),
                None => std::env::remove_var("LOCALFLOW_CONFIG"),
            }
        }
    }

    #[test]
    fn load_config_env_var_not_found() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            let orig = std::env::var("LOCALFLOW_CONFIG").ok();
            std::env::set_var("LOCALFLOW_CONFIG", "/nonexistent/path/config.toml");
            let result = load_config(tmp.path(), None);
            assert!(result.is_err());
            assert!(
                result.unwrap_err().to_string().contains("config file not found"),
                "should report missing config file from env var"
            );
            match orig {
                Some(v) => std::env::set_var("LOCALFLOW_CONFIG", v),
                None => std::env::remove_var("LOCALFLOW_CONFIG"),
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
        let toml_str = "[hooks]\n";
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.log.dir, None);
    }

    #[test]
    fn raw_config_merge_overlay_wins() {
        let base = RawConfig {
            workflow: RawWorkflowConfig {
                completion_mode: Some(CompletionMode::MergeThenComplete),
                auto_merge: Some(true),
            },
            log: RawLogConfig {
                level: Some("debug".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let overlay = RawConfig {
            workflow: RawWorkflowConfig {
                completion_mode: Some(CompletionMode::PrThenComplete),
                auto_merge: None,
            },
            ..Default::default()
        };
        let merged = base.merge(overlay).resolve();
        assert_eq!(merged.workflow.completion_mode, CompletionMode::PrThenComplete);
        assert!(merged.workflow.auto_merge); // from base
        assert_eq!(merged.log.level, "debug"); // from base
    }

    #[test]
    fn raw_config_merge_hooks() {
        let mut base_hooks = HooksConfig::default();
        base_hooks.on_task_added.insert(
            "user-hook".to_string(),
            HookEntry { command: "user-cmd".into(), enabled: true },
        );
        base_hooks.on_task_completed.insert(
            "shared".to_string(),
            HookEntry { command: "user-completed".into(), enabled: true },
        );

        let mut overlay_hooks = HooksConfig::default();
        overlay_hooks.on_task_added.insert(
            "project-hook".to_string(),
            HookEntry { command: "project-cmd".into(), enabled: true },
        );
        // Override the shared hook
        overlay_hooks.on_task_completed.insert(
            "shared".to_string(),
            HookEntry { command: "project-completed".into(), enabled: true },
        );

        let base = RawConfig { hooks: base_hooks, ..Default::default() };
        let overlay = RawConfig { hooks: overlay_hooks, ..Default::default() };
        let merged = base.merge(overlay).resolve();

        // Both hooks present for on_task_added
        assert_eq!(merged.hooks.on_task_added.len(), 2);
        assert_eq!(merged.hooks.on_task_added["user-hook"].command, "user-cmd");
        assert_eq!(merged.hooks.on_task_added["project-hook"].command, "project-cmd");
        // Shared hook overridden by overlay
        assert_eq!(merged.hooks.on_task_completed["shared"].command, "project-completed");
    }

    #[test]
    fn raw_config_merge_hook_disable() {
        let mut base_hooks = HooksConfig::default();
        base_hooks.on_task_added.insert(
            "notify".to_string(),
            HookEntry { command: "notify-cmd".into(), enabled: true },
        );

        let mut overlay_hooks = HooksConfig::default();
        overlay_hooks.on_task_added.insert(
            "notify".to_string(),
            HookEntry { command: "".into(), enabled: false },
        );

        let base = RawConfig { hooks: base_hooks, ..Default::default() };
        let overlay = RawConfig { hooks: overlay_hooks, ..Default::default() };
        let merged = base.merge(overlay).resolve();

        // Hook is in the map but disabled
        assert!(!merged.hooks.on_task_added["notify"].enabled);
        // commands_for_event should filter it out
        let cmds = merged.hooks.commands_for_event("task_added");
        assert!(cmds.is_empty());
    }

    #[test]
    fn user_config_loaded_as_fallback() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let user_config_dir = tmp.path().join("user-config").join("localflow");
        std::fs::create_dir_all(&user_config_dir).unwrap();
        std::fs::write(
            user_config_dir.join("config.toml"),
            r#"
[workflow]
auto_merge = false

[hooks.on_task_added.user-hook]
command = "user-cmd"
"#,
        )
        .unwrap();

        // Project has no config
        let project_dir = tmp.path().join("project");
        std::fs::create_dir_all(project_dir.join(".localflow")).unwrap();

        unsafe {
            let orig_xdg = std::env::var("XDG_CONFIG_HOME").ok();
            std::env::set_var("XDG_CONFIG_HOME", tmp.path().join("user-config"));
            let config = load_config(project_dir.as_path(), None).unwrap();
            assert!(!config.workflow.auto_merge);
            assert_eq!(config.hooks.on_task_added.len(), 1);
            assert_eq!(config.hooks.on_task_added["user-hook"].command, "user-cmd");
            match orig_xdg {
                Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }
    }

    #[test]
    fn project_config_overrides_user_config() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();

        // User config
        let user_config_dir = tmp.path().join("user-config").join("localflow");
        std::fs::create_dir_all(&user_config_dir).unwrap();
        std::fs::write(
            user_config_dir.join("config.toml"),
            r#"
[workflow]
auto_merge = false
completion_mode = "pr_then_complete"

[hooks.on_task_added.user-hook]
command = "user-cmd"
"#,
        )
        .unwrap();

        // Project config overrides some fields
        let project_dir = tmp.path().join("project");
        let localflow_dir = project_dir.join(".localflow");
        std::fs::create_dir_all(&localflow_dir).unwrap();
        std::fs::write(
            localflow_dir.join("config.toml"),
            r#"
[workflow]
auto_merge = true

[hooks.on_task_added.project-hook]
command = "project-cmd"
"#,
        )
        .unwrap();

        unsafe {
            let orig_xdg = std::env::var("XDG_CONFIG_HOME").ok();
            std::env::set_var("XDG_CONFIG_HOME", tmp.path().join("user-config"));
            let config = load_config(project_dir.as_path(), None).unwrap();
            // auto_merge overridden by project
            assert!(config.workflow.auto_merge);
            // completion_mode falls back to user config
            assert_eq!(config.workflow.completion_mode, CompletionMode::PrThenComplete);
            // Both hooks present
            assert_eq!(config.hooks.on_task_added.len(), 2);
            match orig_xdg {
                Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }
    }
}
