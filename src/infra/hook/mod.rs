pub mod executor;
pub mod test_executor;

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;

use crate::application::HookTrigger;
use crate::application::hook_trigger::SelectResult;
use crate::application::port::HookDataSource;
use crate::domain::contract::Contract;
use crate::domain::task::{self, Task, TaskStatus, UnblockedTask};
use crate::infra::config::{
    ActionConfig, Config, ContractActionHooks, HookDef, HookMode, HookOutput, HookWhen, OnFailure,
    OnResult, TaskActionHooks,
};
use crate::infra::xdg::XdgDirs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMode {
    Cli,
    ServerRelay,
    ServerRemote,
}

impl RuntimeMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            RuntimeMode::Cli => "cli",
            RuntimeMode::ServerRelay => "server.relay",
            RuntimeMode::ServerRemote => "server.remote",
        }
    }

    fn section_label(&self) -> &'static str {
        match self {
            RuntimeMode::Cli => "cli",
            RuntimeMode::ServerRelay => "server.relay",
            RuntimeMode::ServerRemote => "server.remote",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackendInfo {
    Sqlite { db_file_path: String },
    Postgresql,
    Http { api_url: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct EnvelopeProjectInfo {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnvelopeUserInfo {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct HookEnvelope<T: Serialize> {
    pub runtime: RuntimeMode,
    pub backend: BackendInfo,
    pub project: EnvelopeProjectInfo,
    pub user: EnvelopeUserInfo,
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
    /// Whether `task` is currently in the "ready to be worked on" state
    /// (status == todo AND every dependency completed). Computed via
    /// `HookDataSource::is_task_ready`, which mirrors `Task::is_ready`.
    pub is_ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unblocked_tasks: Option<Vec<UnblockedTask>>,
}

#[derive(Debug, Serialize)]
pub struct TaskSelectEvent {
    pub event_id: String,
    pub event: String,
    pub timestamp: String,
    pub result: String,
    pub stats: HashMap<String, i64>,
    pub ready_count: i64,
}

#[derive(Debug, Serialize)]
pub struct ContractHookEvent {
    pub event_id: String,
    pub event: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract: Option<Contract>,
}

/// Outcome of firing a batch of hooks. Pre+Sync+Abort failures return `Abort`
/// so the caller (task_service) can skip the state transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum FireOutcome {
    Continue,
    Abort,
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

pub async fn build_event(
    event_name: &str,
    task: &Task,
    backend: &dyn HookDataSource,
    from_status: Option<TaskStatus>,
    unblocked: Option<Vec<UnblockedTask>>,
) -> HookEvent {
    let stats = backend
        .task_stats(task.project_id())
        .await
        .unwrap_or_default();
    let ready_count = backend.ready_count(task.project_id()).await.unwrap_or(0);
    let is_ready = backend
        .is_task_ready(task.project_id(), task.id())
        .await
        .unwrap_or(false);
    HookEvent {
        event_id: Uuid::new_v4().to_string(),
        event: event_name.into(),
        timestamp: Utc::now().to_rfc3339(),
        task: task.clone(),
        stats,
        ready_count,
        is_ready,
        from_status: from_status.map(|s| s.to_string()),
        unblocked_tasks: unblocked,
    }
}

pub async fn build_task_select_event(
    result: SelectResult,
    backend: &dyn HookDataSource,
    project_id: i64,
) -> TaskSelectEvent {
    let stats = backend.task_stats(project_id).await.unwrap_or_default();
    let ready_count = backend.ready_count(project_id).await.unwrap_or(0);
    TaskSelectEvent {
        event_id: Uuid::new_v4().to_string(),
        event: "task_select".into(),
        timestamp: Utc::now().to_rfc3339(),
        result: result.as_str().to_string(),
        stats,
        ready_count,
    }
}

pub fn build_contract_event(event_name: &str, contract: Option<&Contract>) -> ContractHookEvent {
    ContractHookEvent {
        event_id: Uuid::new_v4().to_string(),
        event: event_name.into(),
        timestamp: Utc::now().to_rfc3339(),
        contract: contract.cloned(),
    }
}

/// Return the hook log file path, optionally using a custom log directory.
/// Priority: `log_dir` override > `$XDG_STATE_HOME/senko` > `~/.local/state/senko`
pub fn log_file_path_with_dir(log_dir: Option<&str>, xdg: &XdgDirs) -> Option<PathBuf> {
    let dir = if let Some(d) = log_dir {
        PathBuf::from(d)
    } else {
        xdg.state_home.as_ref()?.join("senko")
    };
    Some(dir.join("hooks.log"))
}

/// Return the hook log file path following XDG Base Directory specification.
/// `$XDG_STATE_HOME/senko/hooks.log` (default: `~/.local/state/senko/hooks.log`)
pub fn log_file_path(xdg: &XdgDirs) -> Option<PathBuf> {
    log_file_path_with_dir(None, xdg)
}

fn log_to_file(path: &Path, entry: &HookLogEntry) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        && let Ok(json) = serde_json::to_string(entry)
    {
        let mut line = json;
        line.push('\n');
        let _ = f.write_all(line.as_bytes());
    }
}

fn log_to_stdout(entry: &HookLogEntry) {
    if let Ok(json) = serde_json::to_string(entry) {
        println!("{json}");
    }
}

#[derive(Clone)]
pub(crate) struct HookLogTarget {
    pub output: HookOutput,
    pub file_path: Option<PathBuf>,
}

fn write_hook_log(target: &HookLogTarget, entry: &HookLogEntry) {
    match target.output {
        HookOutput::File => {
            if let Some(ref p) = target.file_path {
                log_to_file(p, entry);
            }
        }
        HookOutput::Stdout => {
            log_to_stdout(entry);
        }
        HookOutput::Both => {
            if let Some(ref p) = target.file_path {
                log_to_file(p, entry);
            }
            log_to_stdout(entry);
        }
    }
}

/// Run a hook command with the given env map and JSON stdin.
/// When `sync` is true, wait for the child and return the exit status.
/// When false, spawn a background thread that logs the outcome and return `None`.
#[allow(clippy::too_many_arguments)]
fn run_hook_command(
    command: &str,
    event_name: &str,
    event_id: &str,
    hook_name: &str,
    task_id: Option<i64>,
    json: &str,
    env_vars: &HashMap<String, String>,
    sync: bool,
    log_target: Option<&HookLogTarget>,
) -> Option<std::process::ExitStatus> {
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c").arg(command);
    for (k, v) in env_vars {
        cmd.env(k, v);
    }
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("hook spawn error ({}): {}: {:#}", event_name, command, e);
            eprintln!("{msg}");
            if let Some(t) = log_target {
                let entry = HookLogEntry::new("ERROR", "hook_error")
                    .with_event_id(event_id)
                    .with_event(event_name)
                    .with_hook(hook_name)
                    .with_command(command)
                    .with_task_id(task_id)
                    .with_message(&msg);
                write_hook_log(t, &entry);
            }
            return None;
        }
    };

    if let Some(mut stdin) = child.stdin.take()
        && let Err(e) = stdin.write_all(json.as_bytes())
    {
        let msg = format!("hook stdin error ({}): {}: {:#}", event_name, command, e);
        eprintln!("{msg}");
        if let Some(t) = log_target {
            let entry = HookLogEntry::new("ERROR", "hook_error")
                .with_event_id(event_id)
                .with_event(event_name)
                .with_hook(hook_name)
                .with_command(command)
                .with_task_id(task_id)
                .with_message(&msg);
            write_hook_log(t, &entry);
        }
        return None;
    }

    if sync {
        match child.wait_with_output() {
            Ok(output) => {
                log_hook_outcome(
                    log_target, event_name, event_id, hook_name, command, task_id, &output,
                );
                Some(output.status)
            }
            Err(e) => {
                let msg = format!("hook wait error ({}): {}: {:#}", event_name, command, e);
                eprintln!("{msg}");
                if let Some(t) = log_target {
                    let entry = HookLogEntry::new("ERROR", "hook_error")
                        .with_event_id(event_id)
                        .with_event(event_name)
                        .with_hook(hook_name)
                        .with_command(command)
                        .with_task_id(task_id)
                        .with_message(&msg);
                    write_hook_log(t, &entry);
                }
                None
            }
        }
    } else {
        let cmd_s = command.to_owned();
        let evt = event_name.to_owned();
        let eid = event_id.to_owned();
        let hname = hook_name.to_owned();
        let tid = task_id;
        let log = log_target.cloned();
        std::thread::spawn(move || match child.wait_with_output() {
            Ok(output) => {
                log_hook_outcome(log.as_ref(), &evt, &eid, &hname, &cmd_s, tid, &output);
            }
            Err(e) => {
                let msg = format!("hook wait error ({}): {}: {:#}", evt, cmd_s, e);
                eprintln!("{msg}");
                if let Some(ref t) = log {
                    let entry = HookLogEntry::new("ERROR", "hook_error")
                        .with_event_id(&eid)
                        .with_event(&evt)
                        .with_hook(&hname)
                        .with_command(&cmd_s)
                        .with_task_id(tid)
                        .with_message(&msg);
                    write_hook_log(t, &entry);
                }
            }
        });
        None
    }
}

fn log_hook_outcome(
    log_target: Option<&HookLogTarget>,
    event_name: &str,
    event_id: &str,
    hook_name: &str,
    command: &str,
    task_id: Option<i64>,
    output: &std::process::Output,
) {
    if output.status.success() {
        if let Some(t) = log_target {
            let entry = HookLogEntry::new("INFO", "hook_ok")
                .with_event_id(event_id)
                .with_event(event_name)
                .with_hook(hook_name)
                .with_command(command)
                .with_task_id(task_id)
                .with_exit_code(output.status.code());
            write_hook_log(t, &entry);
        }
    } else {
        let msg = format!(
            "hook failed ({}): {} (exit: {})",
            event_name,
            command,
            output
                .status
                .code()
                .map_or("signal".to_string(), |c| c.to_string())
        );
        eprintln!("{msg}");
        if let Some(t) = log_target {
            let mut entry = HookLogEntry::new("WARN", "hook_failed")
                .with_event_id(event_id)
                .with_event(event_name)
                .with_hook(hook_name)
                .with_command(command)
                .with_task_id(task_id)
                .with_exit_code(output.status.code());
            if !output.stdout.is_empty() {
                entry.stdout = Some(truncate_output(&output.stdout));
            }
            if !output.stderr.is_empty() {
                entry.stderr = Some(truncate_output(&output.stderr));
            }
            write_hook_log(t, &entry);
        }
    }
}

/// Resolve the environment map for a hook invocation based on its `env_vars` spec.
/// Returns `Err(missing_var_name)` if a required variable is unset and has no default.
fn resolve_env_vars(hook: &HookDef) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();
    for spec in &hook.env_vars {
        let current = std::env::var(&spec.name).ok();
        if let Some(v) = current {
            map.insert(spec.name.clone(), v);
        } else if let Some(ref def) = spec.default {
            map.insert(spec.name.clone(), def.clone());
        } else if spec.required {
            return Err(spec.name.clone());
        }
    }
    Ok(map)
}

use crate::domain::{DEFAULT_PROJECT_ID, DEFAULT_USER_ID};

pub async fn resolve_envelope_context(
    config: &Config,
    backend: &dyn HookDataSource,
) -> (EnvelopeProjectInfo, EnvelopeUserInfo) {
    let project = match config.project.name.as_deref() {
        Some(name) => backend
            .get_project_by_name(name)
            .await
            .map(|p| EnvelopeProjectInfo {
                id: p.id(),
                name: p.name().to_owned(),
            })
            .unwrap_or_else(|_| EnvelopeProjectInfo {
                id: DEFAULT_PROJECT_ID,
                name: "default".into(),
            }),
        None => backend
            .get_project(DEFAULT_PROJECT_ID)
            .await
            .map(|p| EnvelopeProjectInfo {
                id: p.id(),
                name: p.name().to_owned(),
            })
            .unwrap_or_else(|_| EnvelopeProjectInfo {
                id: DEFAULT_PROJECT_ID,
                name: "default".into(),
            }),
    };
    let user = match config.user.name.as_deref() {
        Some(name) => backend
            .get_user_by_username(name)
            .await
            .map(|u| EnvelopeUserInfo {
                id: u.id(),
                name: u.username().to_owned(),
            })
            .unwrap_or_else(|_| EnvelopeUserInfo {
                id: DEFAULT_USER_ID,
                name: "default".into(),
            }),
        None => backend
            .get_user(DEFAULT_USER_ID)
            .await
            .map(|u| EnvelopeUserInfo {
                id: u.id(),
                name: u.username().to_owned(),
            })
            .unwrap_or_else(|_| EnvelopeUserInfo {
                id: DEFAULT_USER_ID,
                name: "default".into(),
            }),
    };
    (project, user)
}

/// Pick the `TaskActionHooks` section belonging to the currently-running runtime.
fn hooks_for_runtime<'a>(config: &'a Config, runtime: &RuntimeMode) -> &'a TaskActionHooks {
    match runtime {
        RuntimeMode::Cli => &config.cli.hooks,
        RuntimeMode::ServerRelay => &config.server.relay.hooks,
        RuntimeMode::ServerRemote => &config.server.remote.hooks,
    }
}

/// Pick the `ContractActionHooks` section for the currently-running runtime.
fn contract_hooks_for_runtime<'a>(
    config: &'a Config,
    runtime: &RuntimeMode,
) -> &'a ContractActionHooks {
    match runtime {
        RuntimeMode::Cli => &config.cli.contract_hooks,
        RuntimeMode::ServerRelay => &config.server.relay.contract_hooks,
        RuntimeMode::ServerRemote => &config.server.remote.contract_hooks,
    }
}

/// Filter a single hook by `when` and `on_result`.
/// `trigger_result` is `Some(result)` only for `HookTrigger::TaskSelect`; other
/// triggers are treated as matching `OnResult::Any`.
fn hook_applies(hook: &HookDef, when: HookWhen, trigger_result: Option<SelectResult>) -> bool {
    if !hook.enabled {
        return false;
    }
    if hook.when != when {
        return false;
    }
    if let Some(expected) = hook.on_result {
        match (expected, trigger_result) {
            (OnResult::Any, _) => {}
            (OnResult::Selected, Some(SelectResult::Selected)) => {}
            (OnResult::None, Some(SelectResult::None)) => {}
            (_, _) => return false,
        }
    }
    true
}

/// Resolve the action key from the trigger (used to pick CLI/server action hooks).
fn action_for_trigger(trigger: &HookTrigger) -> Option<&'static str> {
    trigger.event_name()
}

/// Run a precomputed list of applicable hooks against a pre-serialized envelope.
/// Shared between task and contract dispatch paths.
#[allow(clippy::too_many_arguments)]
fn execute_hook_batch(
    applicable: Vec<(String, HookDef)>,
    envelope_json: &str,
    envelope_event_id: &str,
    event_name: &str,
    task_id_for_log: Option<i64>,
    when: HookWhen,
    runtime_mode: &RuntimeMode,
    backend_info: &BackendInfo,
    log_target: &HookLogTarget,
) -> FireOutcome {
    if applicable.is_empty() {
        return FireOutcome::Continue;
    }

    let mut outcome = FireOutcome::Continue;
    for (name, hook) in applicable {
        let env_map = match resolve_env_vars(&hook) {
            Ok(m) => m,
            Err(missing) => {
                let msg = format!(
                    "hook skipped ({}): {} — missing required env: {}",
                    event_name, name, missing
                );
                eprintln!("{msg}");
                let entry = HookLogEntry::new("WARN", "hook_skipped")
                    .with_event_id(envelope_event_id)
                    .with_event(event_name)
                    .with_hook(&name)
                    .with_command(&hook.command)
                    .with_task_id(task_id_for_log)
                    .with_runtime(runtime_mode.as_str())
                    .with_backend(backend_info)
                    .with_message(&msg);
                write_hook_log(log_target, &entry);
                continue;
            }
        };

        let sync = hook.mode == HookMode::Sync;
        let status = run_hook_command(
            &hook.command,
            event_name,
            envelope_event_id,
            &name,
            task_id_for_log,
            envelope_json,
            &env_map,
            sync,
            Some(log_target),
        );

        if sync {
            let failed = status.map(|s| !s.success()).unwrap_or(true);
            if failed {
                match hook.on_failure {
                    OnFailure::Abort => {
                        if when == HookWhen::Pre {
                            outcome = FireOutcome::Abort;
                            tracing::warn!(
                                hook = %name,
                                event = %event_name,
                                "hook failed with on_failure=abort; aborting transition"
                            );
                            return outcome;
                        } else {
                            tracing::warn!(
                                hook = %name,
                                event = %event_name,
                                "post-hook failed with on_failure=abort (no-op; abort only applies to sync+pre)"
                            );
                        }
                    }
                    OnFailure::Warn => {
                        tracing::warn!(
                            hook = %name,
                            event = %event_name,
                            "hook failed (on_failure=warn, continuing)"
                        );
                    }
                    OnFailure::Ignore => {}
                }
            }
        }
    }

    outcome
}

/// Fire all hooks matching the given trigger + timing for the current runtime.
#[allow(clippy::too_many_arguments)]
pub async fn fire(
    config: &Config,
    trigger: &HookTrigger,
    when: HookWhen,
    task: Option<&Task>,
    backend: &dyn HookDataSource,
    from_status: Option<TaskStatus>,
    unblocked: Option<Vec<UnblockedTask>>,
    runtime_mode: &RuntimeMode,
    backend_info: &BackendInfo,
) -> FireOutcome {
    let Some(event_name) = trigger.event_name() else {
        return FireOutcome::Continue;
    };
    let Some(action_key) = action_for_trigger(trigger) else {
        return FireOutcome::Continue;
    };

    let runtime_hooks = hooks_for_runtime(config, runtime_mode);
    let Some(action) = runtime_hooks.action_config(action_key) else {
        return FireOutcome::Continue;
    };

    let trigger_result = match trigger {
        HookTrigger::TaskSelect { result, .. } => Some(*result),
        _ => None,
    };

    let applicable: Vec<(String, HookDef)> = action
        .hooks
        .iter()
        .filter(|(_, def)| hook_applies(def, when, trigger_result))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let log_target = HookLogTarget {
        output: config.log.hook_output,
        file_path: log_file_path_with_dir(config.log.dir.as_deref(), &config.xdg),
    };

    let (envelope_project, envelope_user) = resolve_envelope_context(config, backend).await;

    // Build and serialize the envelope (shape depends on trigger kind).
    let (envelope_json, envelope_event_id, task_id_for_log) = match trigger {
        HookTrigger::Task(_) => {
            let Some(task) = task else {
                return FireOutcome::Continue;
            };
            let event = build_event(event_name, task, backend, from_status, unblocked).await;
            let event_id = event.event_id.clone();
            let envelope = HookEnvelope {
                runtime: *runtime_mode,
                backend: backend_info.clone(),
                project: envelope_project,
                user: envelope_user,
                event,
            };
            match serde_json::to_string(&envelope) {
                Ok(s) => (s, event_id, Some(task.id().into())),
                Err(e) => {
                    eprintln!("hook serialize error ({event_name}): {e}");
                    return FireOutcome::Continue;
                }
            }
        }
        HookTrigger::TaskSelect { project_id, result } => {
            let event = build_task_select_event(*result, backend, *project_id).await;
            let event_id = event.event_id.clone();
            let envelope = HookEnvelope {
                runtime: *runtime_mode,
                backend: backend_info.clone(),
                project: envelope_project,
                user: envelope_user,
                event,
            };
            match serde_json::to_string(&envelope) {
                Ok(s) => (s, event_id, None),
                Err(e) => {
                    eprintln!("hook serialize error ({event_name}): {e}");
                    return FireOutcome::Continue;
                }
            }
        }
        HookTrigger::Contract(_) => {
            // `fire()` is task-scoped; contract triggers must use `fire_contract()`.
            return FireOutcome::Continue;
        }
    };

    // Log a single `event_fired` entry even when no hooks match.
    {
        let entry = HookLogEntry::new("INFO", "event_fired")
            .with_event_id(&envelope_event_id)
            .with_event(event_name)
            .with_task_id(task_id_for_log)
            .with_runtime(runtime_mode.as_str())
            .with_backend(backend_info);
        write_hook_log(&log_target, &entry);
    }

    execute_hook_batch(
        applicable,
        &envelope_json,
        &envelope_event_id,
        event_name,
        task_id_for_log,
        when,
        runtime_mode,
        backend_info,
        &log_target,
    )
}

/// Fire contract-aggregate hooks matching the given trigger + timing for the
/// current runtime. Mirrors `fire()` but dispatches against
/// `ContractActionHooks` and serializes a contract-shaped envelope.
pub async fn fire_contract(
    config: &Config,
    trigger: &HookTrigger,
    when: HookWhen,
    contract: Option<&Contract>,
    backend: &dyn HookDataSource,
    runtime_mode: &RuntimeMode,
    backend_info: &BackendInfo,
) -> FireOutcome {
    let Some(event_name) = trigger.event_name() else {
        return FireOutcome::Continue;
    };
    // Only Contract triggers are valid here; other kinds are a no-op.
    if !matches!(trigger, HookTrigger::Contract(_)) {
        return FireOutcome::Continue;
    }

    let runtime_hooks = contract_hooks_for_runtime(config, runtime_mode);
    let Some(action) = runtime_hooks.action_config(event_name) else {
        return FireOutcome::Continue;
    };

    let applicable: Vec<(String, HookDef)> = action
        .hooks
        .iter()
        .filter(|(_, def)| hook_applies(def, when, None))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let log_target = HookLogTarget {
        output: config.log.hook_output,
        file_path: log_file_path_with_dir(config.log.dir.as_deref(), &config.xdg),
    };

    let (envelope_project, envelope_user) = resolve_envelope_context(config, backend).await;

    let event = build_contract_event(event_name, contract);
    let envelope_event_id = event.event_id.clone();
    let envelope = HookEnvelope {
        runtime: *runtime_mode,
        backend: backend_info.clone(),
        project: envelope_project,
        user: envelope_user,
        event,
    };
    let envelope_json = match serde_json::to_string(&envelope) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("hook serialize error ({event_name}): {e}");
            return FireOutcome::Continue;
        }
    };

    // Log a single `event_fired` entry even when no hooks match.
    {
        let entry = HookLogEntry::new("INFO", "event_fired")
            .with_event_id(&envelope_event_id)
            .with_event(event_name)
            .with_runtime(runtime_mode.as_str())
            .with_backend(backend_info);
        write_hook_log(&log_target, &entry);
    }

    execute_hook_batch(
        applicable,
        &envelope_json,
        &envelope_event_id,
        event_name,
        None,
        when,
        runtime_mode,
        backend_info,
        &log_target,
    )
}

/// Warn once per process if the loaded config has hook definitions in runtime
/// sections that do not match the current runtime.
pub fn warn_about_mismatched_runtime_sections(config: &Config, runtime: &RuntimeMode) {
    static FIRED: OnceLock<()> = OnceLock::new();
    if FIRED.get().is_some() {
        return;
    }
    let _ = FIRED.set(());

    let active = runtime.section_label();
    let mut mismatched: Vec<&str> = Vec::new();
    if !matches!(runtime, RuntimeMode::Cli)
        && (!config.cli.hooks.is_empty() || !config.cli.contract_hooks.is_empty())
    {
        mismatched.push("cli");
    }
    if !matches!(runtime, RuntimeMode::ServerRelay)
        && (!config.server.relay.hooks.is_empty() || !config.server.relay.contract_hooks.is_empty())
    {
        mismatched.push("server.relay");
    }
    if !matches!(runtime, RuntimeMode::ServerRemote)
        && (!config.server.remote.hooks.is_empty()
            || !config.server.remote.contract_hooks.is_empty())
    {
        mismatched.push("server.remote");
    }
    if !mismatched.is_empty() {
        tracing::warn!(
            active = active,
            foreign_sections = ?mismatched,
            "hooks configured under runtime sections that do not match the active runtime; they will not fire",
        );
    }
}

/// Emit load-time warnings for hook definitions with unreachable / ambiguous flags.
/// `section_label` identifies where the hook lives (e.g., `cli.task_complete`).
pub fn validate_hook_def(section_label: &str, name: &str, hook: &HookDef, is_task_select: bool) {
    if matches!(hook.when, HookWhen::Pre)
        && matches!(hook.mode, HookMode::Async)
        && matches!(hook.on_failure, OnFailure::Abort)
    {
        tracing::warn!(
            section = section_label,
            hook = name,
            "pre+async hooks cannot abort; on_failure=abort is effectively warn"
        );
    }
    if hook.on_result.is_some() && hook.on_result != Some(OnResult::Any) && !is_task_select {
        tracing::warn!(
            section = section_label,
            hook = name,
            "on_result is only meaningful for task_select hooks; ignored"
        );
    }
}

/// Walk the entire config and run `validate_hook_def` on every hook definition.
/// Callers (bootstrap) invoke this once at startup.
pub fn validate_config_hooks(config: &Config) {
    fn walk(label_prefix: &str, action: &TaskActionHooks) {
        for (action_key, hooks) in [
            ("task_add", &action.task_add),
            ("task_publish", &action.task_publish),
            ("task_start", &action.task_start),
            ("task_complete", &action.task_complete),
            ("task_cancel", &action.task_cancel),
            ("task_select", &action.task_select),
        ] {
            let is_select = action_key == "task_select";
            for (name, def) in &hooks.hooks {
                validate_hook_def(
                    &format!("{label_prefix}.{action_key}"),
                    name,
                    def,
                    is_select,
                );
            }
        }
    }
    fn walk_contract(label_prefix: &str, action: &ContractActionHooks) {
        for (action_key, hooks) in [
            ("contract_add", &action.contract_add),
            ("contract_edit", &action.contract_edit),
            ("contract_delete", &action.contract_delete),
            ("contract_dod_check", &action.contract_dod_check),
            ("contract_dod_uncheck", &action.contract_dod_uncheck),
            ("contract_note_add", &action.contract_note_add),
        ] {
            for (name, def) in &hooks.hooks {
                validate_hook_def(&format!("{label_prefix}.{action_key}"), name, def, false);
            }
        }
    }
    walk("cli", &config.cli.hooks);
    walk("server.relay", &config.server.relay.hooks);
    walk("server.remote", &config.server.remote.hooks);
    walk_contract("cli", &config.cli.contract_hooks);
    walk_contract("server.relay", &config.server.relay.contract_hooks);
    walk_contract("server.remote", &config.server.remote.contract_hooks);
    for (stage_name, stage) in &config.workflow.stages {
        for (hook_name, def) in &stage.hooks {
            validate_hook_def(&format!("workflow.{stage_name}"), hook_name, def, false);
        }
    }
}

/// Return the commands configured for the given CLI task-action event in the
/// active runtime. Used by `senko hooks test`. Returns `None` if the action key
/// is not a valid task action. Empty Vec means the action is valid but has no
/// hooks configured.
pub fn get_commands_for_event(config: &Config, event_name: &str) -> Option<Vec<String>> {
    let action = config.cli.hooks.action_config(event_name)?;
    let mut commands = Vec::new();
    for (name, def) in &action.hooks {
        match resolve_env_vars(def) {
            Ok(_) => commands.push(def.command.clone()),
            Err(missing) => {
                eprintln!(
                    "hook skipped ({}): {} — missing required env: {}",
                    event_name, name, missing
                );
            }
        }
    }
    Some(commands)
}

/// Execute a hook command synchronously, inheriting stdout/stderr.
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

/// Helper accessor so callers can inspect the current ActionConfig for diagnostic
/// display (e.g., `senko doctor`). Returns `None` for unknown action names.
pub fn action_hooks<'a>(
    runtime_hooks: &'a TaskActionHooks,
    action: &str,
) -> Option<&'a ActionConfig> {
    runtime_hooks.action_config(action)
}

/// Compute newly unblocked tasks after a task completion.
pub async fn compute_unblocked(
    backend: &dyn HookDataSource,
    project_id: i64,
    prev_ready_ids: &std::collections::HashSet<crate::domain::task::TaskId>,
) -> Vec<UnblockedTask> {
    let curr_ready = backend
        .list_ready_tasks(project_id)
        .await
        .unwrap_or_default();
    task::compute_unblocked(&curr_ready, prev_ready_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::hook_trigger::SelectResult;
    use crate::infra::config::{EnvVarSpec, HookDef, HookMode, HookWhen, OnFailure, OnResult};
    use std::sync::Mutex;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn truncate_output_within_limit() {
        let data = b"hello world";
        assert_eq!(truncate_output(data), "hello world");
    }

    #[test]
    fn truncate_output_at_limit() {
        let data = vec![b'a'; MAX_OUTPUT_BYTES];
        assert_eq!(truncate_output(&data).len(), MAX_OUTPUT_BYTES);
    }

    #[test]
    fn truncate_output_over_limit_keeps_tail() {
        let mut data = vec![b'x'; MAX_OUTPUT_BYTES];
        data.extend_from_slice(b"end");
        let out = truncate_output(&data);
        assert!(out.ends_with("end"));
        assert_eq!(out.len(), MAX_OUTPUT_BYTES);
    }

    #[test]
    fn log_file_path_uses_xdg_state_home() {
        let tmp = tempfile::tempdir().unwrap();
        let xdg = XdgDirs {
            state_home: Some(tmp.path().to_path_buf()),
            ..Default::default()
        };
        let p = log_file_path(&xdg).unwrap();
        assert_eq!(p, tmp.path().join("senko").join("hooks.log"));
    }

    #[test]
    fn log_file_path_with_custom_dir() {
        let xdg = XdgDirs::default();
        let p = log_file_path_with_dir(Some("/var/logs"), &xdg).unwrap();
        assert_eq!(p, PathBuf::from("/var/logs/hooks.log"));
    }

    #[test]
    fn log_file_path_falls_back_none_when_state_home_absent() {
        let xdg = XdgDirs {
            state_home: None,
            ..Default::default()
        };
        assert!(log_file_path(&xdg).is_none());
    }

    #[test]
    fn log_to_file_creates_and_appends() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sub").join("hooks.log");
        let entry = HookLogEntry::new("INFO", "test")
            .with_event_id("id1")
            .with_event("task_add");
        log_to_file(&path, &entry);
        let entry2 = HookLogEntry::new("INFO", "test").with_event_id("id2");
        log_to_file(&path, &entry2);
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.lines().count(), 2);
        assert!(content.contains("id1"));
        assert!(content.contains("id2"));
    }

    #[test]
    fn backend_info_serialization_variants() {
        let s = serde_json::to_string(&BackendInfo::Sqlite {
            db_file_path: "/tmp/a.db".into(),
        })
        .unwrap();
        assert!(s.contains("\"sqlite\""));
        let s = serde_json::to_string(&BackendInfo::Postgresql).unwrap();
        assert!(s.contains("\"postgresql\""));
        let s = serde_json::to_string(&BackendInfo::Http {
            api_url: "http://x".into(),
        })
        .unwrap();
        assert!(s.contains("\"http\""));
    }

    #[test]
    fn hook_event_serializes_is_ready_field() {
        use crate::domain::task::{Priority, Task, TaskId, TaskStatus};
        let task = Task::new(
            TaskId(1),
            1,
            "t".into(),
            None,
            None,
            None,
            Priority::P2,
            TaskStatus::Todo,
            None,
            None,
            "2026-01-01T00:00:00Z".into(),
            "2026-01-01T00:00:00Z".into(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
        );
        let event = HookEvent {
            event_id: "eid".into(),
            event: "task_add".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            task,
            stats: HashMap::new(),
            ready_count: 0,
            is_ready: true,
            from_status: None,
            unblocked_tasks: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(
            json.contains("\"is_ready\":true"),
            "missing is_ready in {json}"
        );
    }

    #[test]
    fn hook_applies_filters_enabled_when_on_result() {
        let def = HookDef {
            command: "true".into(),
            when: HookWhen::Post,
            mode: HookMode::Async,
            on_failure: OnFailure::Abort,
            enabled: false,
            env_vars: vec![],
            on_result: None,
            prompt: None,
        };
        assert!(!hook_applies(&def, HookWhen::Post, None));

        let mut def = def;
        def.enabled = true;
        assert!(hook_applies(&def, HookWhen::Post, None));
        assert!(!hook_applies(&def, HookWhen::Pre, None));

        def.on_result = Some(OnResult::Selected);
        assert!(hook_applies(
            &def,
            HookWhen::Post,
            Some(SelectResult::Selected)
        ));
        assert!(!hook_applies(
            &def,
            HookWhen::Post,
            Some(SelectResult::None)
        ));
        // non-TaskSelect trigger: other triggers are treated as "no result info".
        assert!(!hook_applies(&def, HookWhen::Post, None));

        def.on_result = Some(OnResult::Any);
        assert!(hook_applies(&def, HookWhen::Post, None));
    }

    #[test]
    fn resolve_env_vars_required_missing_returns_err() {
        let def = HookDef {
            command: "true".into(),
            when: HookWhen::Post,
            mode: HookMode::Async,
            on_failure: OnFailure::Abort,
            enabled: true,
            env_vars: vec![EnvVarSpec {
                name: "DEFINITELY_NOT_SET_XYZ_123".into(),
                required: true,
                default: None,
                description: None,
            }],
            on_result: None,
            prompt: None,
        };
        let res = resolve_env_vars(&def);
        assert!(matches!(res, Err(ref s) if s == "DEFINITELY_NOT_SET_XYZ_123"));
    }

    #[test]
    fn resolve_env_vars_default_applied_when_unset() {
        let def = HookDef {
            command: "true".into(),
            when: HookWhen::Post,
            mode: HookMode::Async,
            on_failure: OnFailure::Abort,
            enabled: true,
            env_vars: vec![EnvVarSpec {
                name: "SENKO_TEST_ENV_DEFAULT_XYZ".into(),
                required: true,
                default: Some("fallback".into()),
                description: None,
            }],
            on_result: None,
            prompt: None,
        };
        // SAFETY: serialized via ENV_MUTEX with other env-touching tests.
        let _guard = ENV_MUTEX.lock().unwrap();
        unsafe {
            std::env::remove_var("SENKO_TEST_ENV_DEFAULT_XYZ");
        }
        let map = resolve_env_vars(&def).unwrap();
        assert_eq!(
            map.get("SENKO_TEST_ENV_DEFAULT_XYZ"),
            Some(&"fallback".to_string())
        );
    }

    #[test]
    fn resolve_env_vars_optional_missing_no_error() {
        let def = HookDef {
            command: "true".into(),
            when: HookWhen::Post,
            mode: HookMode::Async,
            on_failure: OnFailure::Abort,
            enabled: true,
            env_vars: vec![EnvVarSpec {
                name: "SENKO_TEST_OPTIONAL_VAR".into(),
                required: false,
                default: None,
                description: None,
            }],
            on_result: None,
            prompt: None,
        };
        let _guard = ENV_MUTEX.lock().unwrap();
        unsafe {
            std::env::remove_var("SENKO_TEST_OPTIONAL_VAR");
        }
        let map = resolve_env_vars(&def).unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn warn_about_mismatched_runtime_sections_does_not_panic() {
        let mut config = Config::default();
        config.server.relay.hooks.task_complete.hooks.insert(
            "foreign".into(),
            HookDef {
                command: "true".into(),
                when: HookWhen::Post,
                mode: HookMode::Async,
                on_failure: OnFailure::Abort,
                enabled: true,
                env_vars: vec![],
                on_result: None,
                prompt: None,
            },
        );
        // Running as CLI — the relay section is mismatched. Warning is emitted
        // via tracing; just verify the function completes.
        warn_about_mismatched_runtime_sections(&config, &RuntimeMode::Cli);
    }

    #[test]
    fn validate_config_hooks_accepts_valid_definitions() {
        let mut config = Config::default();
        config.cli.hooks.task_complete.hooks.insert(
            "ok_hook".into(),
            HookDef {
                command: "true".into(),
                when: HookWhen::Pre,
                mode: HookMode::Sync,
                on_failure: OnFailure::Abort,
                enabled: true,
                env_vars: vec![],
                on_result: None,
                prompt: None,
            },
        );
        // Should not panic even when hook config has warnings / is fine.
        validate_config_hooks(&config);
    }

    #[test]
    fn hooks_for_runtime_returns_correct_section() {
        let config = Config::default();
        // All empty by default.
        assert!(hooks_for_runtime(&config, &RuntimeMode::Cli).is_empty());
        assert!(hooks_for_runtime(&config, &RuntimeMode::ServerRelay).is_empty());
        assert!(hooks_for_runtime(&config, &RuntimeMode::ServerRemote).is_empty());
    }
}
