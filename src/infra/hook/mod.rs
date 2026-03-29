pub mod executor;

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use chrono::Utc;
use uuid::Uuid;

use crate::domain::config::{Config, HookEntry, RawConfig};
use crate::domain::repository::TaskBackend;
use crate::domain::task::{Task, TaskStatus, UnblockedTask};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMode {
    Cli,
    Api,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackendInfo {
    Sqlite { db_file_path: String },
    Postgresql,
    Dynamodb,
    Http { api_url: String },
}

#[derive(Debug, Serialize)]
pub struct HookEnvelope<T: Serialize> {
    pub runtime: RuntimeMode,
    pub backend: BackendInfo,
    pub event: T,
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

#[derive(Debug, Serialize)]
pub struct NoEligibleTaskEvent {
    pub event_id: String,
    pub event: String,
    pub timestamp: String,
    pub stats: HashMap<String, i64>,
    pub ready_count: i64,
}

/// Maximum bytes of stdout/stderr to retain in log entries.
const MAX_OUTPUT_BYTES: usize = 4096;

/// Structured JSONL log entry for hook operations.
#[derive(Debug, Serialize)]
struct HookLogEntry {
    timestamp: String,
    level: String,
    #[serde(rename = "type")]
    log_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hook: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    backend: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stderr: Option<String>,
}

impl RuntimeMode {
    fn as_str(&self) -> &str {
        match self {
            RuntimeMode::Cli => "cli",
            RuntimeMode::Api => "api",
        }
    }
}

impl HookLogEntry {
    fn new(level: &str, log_type: &str) -> Self {
        Self {
            timestamp: Utc::now().to_rfc3339(),
            level: level.to_owned(),
            log_type: log_type.to_owned(),
            event_id: None,
            event: None,
            hook: None,
            command: None,
            task_id: None,
            message: None,
            exit_code: None,
            runtime: None,
            backend: None,
            stdout: None,
            stderr: None,
        }
    }

    fn with_event_id(mut self, v: &str) -> Self {
        self.event_id = Some(v.to_owned());
        self
    }

    fn with_event(mut self, v: &str) -> Self {
        self.event = Some(v.to_owned());
        self
    }

    fn with_hook(mut self, v: &str) -> Self {
        self.hook = Some(v.to_owned());
        self
    }

    fn with_command(mut self, v: &str) -> Self {
        self.command = Some(v.to_owned());
        self
    }

    fn with_task_id(mut self, v: Option<i64>) -> Self {
        self.task_id = v;
        self
    }

    fn with_message(mut self, v: &str) -> Self {
        self.message = Some(v.to_owned());
        self
    }

    fn with_exit_code(mut self, v: Option<i32>) -> Self {
        self.exit_code = v;
        self
    }

    fn with_runtime(mut self, v: &str) -> Self {
        self.runtime = Some(v.to_owned());
        self
    }

    fn with_backend(mut self, v: &BackendInfo) -> Self {
        self.backend = serde_json::to_value(v).ok();
        self
    }
}

/// Truncate byte output to at most `MAX_OUTPUT_BYTES`, keeping the tail.
fn truncate_output(bytes: &[u8]) -> String {
    if bytes.len() <= MAX_OUTPUT_BYTES {
        String::from_utf8_lossy(bytes).into_owned()
    } else {
        let start = bytes.len() - MAX_OUTPUT_BYTES;
        String::from_utf8_lossy(&bytes[start..]).into_owned()
    }
}

pub fn load_config(project_root: &Path, explicit_config: Option<&Path>) -> Result<Config> {
    // 1. Load user config (lowest priority layer)
    let user_raw = load_user_config()?;

    // 2. Determine and load the project/explicit config
    let project_raw = if let Some(path) = explicit_config {
        // Explicit --config flag: must exist
        Some(load_config_file(path, true)?)
    } else if let Some(env_path) = env_config_path() {
        // SENKO_CONFIG env var: must exist
        Some(load_config_file(&env_path, true)?)
    } else {
        let default_path = project_root.join(".senko").join("config.toml");
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
    let mut config = merged_raw.resolve();
    config.apply_env();
    Ok(config)
}

/// Return the user-level config path.
/// `$XDG_CONFIG_HOME/senko/config.toml` or `~/.config/senko/config.toml`
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
    Some(config_dir.join("senko").join("config.toml"))
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

/// Return the config path from the SENKO_CONFIG env var, if set.
fn env_config_path() -> Option<PathBuf> {
    std::env::var("SENKO_CONFIG")
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

pub async fn build_event(
    event_name: &str,
    task: &Task,
    backend: &dyn TaskBackend,
    from_status: Option<TaskStatus>,
    unblocked: Option<Vec<UnblockedTask>>,
) -> HookEvent {
    let stats = backend.task_stats(task.project_id()).await.unwrap_or_default();
    let ready_count = backend.ready_count(task.project_id()).await.unwrap_or(0);
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
/// Priority: `log_dir` override > `$XDG_STATE_HOME/senko` > `~/.local/state/senko`
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
        state_dir.join("senko")
    };
    Some(dir.join("hooks.log"))
}

/// Return the hook log file path following XDG Base Directory specification.
/// `$XDG_STATE_HOME/senko/hooks.log` (default: `~/.local/state/senko/hooks.log`)
pub fn log_file_path() -> Option<PathBuf> {
    log_file_path_with_dir(None)
}

fn log_to_file(path: &Path, entry: &HookLogEntry) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        // Build the full JSONL line so the O_APPEND write is atomic.
        if let Ok(json) = serde_json::to_string(entry) {
            let mut line = json;
            line.push('\n');
            let _ = f.write_all(line.as_bytes());
        }
    }
}

fn execute_hook(
    command: &str,
    event_name: &str,
    event_id: &str,
    hook_name: &str,
    task_id: Option<i64>,
    json: &str,
    log_path: Option<&Path>,
) {
    let mut child = match std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("hook spawn error ({}): {}: {:#}", event_name, command, e);
            eprintln!("{msg}");
            if let Some(p) = log_path {
                let entry = HookLogEntry::new("ERROR", "hook_error")
                    .with_event_id(event_id)
                    .with_event(event_name)
                    .with_hook(hook_name)
                    .with_command(command)
                    .with_task_id(task_id)
                    .with_message(&msg);
                log_to_file(p, &entry);
            }
            return;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(json.as_bytes()) {
            let msg = format!("hook stdin error ({}): {}: {:#}", event_name, command, e);
            eprintln!("{msg}");
            if let Some(p) = log_path {
                let entry = HookLogEntry::new("ERROR", "hook_error")
                    .with_event_id(event_id)
                    .with_event(event_name)
                    .with_hook(hook_name)
                    .with_command(command)
                    .with_task_id(task_id)
                    .with_message(&msg);
                log_to_file(p, &entry);
            }
            return;
        }
    }

    // Spawn a thread to wait for exit and log the result.
    // The CLI returns immediately; the thread outlives the main function
    // but Rust waits for non-daemon threads before process exit.
    let cmd = command.to_owned();
    let evt = event_name.to_owned();
    let eid = event_id.to_owned();
    let hname = hook_name.to_owned();
    let tid = task_id;
    let log = log_path.map(|p| p.to_owned());
    std::thread::spawn(move || {
        match child.wait_with_output() {
            Ok(output) if output.status.success() => {
                // Success: discard stdout/stderr
                if let Some(p) = log {
                    let entry = HookLogEntry::new("INFO", "hook_ok")
                        .with_event_id(&eid)
                        .with_event(&evt)
                        .with_hook(&hname)
                        .with_command(&cmd)
                        .with_task_id(tid)
                        .with_exit_code(output.status.code());
                    log_to_file(&p, &entry);
                }
            }
            Ok(output) => {
                let msg = format!(
                    "hook failed ({}): {} (exit: {})",
                    evt,
                    cmd,
                    output.status.code().map_or("signal".to_string(), |c| c.to_string())
                );
                eprintln!("{msg}");
                if let Some(p) = log {
                    let mut entry = HookLogEntry::new("WARN", "hook_failed")
                        .with_event_id(&eid)
                        .with_event(&evt)
                        .with_hook(&hname)
                        .with_command(&cmd)
                        .with_task_id(tid)
                        .with_exit_code(output.status.code());
                    if !output.stdout.is_empty() {
                        entry.stdout = Some(truncate_output(&output.stdout));
                    }
                    if !output.stderr.is_empty() {
                        entry.stderr = Some(truncate_output(&output.stderr));
                    }
                    log_to_file(&p, &entry);
                }
            }
            Err(e) => {
                let msg = format!("hook wait error ({}): {}: {:#}", evt, cmd, e);
                eprintln!("{msg}");
                if let Some(p) = log {
                    let entry = HookLogEntry::new("ERROR", "hook_error")
                        .with_event_id(&eid)
                        .with_event(&evt)
                        .with_hook(&hname)
                        .with_command(&cmd)
                        .with_task_id(tid)
                        .with_message(&msg);
                    log_to_file(&p, &entry);
                }
            }
        }
    });
}

/// Check which required environment variables are missing for a hook entry.
/// Returns a list of missing variable names. Empty list means all required vars are set.
fn check_required_env(entry: &HookEntry) -> Vec<&str> {
    entry
        .requires_env
        .iter()
        .filter(|var| std::env::var(var).is_err())
        .map(|s| s.as_str())
        .collect()
}

/// Fire hooks for the given event, spawning each hook command as a
/// fire-and-forget child process. Returns immediately.
/// Results are logged to `$XDG_STATE_HOME/senko/hooks.log`.
pub async fn fire_hooks(
    config: &Config,
    event_name: &str,
    task: &Task,
    backend: &dyn TaskBackend,
    from_status: Option<TaskStatus>,
    unblocked: Option<Vec<UnblockedTask>>,
    runtime_mode: &RuntimeMode,
    backend_info: &BackendInfo,
) {
    let entries = config.hooks.entries_for_event(event_name);
    let log_path = log_file_path_with_dir(config.log.dir.as_deref());

    let event = build_event(event_name, task, backend, from_status, unblocked).await;
    let envelope = HookEnvelope {
        runtime: runtime_mode.clone(),
        backend: backend_info.clone(),
        event,
    };
    let json = match serde_json::to_string(&envelope) {
        Ok(j) => j,
        Err(e) => {
            let msg = format!("hook error: failed to serialize event: {e}");
            eprintln!("{msg}");
            if let Some(ref p) = log_path {
                let entry = HookLogEntry::new("ERROR", "hook_error")
                    .with_event_id(&envelope.event.event_id)
                    .with_event(event_name)
                    .with_task_id(Some(task.id()))
                    .with_runtime(runtime_mode.as_str())
                    .with_backend(backend_info)
                    .with_message(&msg);
                log_to_file(p, &entry);
            }
            return;
        }
    };

    // Always log event_fired, even with 0 hooks
    if let Some(ref p) = log_path {
        let entry = HookLogEntry::new("INFO", "event_fired")
            .with_event_id(&envelope.event.event_id)
            .with_event(event_name)
            .with_task_id(Some(task.id()))
            .with_runtime(runtime_mode.as_str())
            .with_backend(backend_info);
        log_to_file(p, &entry);
    }

    if entries.is_empty() {
        return;
    }

    for (name, hook_entry) in &entries {
        let missing = check_required_env(hook_entry);
        if !missing.is_empty() {
            let msg = format!(
                "hook skipped ({}): {} — missing env: {}",
                event_name, name, missing.join(", ")
            );
            eprintln!("{msg}");
            if let Some(ref p) = log_path {
                let entry = HookLogEntry::new("WARN", "hook_skipped")
                    .with_event_id(&envelope.event.event_id)
                    .with_event(event_name)
                    .with_hook(name)
                    .with_command(&hook_entry.command)
                    .with_task_id(Some(task.id()))
                    .with_runtime(runtime_mode.as_str())
                    .with_backend(backend_info)
                    .with_message(&msg);
                log_to_file(p, &entry);
            }
            continue;
        }
        execute_hook(
            &hook_entry.command,
            event_name,
            &envelope.event.event_id,
            name,
            Some(task.id()),
            &json,
            log_path.as_deref(),
        );
    }
}

/// Fire hooks for the `no_eligible_task` event (no task object in payload).
pub async fn fire_no_eligible_task_hooks(
    config: &Config,
    backend: &dyn TaskBackend,
    project_id: i64,
    runtime_mode: &RuntimeMode,
    backend_info: &BackendInfo,
) {
    let entries = config.hooks.entries_for_event("no_eligible_task");
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
    let envelope = HookEnvelope {
        runtime: runtime_mode.clone(),
        backend: backend_info.clone(),
        event,
    };

    let json = match serde_json::to_string(&envelope) {
        Ok(j) => j,
        Err(e) => {
            let msg = format!("hook error: failed to serialize event: {e}");
            eprintln!("{msg}");
            if let Some(ref p) = log_path {
                let entry = HookLogEntry::new("ERROR", "hook_error")
                    .with_event_id(&envelope.event.event_id)
                    .with_event("no_eligible_task")
                    .with_runtime(runtime_mode.as_str())
                    .with_backend(backend_info)
                    .with_message(&msg);
                log_to_file(p, &entry);
            }
            return;
        }
    };

    // Always log event_fired, even with 0 hooks
    if let Some(ref p) = log_path {
        let entry = HookLogEntry::new("INFO", "event_fired")
            .with_event_id(&envelope.event.event_id)
            .with_event("no_eligible_task")
            .with_runtime(runtime_mode.as_str())
            .with_backend(backend_info);
        log_to_file(p, &entry);
    }

    if entries.is_empty() {
        return;
    }

    for (name, hook_entry) in &entries {
        let missing = check_required_env(hook_entry);
        if !missing.is_empty() {
            let msg = format!(
                "hook skipped (no_eligible_task): {} — missing env: {}",
                name, missing.join(", ")
            );
            eprintln!("{msg}");
            if let Some(ref p) = log_path {
                let entry = HookLogEntry::new("WARN", "hook_skipped")
                    .with_event_id(&envelope.event.event_id)
                    .with_event("no_eligible_task")
                    .with_hook(name)
                    .with_command(&hook_entry.command)
                    .with_runtime(runtime_mode.as_str())
                    .with_backend(backend_info)
                    .with_message(&msg);
                log_to_file(p, &entry);
            }
            continue;
        }
        execute_hook(
            &hook_entry.command,
            "no_eligible_task",
            &envelope.event.event_id,
            name,
            None,
            &json,
            log_path.as_deref(),
        );
    }
}

/// Return the hook commands configured for the given event name,
/// filtering out hooks with missing required environment variables.
/// Returns `None` if the event name is not recognized.
pub fn get_commands_for_event(config: &Config, event_name: &str) -> Option<Vec<String>> {
    // Return None only for unrecognized event names
    match event_name {
        "task_added" | "task_ready" | "task_started" | "task_completed" | "task_canceled"
        | "no_eligible_task" => {
            let entries = config.hooks.entries_for_event(event_name);
            let mut commands = Vec::new();
            for (name, entry) in &entries {
                let missing = check_required_env(entry);
                if !missing.is_empty() {
                    eprintln!(
                        "hook skipped ({}): {} — missing env: {}",
                        event_name, name, missing.join(", ")
                    );
                    continue;
                }
                commands.push(entry.command.clone());
            }
            Some(commands)
        }
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
        .filter(|t| !prev_ready_ids.contains(&t.id()))
        .map(|t| UnblockedTask::new(t.id(), t.title().to_string(), t.priority(), t.metadata().cloned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::config::{
        CompletionMode, HookMode, HooksConfig, RawLogConfig, RawWorkflowConfig,
    };
    use crate::infra::sqlite::SqliteBackend;
    use crate::domain::repository::{ProjectRepository, TaskRepository};
    use std::sync::Mutex;

    /// Mutex to serialize tests that modify environment variables.
    /// `std::env::set_var` is not thread-safe, so env-var tests must not run concurrently.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    /// Run a closure with `XDG_CONFIG_HOME` pointed to an empty temp dir,
    /// preventing user-level config from leaking into tests.
    /// Returns the closure's return value. The temp dir is cleaned up on drop.
    fn with_isolated_user_config<F, R>(f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            let orig = std::env::var("XDG_CONFIG_HOME").ok();
            std::env::set_var("XDG_CONFIG_HOME", tmp.path());
            let result = f();
            match orig {
                Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
            result
        }
    }

    fn setup_db() -> (tempfile::TempDir, SqliteBackend) {
        let dir = tempfile::tempdir().unwrap();
        let backend = SqliteBackend::new(dir.path(), Some(&dir.path().join("data.db")), None).unwrap();
        (dir, backend)
    }

    #[test]
    fn load_config_missing_file() {
        let _lock = ENV_MUTEX.lock().unwrap();
        with_isolated_user_config(|| {
            let dir = tempfile::tempdir().unwrap();
            let config = load_config(dir.path(), None).unwrap();
            assert!(config.hooks.on_task_added.is_empty());
            assert!(config.hooks.on_task_completed.is_empty());
        });
    }

    #[test]
    fn load_config_valid_toml() {
        let _lock = ENV_MUTEX.lock().unwrap();
        with_isolated_user_config(|| {
            let dir = tempfile::tempdir().unwrap();
            let senko_dir = dir.path().join(".senko");
            std::fs::create_dir_all(&senko_dir).unwrap();
            std::fs::write(
                senko_dir.join("config.toml"),
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
        });
    }

    #[test]
    fn load_config_empty_hooks() {
        let _lock = ENV_MUTEX.lock().unwrap();
        with_isolated_user_config(|| {
            let dir = tempfile::tempdir().unwrap();
            let senko_dir = dir.path().join(".senko");
            std::fs::create_dir_all(&senko_dir).unwrap();
            std::fs::write(senko_dir.join("config.toml"), "[hooks]\n").unwrap();

            let config = load_config(dir.path(), None).unwrap();
            assert!(config.hooks.on_task_added.is_empty());
            assert!(config.hooks.on_task_completed.is_empty());
        });
    }

    #[tokio::test]
    async fn hook_event_serialization() {
        let (_dir, backend) = setup_db();
        let task = Task::new(
            1, 1, "Test".into(), None, None, None,
            crate::domain::task::Priority::P2, TaskStatus::Draft,
            None, None,
            "2026-01-01T00:00:00Z".into(), "2026-01-01T00:00:00Z".into(),
            None, None, None, None, None, None, None,
            vec![], vec![], vec![], vec![], vec![],
        );
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
        let task = Task::new(
            1, 1, "Test".into(), None, None, None,
            crate::domain::task::Priority::P2, TaskStatus::Draft,
            None, None,
            "2026-01-01T00:00:00Z".into(), "2026-01-01T00:00:00Z".into(),
            None, None, None, None, None, None, None,
            vec![], vec![], vec![], vec![], vec![],
        );
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
        let task = backend.get_task(1, 1).await.unwrap();
        let (task, _) = task.ready("2025-01-01T00:00:00Z".to_string()).unwrap();
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
        let t1 = backend.get_task(1, 1).await.unwrap();
        let (t1, _) = t1.ready("2025-01-01T00:00:00Z".to_string()).unwrap();
        let (t1, _) = t1.start(None, None, "2025-01-01T00:00:00Z".to_string()).unwrap();
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
        let t2 = backend.get_task(1, 2).await.unwrap();
        let (t2, _) = t2.ready("2025-01-01T00:00:00Z".to_string()).unwrap();
        backend.save(&t2).await.unwrap();
        backend.add_dependency(1, 2, 1).await.unwrap();

        // Capture ready tasks before completion
        let prev_ready: std::collections::HashSet<i64> =
            backend.list_ready_tasks(1).await.unwrap().iter().map(|t| t.id()).collect();

        // Complete task 1
        let t1 = backend.get_task(1, 1).await.unwrap();
        let (t1, _) = t1.complete("2025-01-01T00:00:00Z".to_string()).unwrap();
        backend.save(&t1).await.unwrap();

        let unblocked = compute_unblocked(&backend, 1, &prev_ready).await;
        assert_eq!(unblocked.len(), 1);
        assert_eq!(unblocked[0].id(), 2);
        assert_eq!(unblocked[0].title(), "Blocked");
    }

    #[tokio::test]
    async fn fire_hooks_executes_multiple_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let marker1 = dir.path().join("hook1.txt");
        let marker2 = dir.path().join("hook2.txt");
        let cmd1 = format!("echo hook1 > {}", marker1.display());
        let cmd2 = format!("echo hook2 > {}", marker2.display());

        let mut on_task_added = std::collections::BTreeMap::new();
        on_task_added.insert("hook1".to_string(), HookEntry { command: cmd1, enabled: true, requires_env: vec![] });
        on_task_added.insert("hook2".to_string(), HookEntry { command: cmd2, enabled: true, requires_env: vec![] });

        let config = Config {
            hooks: HooksConfig {
                on_task_added,
                ..Default::default()
            },
            ..Default::default()
        };

        let (_db_dir, backend) = setup_db();
        let task = Task::new(
            1, 1, "Test".into(), None, None, None,
            crate::domain::task::Priority::P2, TaskStatus::Draft,
            None, None,
            "2026-01-01T00:00:00Z".into(), "2026-01-01T00:00:00Z".into(),
            None, None, None, None, None, None, None,
            vec![], vec![], vec![], vec![], vec![],
        );
        fire_hooks(&config, "task_added", &task, &backend, None, None, &RuntimeMode::Cli, &BackendInfo::Sqlite { db_file_path: "test.db".into() }).await;

        // Give child processes a moment to complete
        std::thread::sleep(std::time::Duration::from_millis(200));

        assert!(marker1.exists(), "first hook should have run");
        assert!(marker2.exists(), "second hook should have run");
    }

    #[tokio::test]
    async fn fire_hooks_noop_when_no_commands() {
        let (_db_dir, backend) = setup_db();
        let config = Config::default();
        let task = Task::new(
            1, 1, "Test".into(), None, None, None,
            crate::domain::task::Priority::P2, TaskStatus::Draft,
            None, None,
            "2026-01-01T00:00:00Z".into(), "2026-01-01T00:00:00Z".into(),
            None, None, None, None, None, None, None,
            vec![], vec![], vec![], vec![], vec![],
        );
        // Should not panic
        fire_hooks(&config, "task_added", &task, &backend, None, None, &RuntimeMode::Cli, &BackendInfo::Sqlite { db_file_path: "test.db".into() }).await;
    }

    #[test]
    fn log_file_path_uses_xdg_state_home() {
        unsafe {
            let orig = std::env::var("XDG_STATE_HOME").ok();
            std::env::set_var("XDG_STATE_HOME", "/tmp/test-xdg-state");
            let path = log_file_path().unwrap();
            assert_eq!(
                path,
                PathBuf::from("/tmp/test-xdg-state/senko/hooks.log")
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
                PathBuf::from("/tmp/test-home/.local/state/senko/hooks.log")
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
        let entry1 = HookLogEntry::new("INFO", "hook_ok")
            .with_message("first message");
        let entry2 = HookLogEntry::new("WARN", "hook_failed")
            .with_message("second message");
        log_to_file(&log_path, &entry1);
        log_to_file(&log_path, &entry2);
        let content = std::fs::read_to_string(&log_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        let j1: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(j1["level"], "INFO");
        assert_eq!(j1["type"], "hook_ok");
        assert_eq!(j1["message"], "first message");
        let j2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(j2["level"], "WARN");
        assert_eq!(j2["type"], "hook_failed");
    }

    #[tokio::test]
    async fn hook_failure_logged_to_file() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("hooks.log");

        let (_db_dir, backend) = setup_db();
        let task = Task::new(
            1, 1, "Test".into(), None, None, None,
            crate::domain::task::Priority::P2, TaskStatus::Draft,
            None, None,
            "2026-01-01T00:00:00Z".into(), "2026-01-01T00:00:00Z".into(),
            None, None, None, None, None, None, None,
            vec![], vec![], vec![], vec![], vec![],
        );

        // Call execute_hook directly with our log path
        let json = serde_json::to_string(&build_event("task_added", &task, &backend, None, None).await).unwrap();
        execute_hook("exit 1", "task_added", "test-event-id", "fail", Some(1), &json, Some(&log_path));

        // Wait for the thread to finish logging
        std::thread::sleep(std::time::Duration::from_millis(300));

        let content = std::fs::read_to_string(&log_path).unwrap();
        let line: serde_json::Value = serde_json::from_str(content.lines().next().unwrap()).unwrap();
        assert_eq!(line["level"], "WARN", "should log failure: {content}");
        assert_eq!(line["type"], "hook_failed", "should be hook_failed type: {content}");
        assert_eq!(line["event_id"], "test-event-id");
        assert_eq!(line["hook"], "fail");
        assert_eq!(line["event"], "task_added");
        assert!(line["exit_code"].is_number());
    }

    #[test]
    fn truncate_output_within_limit() {
        let data = b"hello";
        assert_eq!(truncate_output(data), "hello");
    }

    #[test]
    fn truncate_output_at_limit() {
        let data = vec![b'x'; MAX_OUTPUT_BYTES];
        assert_eq!(truncate_output(&data).len(), MAX_OUTPUT_BYTES);
    }

    #[test]
    fn truncate_output_over_limit_keeps_tail() {
        let data: Vec<u8> = (0..MAX_OUTPUT_BYTES + 100)
            .map(|i| b'A' + (i % 26) as u8)
            .collect();
        let result = truncate_output(&data);
        assert_eq!(result.as_bytes(), &data[100..]);
    }

    #[tokio::test]
    async fn hook_success_discards_stdout_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("hooks.log");

        let (_db_dir, backend) = setup_db();
        let task = Task::new(
            1, 1, "Test".into(), None, None, None,
            crate::domain::task::Priority::P2, TaskStatus::Draft,
            None, None,
            "2026-01-01T00:00:00Z".into(), "2026-01-01T00:00:00Z".into(),
            None, None, None, None, None, None, None,
            vec![], vec![], vec![], vec![], vec![],
        );

        let json = serde_json::to_string(&build_event("task_added", &task, &backend, None, None).await).unwrap();
        execute_hook("echo STDOUT_MSG; echo STDERR_MSG >&2; exit 0", "task_added", "eid", "ok-hook", Some(1), &json, Some(&log_path));

        std::thread::sleep(std::time::Duration::from_millis(300));

        let content = std::fs::read_to_string(&log_path).unwrap();
        let line: serde_json::Value = serde_json::from_str(content.lines().next().unwrap()).unwrap();
        assert_eq!(line["type"], "hook_ok");
        assert!(line.get("stdout").is_none() || line["stdout"].is_null(), "stdout should not be in success log");
        assert!(line.get("stderr").is_none() || line["stderr"].is_null(), "stderr should not be in success log");
    }

    #[tokio::test]
    async fn hook_failure_captures_stdout_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("hooks.log");

        let (_db_dir, backend) = setup_db();
        let task = Task::new(
            1, 1, "Test".into(), None, None, None,
            crate::domain::task::Priority::P2, TaskStatus::Draft,
            None, None,
            "2026-01-01T00:00:00Z".into(), "2026-01-01T00:00:00Z".into(),
            None, None, None, None, None, None, None,
            vec![], vec![], vec![], vec![], vec![],
        );

        let json = serde_json::to_string(&build_event("task_added", &task, &backend, None, None).await).unwrap();
        execute_hook("echo STDOUT_MSG; echo STDERR_MSG >&2; exit 1", "task_added", "eid", "fail-hook", Some(1), &json, Some(&log_path));

        std::thread::sleep(std::time::Duration::from_millis(300));

        let content = std::fs::read_to_string(&log_path).unwrap();
        let line: serde_json::Value = serde_json::from_str(content.lines().next().unwrap()).unwrap();
        assert_eq!(line["type"], "hook_failed");
        assert_eq!(line["stdout"].as_str().unwrap().trim(), "STDOUT_MSG");
        assert_eq!(line["stderr"].as_str().unwrap().trim(), "STDERR_MSG");
    }

    #[tokio::test]
    async fn event_fired_logged_with_zero_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().to_str().unwrap().to_string();

        let config = Config {
            log: crate::domain::config::LogConfig {
                dir: Some(log_dir.clone()),
                ..Default::default()
            },
            ..Default::default()
        };

        let (_db_dir, backend) = setup_db();
        let task = Task::new(
            1, 1, "Test".into(), None, None, None,
            crate::domain::task::Priority::P2, TaskStatus::Draft,
            None, None,
            "2026-01-01T00:00:00Z".into(), "2026-01-01T00:00:00Z".into(),
            None, None, None, None, None, None, None,
            vec![], vec![], vec![], vec![], vec![],
        );

        fire_hooks(&config, "task_added", &task, &backend, None, None, &RuntimeMode::Cli, &BackendInfo::Sqlite { db_file_path: "test.db".into() }).await;

        let log_path = dir.path().join("hooks.log");
        let content = std::fs::read_to_string(&log_path).unwrap();
        let line: serde_json::Value = serde_json::from_str(content.lines().next().unwrap()).unwrap();
        assert_eq!(line["type"], "event_fired");
        assert_eq!(line["event"], "task_added");
        assert_eq!(line["task_id"], 1);
        assert_eq!(line["runtime"], "cli");
        assert_eq!(line["backend"]["type"], "sqlite");
        assert_eq!(line["backend"]["db_file_path"], "test.db");
    }

    #[tokio::test]
    async fn event_fired_logged_for_no_eligible_task() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().to_str().unwrap().to_string();

        let config = Config {
            log: crate::domain::config::LogConfig {
                dir: Some(log_dir.clone()),
                ..Default::default()
            },
            ..Default::default()
        };

        let (_db_dir, backend) = setup_db();

        fire_no_eligible_task_hooks(&config, &backend, 1, &RuntimeMode::Cli, &BackendInfo::Sqlite { db_file_path: "test.db".into() }).await;

        let log_path = dir.path().join("hooks.log");
        let content = std::fs::read_to_string(&log_path).unwrap();
        let line: serde_json::Value = serde_json::from_str(content.lines().next().unwrap()).unwrap();
        assert_eq!(line["type"], "event_fired");
        assert_eq!(line["event"], "no_eligible_task");
        assert_eq!(line["runtime"], "cli");
        assert_eq!(line["backend"]["type"], "sqlite");
        assert_eq!(line["backend"]["db_file_path"], "test.db");
    }

    #[tokio::test]
    async fn envelope_serialization() {
        let (_dir, backend) = setup_db();
        let task = Task::new(
            1, 1, "Test".into(), None, None, None,
            crate::domain::task::Priority::P2, TaskStatus::Draft,
            None, None,
            "2026-01-01T00:00:00Z".into(), "2026-01-01T00:00:00Z".into(),
            None, None, None, None, None, None, None,
            vec![], vec![], vec![], vec![], vec![],
        );
        let event = build_event("task_added", &task, &backend, None, None).await;
        let envelope = HookEnvelope {
            runtime: RuntimeMode::Cli,
            backend: BackendInfo::Sqlite { db_file_path: "/tmp/test.db".into() },
            event,
        };
        let json = serde_json::to_string(&envelope).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["runtime"], "cli");
        assert_eq!(v["backend"]["type"], "sqlite");
        assert_eq!(v["backend"]["db_file_path"], "/tmp/test.db");
        assert_eq!(v["event"]["event"], "task_added");
        assert_eq!(v["event"]["task"]["id"], 1);
    }

    #[test]
    fn envelope_no_eligible_task_serialization() {
        let event = NoEligibleTaskEvent {
            event_id: "test-id".into(),
            event: "no_eligible_task".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            stats: HashMap::new(),
            ready_count: 0,
        };
        let envelope = HookEnvelope {
            runtime: RuntimeMode::Api,
            backend: BackendInfo::Http { api_url: "http://localhost:8080".into() },
            event,
        };
        let json = serde_json::to_string(&envelope).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["runtime"], "api");
        assert_eq!(v["backend"]["type"], "http");
        assert_eq!(v["backend"]["api_url"], "http://localhost:8080");
        assert_eq!(v["event"]["event"], "no_eligible_task");
    }

    #[test]
    fn backend_info_serialization_variants() {
        let sqlite = BackendInfo::Sqlite { db_file_path: "/path/db.sqlite".into() };
        let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&sqlite).unwrap()).unwrap();
        assert_eq!(v["type"], "sqlite");
        assert_eq!(v["db_file_path"], "/path/db.sqlite");

        let pg = BackendInfo::Postgresql;
        let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&pg).unwrap()).unwrap();
        assert_eq!(v["type"], "postgresql");

        let ddb = BackendInfo::Dynamodb;
        let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&ddb).unwrap()).unwrap();
        assert_eq!(v["type"], "dynamodb");

        let http = BackendInfo::Http { api_url: "http://example.com".into() };
        let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&http).unwrap()).unwrap();
        assert_eq!(v["type"], "http");
        assert_eq!(v["api_url"], "http://example.com");
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
    fn parse_requires_env_from_toml() {
        let toml_str = r#"
[hooks.on_task_ready.my-hook]
command = "echo ready"
requires_env = ["SENKO_HOST_PROJECT_DIR", "MY_VAR"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let entry = &config.hooks.on_task_ready["my-hook"];
        assert_eq!(entry.requires_env, vec!["SENKO_HOST_PROJECT_DIR", "MY_VAR"]);
    }

    #[test]
    fn parse_requires_env_defaults_to_empty() {
        let toml_str = r#"
[hooks.on_task_ready.my-hook]
command = "echo ready"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let entry = &config.hooks.on_task_ready["my-hook"];
        assert!(entry.requires_env.is_empty());
    }

    #[test]
    fn entries_for_event_returns_enabled_entries() {
        let toml_str = r#"
[hooks.on_task_added.hook1]
command = "echo 1"

[hooks.on_task_added.hook2]
command = "echo 2"
enabled = false

[hooks.on_task_added.hook3]
command = "echo 3"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let entries = config.hooks.entries_for_event("task_added");
        assert_eq!(entries.len(), 2);
        let names: Vec<&str> = entries.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"hook1"));
        assert!(names.contains(&"hook3"));
    }

    #[test]
    fn check_required_env_all_set() {
        let _lock = ENV_MUTEX.lock().unwrap();
        unsafe {
            std::env::set_var("SENKO_TEST_REQENV_A", "val");
        }
        let entry = HookEntry {
            command: "echo test".into(),
            enabled: true,
            requires_env: vec!["SENKO_TEST_REQENV_A".into()],
        };
        let missing = check_required_env(&entry);
        assert!(missing.is_empty());
        unsafe {
            std::env::remove_var("SENKO_TEST_REQENV_A");
        }
    }

    #[test]
    fn check_required_env_missing() {
        let _lock = ENV_MUTEX.lock().unwrap();
        unsafe {
            std::env::remove_var("SENKO_TEST_REQENV_MISSING_1");
            std::env::remove_var("SENKO_TEST_REQENV_MISSING_2");
        }
        let entry = HookEntry {
            command: "echo test".into(),
            enabled: true,
            requires_env: vec![
                "SENKO_TEST_REQENV_MISSING_1".into(),
                "SENKO_TEST_REQENV_MISSING_2".into(),
            ],
        };
        let missing = check_required_env(&entry);
        assert_eq!(missing, vec!["SENKO_TEST_REQENV_MISSING_1", "SENKO_TEST_REQENV_MISSING_2"]);
    }

    #[test]
    fn check_required_env_empty_requires() {
        let entry = HookEntry {
            command: "echo test".into(),
            enabled: true,
            requires_env: vec![],
        };
        let missing = check_required_env(&entry);
        assert!(missing.is_empty());
    }

    #[test]
    fn fire_hooks_skips_hook_with_missing_env() {
        let _lock = ENV_MUTEX.lock().unwrap();
        unsafe {
            std::env::remove_var("SENKO_TEST_FIRE_HOOK_MISSING");
        }

        let tmp = tempfile::tempdir().unwrap();
        let log_path = tmp.path().join("hooks.log");

        let mut on_task_added = std::collections::BTreeMap::new();
        on_task_added.insert(
            "needs-env".to_string(),
            HookEntry {
                command: "echo should-not-run".into(),
                enabled: true,
                requires_env: vec!["SENKO_TEST_FIRE_HOOK_MISSING".into()],
            },
        );

        let config = Config {
            hooks: HooksConfig {
                on_task_added,
                ..Default::default()
            },
            log: crate::domain::config::LogConfig {
                dir: Some(tmp.path().to_str().unwrap().to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let entries = config.hooks.entries_for_event("task_added");
        assert_eq!(entries.len(), 1);

        // Simulate the env check that fire_hooks performs
        let (name, entry) = &entries[0];
        let missing = check_required_env(entry);
        assert!(!missing.is_empty());

        let msg = format!(
            "hook skipped (task_added): {} — missing env: {}",
            name,
            missing.join(", ")
        );
        let log_entry = HookLogEntry::new("WARN", "hook_skipped")
            .with_event("task_added")
            .with_hook(name)
            .with_message(&msg);
        log_to_file(&log_path, &log_entry);

        let log_content = std::fs::read_to_string(&log_path).unwrap();
        let line: serde_json::Value = serde_json::from_str(log_content.lines().next().unwrap()).unwrap();
        assert_eq!(line["type"], "hook_skipped");
        assert_eq!(line["level"], "WARN");
        assert!(line["message"].as_str().unwrap().contains("hook skipped (task_added): needs-env"));
        assert!(line["message"].as_str().unwrap().contains("SENKO_TEST_FIRE_HOOK_MISSING"));
    }

    #[test]
    fn get_commands_for_event_skips_missing_env() {
        let _lock = ENV_MUTEX.lock().unwrap();
        unsafe {
            std::env::remove_var("SENKO_TEST_GET_CMD_MISSING");
            std::env::set_var("SENKO_TEST_GET_CMD_PRESENT", "val");
        }

        let mut on_task_added = std::collections::BTreeMap::new();
        on_task_added.insert(
            "needs-env".to_string(),
            HookEntry {
                command: "echo skip".into(),
                enabled: true,
                requires_env: vec!["SENKO_TEST_GET_CMD_MISSING".into()],
            },
        );
        on_task_added.insert(
            "has-env".to_string(),
            HookEntry {
                command: "echo run".into(),
                enabled: true,
                requires_env: vec!["SENKO_TEST_GET_CMD_PRESENT".into()],
            },
        );
        on_task_added.insert(
            "no-req".to_string(),
            HookEntry {
                command: "echo always".into(),
                enabled: true,
                requires_env: vec![],
            },
        );

        let config = Config {
            hooks: HooksConfig {
                on_task_added,
                ..Default::default()
            },
            ..Default::default()
        };

        let commands = get_commands_for_event(&config, "task_added").unwrap();
        assert_eq!(commands.len(), 2);
        assert!(commands.contains(&"echo run".to_string()));
        assert!(commands.contains(&"echo always".to_string()));
        assert!(!commands.contains(&"echo skip".to_string()));

        unsafe {
            std::env::remove_var("SENKO_TEST_GET_CMD_PRESENT");
        }
    }

    #[test]
    fn legacy_hook_format_rejected() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let senko_dir = dir.path().join(".senko");
        std::fs::create_dir_all(&senko_dir).unwrap();
        std::fs::write(
            senko_dir.join("config.toml"),
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
        on_task_added.insert("capture".to_string(), HookEntry { command: cmd, enabled: true, requires_env: vec![] });
        let config = Config {
            hooks: HooksConfig {
                on_task_added,
                ..Default::default()
            },
            ..Default::default()
        };

        let (_db_dir, backend) = setup_db();
        let task = Task::new(
            42, 1, "Hook stdin test".into(), None, None, None,
            crate::domain::task::Priority::P1, TaskStatus::Draft,
            None, None,
            "2026-01-01T00:00:00Z".into(), "2026-01-01T00:00:00Z".into(),
            None, None, None, None, None, None, None,
            vec![], vec![], vec![], vec![], vec![],
        );
        fire_hooks(&config, "task_added", &task, &backend, None, None, &RuntimeMode::Cli, &BackendInfo::Sqlite { db_file_path: "test.db".into() }).await;

        std::thread::sleep(std::time::Duration::from_millis(200));

        let content = std::fs::read_to_string(&output_file).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();
        // Envelope wraps the event
        assert_eq!(json["runtime"], "cli");
        assert_eq!(json["backend"]["type"], "sqlite");
        assert_eq!(json["backend"]["db_file_path"], "test.db");
        assert_eq!(json["event"]["event"], "task_added");
        assert_eq!(json["event"]["task"]["id"], 42);
        assert_eq!(json["event"]["task"]["title"], "Hook stdin test");
    }

    #[test]
    fn env_override_completion_mode() {
        let _lock = ENV_MUTEX.lock().unwrap();
        unsafe {
            let orig = std::env::var("SENKO_COMPLETION_MODE").ok();
            std::env::set_var("SENKO_COMPLETION_MODE", "pr_then_complete");
            let mut config = Config::default();
            config.apply_env();
            assert_eq!(config.workflow.completion_mode, CompletionMode::PrThenComplete);
            match orig {
                Some(v) => std::env::set_var("SENKO_COMPLETION_MODE", v),
                None => std::env::remove_var("SENKO_COMPLETION_MODE"),
            }
        }
    }

    #[test]
    fn env_override_auto_merge() {
        let _lock = ENV_MUTEX.lock().unwrap();
        unsafe {
            let orig = std::env::var("SENKO_AUTO_MERGE").ok();
            std::env::set_var("SENKO_AUTO_MERGE", "false");
            let mut config = Config::default();
            config.apply_env();
            assert!(!config.workflow.auto_merge);
            std::env::set_var("SENKO_AUTO_MERGE", "0");
            let mut config = Config::default();
            config.apply_env();
            assert!(!config.workflow.auto_merge);
            match orig {
                Some(v) => std::env::set_var("SENKO_AUTO_MERGE", v),
                None => std::env::remove_var("SENKO_AUTO_MERGE"),
            }
        }
    }

    #[test]
    fn env_override_hook_mode() {
        let _lock = ENV_MUTEX.lock().unwrap();
        unsafe {
            let orig = std::env::var("SENKO_HOOK_MODE").ok();
            std::env::set_var("SENKO_HOOK_MODE", "client");
            let mut config = Config::default();
            config.apply_env();
            assert_eq!(config.backend.hook_mode, HookMode::Client);
            std::env::set_var("SENKO_HOOK_MODE", "both");
            let mut config = Config::default();
            config.apply_env();
            assert_eq!(config.backend.hook_mode, HookMode::Both);
            match orig {
                Some(v) => std::env::set_var("SENKO_HOOK_MODE", v),
                None => std::env::remove_var("SENKO_HOOK_MODE"),
            }
        }
    }

    #[test]
    fn env_override_api_url() {
        let _lock = ENV_MUTEX.lock().unwrap();
        unsafe {
            let orig = std::env::var("SENKO_API_URL").ok();
            std::env::set_var("SENKO_API_URL", "http://remote:3142");
            let mut config = Config::default();
            config.apply_env();
            assert_eq!(config.backend.api_url, Some("http://remote:3142".to_string()));
            match orig {
                Some(v) => std::env::set_var("SENKO_API_URL", v),
                None => std::env::remove_var("SENKO_API_URL"),
            }
        }
    }

    #[test]
    fn env_override_hooks_insert() {
        let _lock = ENV_MUTEX.lock().unwrap();
        unsafe {
            let orig = std::env::var("SENKO_HOOK_ON_TASK_ADDED").ok();
            std::env::set_var("SENKO_HOOK_ON_TASK_ADDED", "env-hook");
            // Start with a config that already has a hook from config.toml
            let mut config = Config::default();
            config.hooks.on_task_added.insert(
                "toml-hook".to_string(),
                HookEntry { command: "toml-hook".into(), enabled: true, requires_env: vec![] },
            );
            config.apply_env();
            assert_eq!(config.hooks.on_task_added.len(), 2);
            assert_eq!(config.hooks.on_task_added["toml-hook"].command, "toml-hook");
            assert_eq!(config.hooks.on_task_added["_env"].command, "env-hook");
            match orig {
                Some(v) => std::env::set_var("SENKO_HOOK_ON_TASK_ADDED", v),
                None => std::env::remove_var("SENKO_HOOK_ON_TASK_ADDED"),
            }
        }
    }

    #[test]
    fn env_override_empty_values_ignored() {
        let _lock = ENV_MUTEX.lock().unwrap();
        unsafe {
            let orig_url = std::env::var("SENKO_API_URL").ok();
            let orig_hook = std::env::var("SENKO_HOOK_ON_TASK_ADDED").ok();
            std::env::set_var("SENKO_API_URL", "");
            std::env::set_var("SENKO_HOOK_ON_TASK_ADDED", "");
            let mut config = Config::default();
            config.apply_env();
            assert_eq!(config.backend.api_url, None);
            assert!(config.hooks.on_task_added.is_empty());
            match orig_url {
                Some(v) => std::env::set_var("SENKO_API_URL", v),
                None => std::env::remove_var("SENKO_API_URL"),
            }
            match orig_hook {
                Some(v) => std::env::set_var("SENKO_HOOK_ON_TASK_ADDED", v),
                None => std::env::remove_var("SENKO_HOOK_ON_TASK_ADDED"),
            }
        }
    }

    #[test]
    fn load_config_no_file_with_env_overrides() {
        let _lock = ENV_MUTEX.lock().unwrap();
        with_isolated_user_config(|| {
            unsafe {
                let orig = std::env::var("SENKO_COMPLETION_MODE").ok();
                std::env::set_var("SENKO_COMPLETION_MODE", "pr_then_complete");
                let tmp = tempfile::tempdir().unwrap();
                let config = load_config(tmp.path(), None).unwrap();
                assert_eq!(config.workflow.completion_mode, CompletionMode::PrThenComplete);
                match orig {
                    Some(v) => std::env::set_var("SENKO_COMPLETION_MODE", v),
                    None => std::env::remove_var("SENKO_COMPLETION_MODE"),
                }
            }
        });
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
        with_isolated_user_config(|| {
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
                let orig = std::env::var("SENKO_CONFIG").ok();
                std::env::set_var("SENKO_CONFIG", config_file.to_str().unwrap());
                let config = load_config(tmp.path(), None).unwrap();
                assert!(!config.workflow.auto_merge);
                match orig {
                    Some(v) => std::env::set_var("SENKO_CONFIG", v),
                    None => std::env::remove_var("SENKO_CONFIG"),
                }
            }
        });
    }

    #[test]
    fn load_config_explicit_overrides_env_var() {
        let _lock = ENV_MUTEX.lock().unwrap();
        with_isolated_user_config(|| {
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
                let orig = std::env::var("SENKO_CONFIG").ok();
                std::env::set_var("SENKO_CONFIG", env_config.to_str().unwrap());
                let config = load_config(tmp.path(), Some(&cli_config)).unwrap();
                // CLI flag should take priority over env var
                assert!(!config.workflow.auto_merge);
                match orig {
                    Some(v) => std::env::set_var("SENKO_CONFIG", v),
                    None => std::env::remove_var("SENKO_CONFIG"),
                }
            }
        });
    }

    #[test]
    fn load_config_env_var_not_found() {
        let _lock = ENV_MUTEX.lock().unwrap();
        with_isolated_user_config(|| {
            let tmp = tempfile::tempdir().unwrap();
            unsafe {
                let orig = std::env::var("SENKO_CONFIG").ok();
                std::env::set_var("SENKO_CONFIG", "/nonexistent/path/config.toml");
                let result = load_config(tmp.path(), None);
                assert!(result.is_err());
                assert!(
                    result.unwrap_err().to_string().contains("config file not found"),
                    "should report missing config file from env var"
                );
                match orig {
                    Some(v) => std::env::set_var("SENKO_CONFIG", v),
                    None => std::env::remove_var("SENKO_CONFIG"),
                }
            }
        });
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
            let orig = std::env::var("SENKO_LOG_DIR").ok();
            std::env::set_var("SENKO_LOG_DIR", "/tmp/custom-logs");
            let mut config = Config::default();
            config.apply_env();
            assert_eq!(config.log.dir, Some("/tmp/custom-logs".into()));
            match orig {
                Some(v) => std::env::set_var("SENKO_LOG_DIR", v),
                None => std::env::remove_var("SENKO_LOG_DIR"),
            }
        }
    }

    #[test]
    fn env_override_log_dir_empty_ignored() {
        unsafe {
            let orig = std::env::var("SENKO_LOG_DIR").ok();
            std::env::set_var("SENKO_LOG_DIR", "");
            let mut config = Config::default();
            config.apply_env();
            assert_eq!(config.log.dir, None);
            match orig {
                Some(v) => std::env::set_var("SENKO_LOG_DIR", v),
                None => std::env::remove_var("SENKO_LOG_DIR"),
            }
        }
    }

    #[test]
    fn log_config_deserialization() {
        let toml_str = r#"
[log]
dir = "/var/log/senko"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.log.dir, Some("/var/log/senko".into()));
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
            HookEntry { command: "user-cmd".into(), enabled: true, requires_env: vec![] },
        );
        base_hooks.on_task_completed.insert(
            "shared".to_string(),
            HookEntry { command: "user-completed".into(), enabled: true, requires_env: vec![] },
        );

        let mut overlay_hooks = HooksConfig::default();
        overlay_hooks.on_task_added.insert(
            "project-hook".to_string(),
            HookEntry { command: "project-cmd".into(), enabled: true, requires_env: vec![] },
        );
        // Override the shared hook
        overlay_hooks.on_task_completed.insert(
            "shared".to_string(),
            HookEntry { command: "project-completed".into(), enabled: true, requires_env: vec![] },
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
            HookEntry { command: "notify-cmd".into(), enabled: true, requires_env: vec![] },
        );

        let mut overlay_hooks = HooksConfig::default();
        overlay_hooks.on_task_added.insert(
            "notify".to_string(),
            HookEntry { command: "".into(), enabled: false, requires_env: vec![] },
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
        let user_config_dir = tmp.path().join("user-config").join("senko");
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
        std::fs::create_dir_all(project_dir.join(".senko")).unwrap();

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
        let user_config_dir = tmp.path().join("user-config").join("senko");
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
        let senko_dir = project_dir.join(".senko");
        std::fs::create_dir_all(&senko_dir).unwrap();
        std::fs::write(
            senko_dir.join("config.toml"),
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
