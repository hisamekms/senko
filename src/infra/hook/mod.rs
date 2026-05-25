pub mod executor;
pub mod test_executor;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;

use crate::application::HookTrigger;
use crate::application::hook_trigger::SelectResult;
use crate::application::port::HookDataSource;
use crate::application::telemetry::{ForwardedScope, RESOLVED_USER};
use crate::domain::contract::Contract;
use crate::domain::project::ProjectId;
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
    pub id: ProjectId,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnvelopeUserInfo {
    pub id: crate::domain::user::UserId,
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

    fn with_optional_stdout(mut self, bytes: &[u8]) -> Self {
        if !bytes.is_empty() {
            self.stdout = Some(truncate_output(bytes));
        }
        self
    }

    fn with_optional_stderr(mut self, bytes: &[u8]) -> Self {
        if !bytes.is_empty() {
            self.stderr = Some(truncate_output(bytes));
        }
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
    project_id: ProjectId,
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

/// Maximum bytes of stderr to attach to `senko.hook.failed` events.
/// Contract #8 D1 spec.
const STDERR_EXCERPT_BYTES: usize = 1024;

/// Outcome of a single hook command execution. Used to drive both the
/// `senko.hook.fired` / `senko.hook.failed` business event and the
/// per-line JSONL `HookLogEntry` in one place.
enum WaitOutcome {
    Exited {
        status: std::process::ExitStatus,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    /// `child.try_wait()` returned `Ok(None)` past the timeout window;
    /// the child was sent `Child::kill()`. `stdout` / `stderr` are whatever
    /// the drain threads collected before the kill landed.
    Timeout { stdout: Vec<u8>, stderr: Vec<u8> },
    /// `child.try_wait()` itself returned `Err`. Defensive — in practice
    /// only reachable via OS-level malfunctions (e.g., reaped externally).
    WaitError(String),
}

/// Drive a child process to completion or timeout. Drains stdout/stderr on
/// dedicated threads so a long-running command cannot block on a full pipe
/// buffer. Polls `try_wait()` every 50ms; on timeout sends `kill()` and
/// reaps via `wait()`.
fn wait_with_timeout(mut child: std::process::Child, timeout: Duration) -> WaitOutcome {
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let stdout_h = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::BufReader::new(stdout).read_to_end(&mut buf);
        buf
    });
    let stderr_h = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::BufReader::new(stderr).read_to_end(&mut buf);
        buf
    });

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = stdout_h.join().unwrap_or_default();
                let stderr = stderr_h.join().unwrap_or_default();
                return WaitOutcome::Exited {
                    status,
                    stdout,
                    stderr,
                };
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let stdout = stdout_h.join().unwrap_or_default();
                    let stderr = stderr_h.join().unwrap_or_default();
                    return WaitOutcome::Timeout { stdout, stderr };
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_h.join();
                let _ = stderr_h.join();
                return WaitOutcome::WaitError(format!("{e:#}"));
            }
        }
    }
}

/// Truncate stderr bytes to at most `STDERR_EXCERPT_BYTES` for the
/// `stderr_excerpt` attribute on `senko.hook.failed`.
fn stderr_excerpt(stderr: &[u8]) -> String {
    let end = stderr.len().min(STDERR_EXCERPT_BYTES);
    String::from_utf8_lossy(&stderr[..end]).into_owned()
}

/// Emit `senko.hook.fired` / `senko.hook.failed` for one completed hook
/// invocation. Common attributes (`enduser.id`, `senko.operation.id`, ...)
/// are auto-attached by [`crate::application::telemetry::BusinessAttributesProcessor`]
/// when the macro fires under populated task-locals.
fn emit_hook_outcome_event(
    hook_name: &str,
    trigger: &str,
    outcome: &WaitOutcome,
    elapsed: Duration,
) {
    let elapsed_ms = elapsed.as_millis() as i64;
    match outcome {
        WaitOutcome::Exited { status, .. } if status.success() => {
            crate::emit_business_event!(
                "senko.hook.fired",
                hook.name = hook_name,
                hook.trigger = trigger,
                exit_status = 0_i64,
                duration_ms = elapsed_ms,
            );
        }
        WaitOutcome::Exited { status, stderr, .. } => {
            let exit_code = status.code().map(i64::from).unwrap_or(-1);
            let excerpt = stderr_excerpt(stderr);
            crate::emit_business_event!(
                "senko.hook.failed",
                level: WARN,
                hook.name = hook_name,
                hook.trigger = trigger,
                failure.reason = "non_zero_exit",
                exit_status = exit_code,
                duration_ms = elapsed_ms,
                stderr_excerpt = excerpt,
            );
        }
        WaitOutcome::Timeout { stderr, .. } => {
            let excerpt = stderr_excerpt(stderr);
            crate::emit_business_event!(
                "senko.hook.failed",
                level: WARN,
                hook.name = hook_name,
                hook.trigger = trigger,
                failure.reason = "timeout",
                duration_ms = elapsed_ms,
                stderr_excerpt = excerpt,
            );
        }
        WaitOutcome::WaitError(msg) => {
            crate::emit_business_event!(
                "senko.hook.failed",
                level: WARN,
                hook.name = hook_name,
                hook.trigger = trigger,
                failure.reason = "wait_error",
                duration_ms = elapsed_ms,
                error.message = msg.as_str(),
            );
        }
    }
}

/// Emit a pre-spawn / pre-wait failure (`spawn_error`, `stdin_error`).
/// Stays separate from `emit_hook_outcome_event` because no `WaitOutcome`
/// exists yet — the child either failed to launch or never received stdin.
fn emit_hook_pre_wait_failure(
    hook_name: &str,
    trigger: &str,
    reason: &str,
    elapsed: Duration,
    error_message: &str,
) {
    let elapsed_ms = elapsed.as_millis() as i64;
    crate::emit_business_event!(
        "senko.hook.failed",
        level: WARN,
        hook.name = hook_name,
        hook.trigger = trigger,
        failure.reason = reason,
        duration_ms = elapsed_ms,
        error.message = error_message,
    );
}

/// Write the per-line `HookLogEntry` (file/stdout JSONL) for a completed hook.
/// Replaces the old `log_hook_outcome` and consolidates the failure paths
/// (`Timeout` / `WaitError`) into the same JSONL stream.
fn log_hook_outcome_entry(
    log_target: Option<&HookLogTarget>,
    event_name: &str,
    event_id: &str,
    hook_name: &str,
    command: &str,
    task_id: Option<i64>,
    outcome: &WaitOutcome,
) {
    let Some(t) = log_target else {
        return;
    };
    let entry = match outcome {
        WaitOutcome::Exited {
            status,
            stdout,
            stderr,
        } if status.success() => HookLogEntry::new("INFO", "hook_ok")
            .with_event_id(event_id)
            .with_event(event_name)
            .with_hook(hook_name)
            .with_command(command)
            .with_task_id(task_id)
            .with_exit_code(status.code())
            .with_optional_stdout(stdout)
            .with_optional_stderr(stderr),
        WaitOutcome::Exited {
            status,
            stdout,
            stderr,
        } => HookLogEntry::new("WARN", "hook_failed")
            .with_event_id(event_id)
            .with_event(event_name)
            .with_hook(hook_name)
            .with_command(command)
            .with_task_id(task_id)
            .with_exit_code(status.code())
            .with_optional_stdout(stdout)
            .with_optional_stderr(stderr),
        WaitOutcome::Timeout { stdout, stderr } => HookLogEntry::new("WARN", "hook_failed")
            .with_event_id(event_id)
            .with_event(event_name)
            .with_hook(hook_name)
            .with_command(command)
            .with_task_id(task_id)
            .with_message("hook killed: timeout")
            .with_optional_stdout(stdout)
            .with_optional_stderr(stderr),
        WaitOutcome::WaitError(msg) => HookLogEntry::new("ERROR", "hook_error")
            .with_event_id(event_id)
            .with_event(event_name)
            .with_hook(hook_name)
            .with_command(command)
            .with_task_id(task_id)
            .with_message(&format!("hook wait error: {msg}")),
    };
    write_hook_log(t, &entry);
}

/// Run a hook command with the given env map and JSON stdin.
///
/// On every invocation emits exactly one Contract #8 business event:
/// `senko.hook.fired` (success) or `senko.hook.failed` (any of
/// `failure.reason ∈ { spawn_error, stdin_error, non_zero_exit, timeout, wait_error }`).
///
/// Returns `(exit_status, join_handle)` where:
/// - `exit_status` is `Some(_)` only when `sync=true` and the child reached
///   an `Exited` outcome; `None` otherwise (including all failure paths and
///   every `sync=false` call).
/// - `join_handle` is `Some(_)` only when `sync=false` and the worker thread
///   was spawned; production callers drop it (fire-and-forget) while tests
///   join it to wait for the worker's emit to land.
///
/// `std::thread::spawn` does not propagate tokio task-locals
/// (`RESOLVED_USER`, `INBOUND_BAGGAGE`); the async branch captures them
/// before spawning and re-establishes them on the worker via
/// [`ForwardedScope`] so [`crate::application::telemetry::BusinessAttributesProcessor`]
/// can still attach `enduser.*` / `senko.operation.id`. The current
/// `tracing::Dispatch` is captured the same way so emits from the worker
/// reach the configured subscriber (matters for tests using thread-local
/// `with_default`; production sets a global default).
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
    timeout: Duration,
) -> (
    Option<std::process::ExitStatus>,
    Option<std::thread::JoinHandle<()>>,
) {
    let start = Instant::now();
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
            let err = format!("{e:#}");
            emit_hook_pre_wait_failure(hook_name, event_name, "spawn_error", start.elapsed(), &err);
            if let Some(t) = log_target {
                let entry = HookLogEntry::new("ERROR", "hook_error")
                    .with_event_id(event_id)
                    .with_event(event_name)
                    .with_hook(hook_name)
                    .with_command(command)
                    .with_task_id(task_id)
                    .with_message(&format!("hook spawn error: {err}"));
                write_hook_log(t, &entry);
            }
            return (None, None);
        }
    };

    if let Some(mut stdin) = child.stdin.take()
        && let Err(e) = stdin.write_all(json.as_bytes())
    {
        let err = format!("{e:#}");
        // Drop the child cleanly so the kernel reaps it.
        let _ = child.kill();
        let _ = child.wait();
        emit_hook_pre_wait_failure(hook_name, event_name, "stdin_error", start.elapsed(), &err);
        if let Some(t) = log_target {
            let entry = HookLogEntry::new("ERROR", "hook_error")
                .with_event_id(event_id)
                .with_event(event_name)
                .with_hook(hook_name)
                .with_command(command)
                .with_task_id(task_id)
                .with_message(&format!("hook stdin error: {err}"));
            write_hook_log(t, &entry);
        }
        return (None, None);
    }

    if sync {
        let outcome = wait_with_timeout(child, timeout);
        let returned = match &outcome {
            WaitOutcome::Exited { status, .. } => Some(*status),
            _ => None,
        };
        emit_hook_outcome_event(hook_name, event_name, &outcome, start.elapsed());
        log_hook_outcome_entry(
            log_target, event_name, event_id, hook_name, command, task_id, &outcome,
        );
        (returned, None)
    } else {
        // Capture tokio task-locals + tracing dispatch BEFORE std::thread::spawn —
        // they don't cross std::thread boundaries (only tokio task boundaries
        // for task-locals, only the global default for tracing).
        let forwarded_user = RESOLVED_USER.try_with(|u| u.clone()).ok();
        let forwarded_op = crate::infra::http::INBOUND_BAGGAGE
            .try_with(|m| m.get("senko.operation.id").cloned())
            .ok()
            .flatten();
        let dispatch = tracing::dispatcher::get_default(|d| d.clone());

        let cmd_s = command.to_owned();
        let evt = event_name.to_owned();
        let eid = event_id.to_owned();
        let hname = hook_name.to_owned();
        let tid = task_id;
        let log = log_target.cloned();
        let handle = std::thread::spawn(move || {
            let _scope = ForwardedScope::enter(forwarded_user, forwarded_op);
            tracing::dispatcher::with_default(&dispatch, || {
                let outcome = wait_with_timeout(child, timeout);
                emit_hook_outcome_event(&hname, &evt, &outcome, start.elapsed());
                log_hook_outcome_entry(log.as_ref(), &evt, &eid, &hname, &cmd_s, tid, &outcome);
            });
        });
        (None, Some(handle))
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
        Some(name) => match crate::domain::user::Username::try_from(name.to_string()) {
            Ok(username) => backend
                .get_user_by_username(&username)
                .await
                .map(|u| EnvelopeUserInfo {
                    id: u.id(),
                    name: u.username().as_ref().to_owned(),
                })
                .unwrap_or_else(|_| EnvelopeUserInfo {
                    id: DEFAULT_USER_ID,
                    name: "default".into(),
                }),
            Err(_) => EnvelopeUserInfo {
                id: DEFAULT_USER_ID,
                name: "default".into(),
            },
        },
        None => backend
            .get_user(DEFAULT_USER_ID)
            .await
            .map(|u| EnvelopeUserInfo {
                id: u.id(),
                name: u.username().as_ref().to_owned(),
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
        let timeout = Duration::from_secs(hook.timeout_secs);
        let (status, _join) = run_hook_command(
            &hook.command,
            event_name,
            envelope_event_id,
            &name,
            task_id_for_log,
            envelope_json,
            &env_map,
            sync,
            Some(log_target),
            timeout,
        );

        if sync {
            let failed = status.map(|s| !s.success()).unwrap_or(true);
            if failed && hook.on_failure == OnFailure::Abort && when == HookWhen::Pre {
                // `senko.hook.failed` is already emitted from `run_hook_command`;
                // here we only honor the on_failure=abort + sync+pre semantics
                // by signaling FireOutcome::Abort to the caller. Other branches
                // (post / on_failure=warn / ignore) used to emit duplicate
                // `tracing::warn!` lines that were superseded by the new
                // business event in Contract #8 D1.
                outcome = FireOutcome::Abort;
                return outcome;
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
        HookTrigger::Project(_) | HookTrigger::User(_) | HookTrigger::MetadataField(_) => {
            // `fire()` is task-scoped. Project / User / MetadataField triggers
            // are wired in Phase B3 via their own dispatchers; here we no-op so
            // accidental task-path calls do not raise an error.
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
            ("task_resume", &action.task_resume),
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
    project_id: ProjectId,
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
            ProjectId(1),
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
            timeout_secs: 30,
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
            timeout_secs: 30,
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
            timeout_secs: 30,
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
            timeout_secs: 30,
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
                timeout_secs: 30,
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
                timeout_secs: 30,
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

    // --- Contract #8 D1: senko.hook.fired / senko.hook.failed -----------

    use crate::application::telemetry::test_support::{
        build_capture_provider, capture_layer, lookup_attr,
    };
    use opentelemetry::logs::{AnyValue, Severity};
    use tracing_subscriber::layer::SubscriberExt;

    /// Helper: run `run_hook_command` under a capture-only subscriber and
    /// return the emitted business-event LogRecords.
    fn run_and_capture(
        command: &str,
        timeout: Duration,
    ) -> Vec<opentelemetry_sdk::logs::SdkLogRecord> {
        let (exporter, provider) = build_capture_provider();
        let subscriber = tracing_subscriber::registry().with(capture_layer(&provider));
        tracing::subscriber::with_default(subscriber, || {
            run_hook_command(
                command,
                "task_complete",
                "evt-1",
                "h1",
                None,
                "{}",
                &HashMap::new(),
                true,
                None,
                timeout,
            );
        });
        provider.force_flush().expect("flush ok");
        exporter
            .get_emitted_logs()
            .expect("logs exported")
            .into_iter()
            .map(|l| l.record)
            .collect()
    }

    #[test]
    fn run_hook_command_emits_senko_hook_fired_on_success() {
        // `cat >/dev/null` consumes stdin so the parent's `write_all("{}")`
        // completes before the child exits, avoiding an EPIPE race that would
        // make `run_hook_command` early-return `(None, None)`.
        let records = run_and_capture("cat >/dev/null", Duration::from_secs(5));
        let fired = records
            .iter()
            .find(|r| r.event_name() == Some("senko.hook.fired"))
            .expect("expected senko.hook.fired");

        assert_eq!(
            lookup_attr(fired, "hook.name"),
            Some(AnyValue::String("h1".into()))
        );
        assert_eq!(
            lookup_attr(fired, "hook.trigger"),
            Some(AnyValue::String("task_complete".into()))
        );
        assert_eq!(lookup_attr(fired, "exit_status"), Some(AnyValue::Int(0)));
        assert!(matches!(
            lookup_attr(fired, "duration_ms"),
            Some(AnyValue::Int(_))
        ));
        assert_eq!(fired.severity_number(), Some(Severity::Info));
        assert_eq!(fired.target().map(|c| c.as_ref()), Some("senko_business"));

        // No senko.hook.failed alongside senko.hook.fired
        assert!(
            records
                .iter()
                .all(|r| r.event_name() != Some("senko.hook.failed")),
            "did not expect senko.hook.failed on success path",
        );
    }

    #[test]
    fn run_hook_command_emits_senko_hook_failed_on_non_zero_exit() {
        let records = run_and_capture("cat >/dev/null; exit 1", Duration::from_secs(5));
        let failed = records
            .iter()
            .find(|r| r.event_name() == Some("senko.hook.failed"))
            .expect("expected senko.hook.failed");

        assert_eq!(
            lookup_attr(failed, "failure.reason"),
            Some(AnyValue::String("non_zero_exit".into()))
        );
        assert_eq!(lookup_attr(failed, "exit_status"), Some(AnyValue::Int(1)));
        assert_eq!(
            lookup_attr(failed, "hook.trigger"),
            Some(AnyValue::String("task_complete".into()))
        );
        assert_eq!(failed.severity_number(), Some(Severity::Warn));

        // No senko.hook.fired on a failed run
        assert!(
            records
                .iter()
                .all(|r| r.event_name() != Some("senko.hook.fired")),
            "did not expect senko.hook.fired on failure path",
        );
    }

    #[test]
    fn run_hook_command_emits_senko_hook_failed_on_timeout() {
        // 150ms timeout against a `sleep 5` command; child gets killed and
        // the outcome is surfaced as failure.reason=timeout.
        let records = run_and_capture("sleep 5", Duration::from_millis(150));
        let failed = records
            .iter()
            .find(|r| r.event_name() == Some("senko.hook.failed"))
            .expect("expected senko.hook.failed");

        assert_eq!(
            lookup_attr(failed, "failure.reason"),
            Some(AnyValue::String("timeout".into()))
        );
        // Timeout path does not carry exit_status (no exit happened)
        assert!(lookup_attr(failed, "exit_status").is_none());

        // duration_ms must be ≥ the configured timeout
        let dur_ms = match lookup_attr(failed, "duration_ms") {
            Some(AnyValue::Int(v)) => v,
            other => panic!("expected duration_ms Int, got {other:?}"),
        };
        assert!(dur_ms >= 150, "expected duration_ms ≥ 150, got {dur_ms}");
    }

    // --- Contract #8 V1 follow-up #365: async-mode hook task-local forwarding ---

    use crate::application::telemetry::ResolvedUser;
    use crate::infra::http::INBOUND_BAGGAGE;
    use std::collections::BTreeMap;

    /// Run `run_hook_command` in async mode (`sync=false`) under optional
    /// `RESOLVED_USER` / `senko.operation.id` scopes. Joins the worker thread
    /// so the LogRecord is observable when the helper returns.
    ///
    /// The dispatcher is set thread-locally on the calling thread; the
    /// production async branch captures it via `tracing::dispatcher::get_default`
    /// and re-establishes it on the worker via `with_default`. Without that
    /// capture the worker's emit would land on the no-op global dispatcher.
    fn run_async_and_capture_with_principal(
        command: &str,
        timeout: Duration,
        user: Option<ResolvedUser>,
        operation_id: Option<String>,
    ) -> Vec<opentelemetry_sdk::logs::SdkLogRecord> {
        let (exporter, provider) = build_capture_provider();
        let subscriber = tracing_subscriber::registry().with(capture_layer(&provider));
        let dispatch = tracing::Dispatch::new(subscriber);

        let join_handle = {
            let _g = tracing::dispatcher::set_default(&dispatch);

            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let cmd = command.to_string();
            rt.block_on(async move {
                let mut bag = BTreeMap::new();
                if let Some(op) = operation_id {
                    bag.insert("senko.operation.id".to_string(), op);
                }
                let fut = INBOUND_BAGGAGE.scope(bag, async move {
                    let (_status, handle) = run_hook_command(
                        &cmd,
                        "task_complete",
                        "evt-1",
                        "h1",
                        None,
                        "{}",
                        &HashMap::new(),
                        false,
                        None,
                        timeout,
                    );
                    handle
                });
                match user {
                    Some(u) => RESOLVED_USER.scope(u, fut).await,
                    None => fut.await,
                }
            })
        };

        join_handle
            .expect("async mode returns a JoinHandle")
            .join()
            .expect("hook worker thread joined cleanly");

        provider.force_flush().expect("flush ok");
        exporter
            .get_emitted_logs()
            .expect("logs exported")
            .into_iter()
            .map(|l| l.record)
            .collect()
    }

    fn alice() -> ResolvedUser {
        ResolvedUser {
            id: 7,
            username: "alice".into(),
        }
    }

    #[test]
    fn async_hook_attaches_enduser_and_op_id_on_success() {
        let records = run_async_and_capture_with_principal(
            "cat >/dev/null",
            Duration::from_secs(5),
            Some(alice()),
            Some("op-async-ok".into()),
        );
        let fired = records
            .iter()
            .find(|r| r.event_name() == Some("senko.hook.fired"))
            .expect("expected senko.hook.fired");

        assert_eq!(lookup_attr(fired, "enduser.id"), Some(AnyValue::Int(7)));
        assert_eq!(
            lookup_attr(fired, "enduser.name"),
            Some(AnyValue::String("alice".into()))
        );
        assert_eq!(
            lookup_attr(fired, "senko.operation.id"),
            Some(AnyValue::String("op-async-ok".into()))
        );
        // Existing event-specific attrs preserved.
        assert_eq!(lookup_attr(fired, "exit_status"), Some(AnyValue::Int(0)));
        assert_eq!(
            lookup_attr(fired, "hook.trigger"),
            Some(AnyValue::String("task_complete".into()))
        );
    }

    #[test]
    fn async_hook_attaches_enduser_and_op_id_on_non_zero_exit() {
        let records = run_async_and_capture_with_principal(
            "cat >/dev/null; exit 1",
            Duration::from_secs(5),
            Some(alice()),
            Some("op-async-fail".into()),
        );
        let failed = records
            .iter()
            .find(|r| r.event_name() == Some("senko.hook.failed"))
            .expect("expected senko.hook.failed");

        assert_eq!(
            lookup_attr(failed, "failure.reason"),
            Some(AnyValue::String("non_zero_exit".into()))
        );
        assert_eq!(lookup_attr(failed, "enduser.id"), Some(AnyValue::Int(7)));
        assert_eq!(
            lookup_attr(failed, "enduser.name"),
            Some(AnyValue::String("alice".into()))
        );
        assert_eq!(
            lookup_attr(failed, "senko.operation.id"),
            Some(AnyValue::String("op-async-fail".into()))
        );
    }

    #[test]
    fn async_hook_attaches_enduser_and_op_id_on_timeout() {
        let records = run_async_and_capture_with_principal(
            "sleep 5",
            Duration::from_millis(150),
            Some(alice()),
            Some("op-async-timeout".into()),
        );
        let failed = records
            .iter()
            .find(|r| r.event_name() == Some("senko.hook.failed"))
            .expect("expected senko.hook.failed");

        assert_eq!(
            lookup_attr(failed, "failure.reason"),
            Some(AnyValue::String("timeout".into()))
        );
        assert_eq!(lookup_attr(failed, "enduser.id"), Some(AnyValue::Int(7)));
        assert_eq!(
            lookup_attr(failed, "enduser.name"),
            Some(AnyValue::String("alice".into()))
        );
        assert_eq!(
            lookup_attr(failed, "senko.operation.id"),
            Some(AnyValue::String("op-async-timeout".into()))
        );
    }

    #[test]
    fn async_hook_attaches_no_enduser_when_principal_unset() {
        // Defensive: when neither RESOLVED_USER nor senko.operation.id is in
        // scope (e.g., CLI mode), the worker's emit must not synthesise either
        // attribute. The forwarded thread-locals stay None.
        let records = run_async_and_capture_with_principal(
            "cat >/dev/null",
            Duration::from_secs(5),
            None,
            None,
        );
        let fired = records
            .iter()
            .find(|r| r.event_name() == Some("senko.hook.fired"))
            .expect("expected senko.hook.fired");

        assert!(lookup_attr(fired, "enduser.id").is_none());
        assert!(lookup_attr(fired, "enduser.name").is_none());
        assert!(lookup_attr(fired, "senko.operation.id").is_none());
    }

    /// Re-confirm that the **sync** path (auto-attach via tokio task-local)
    /// keeps working post-fix. The thread-local fallback is only consulted
    /// when the tokio task-local is absent, so there must be no double-attach
    /// or regression here.
    #[test]
    fn sync_hook_still_attaches_enduser_and_op_id_via_task_local() {
        let (exporter, provider) = build_capture_provider();
        let subscriber = tracing_subscriber::registry().with(capture_layer(&provider));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let mut bag = BTreeMap::new();
            bag.insert("senko.operation.id".to_string(), "op-sync".to_string());
            RESOLVED_USER
                .scope(
                    alice(),
                    INBOUND_BAGGAGE.scope(bag, async {
                        let _g = tracing::subscriber::set_default(subscriber);
                        let (_status, _join) = run_hook_command(
                            "true",
                            "task_complete",
                            "evt-1",
                            "h1",
                            None,
                            "{}",
                            &HashMap::new(),
                            true,
                            None,
                            Duration::from_secs(5),
                        );
                    }),
                )
                .await;
        });

        provider.force_flush().expect("flush ok");
        let logs = exporter.get_emitted_logs().expect("logs exported");
        let fired_log = logs
            .iter()
            .find(|l| l.record.event_name() == Some("senko.hook.fired"))
            .expect("expected senko.hook.fired");
        let fired = &fired_log.record;

        assert_eq!(lookup_attr(fired, "enduser.id"), Some(AnyValue::Int(7)));
        assert_eq!(
            lookup_attr(fired, "enduser.name"),
            Some(AnyValue::String("alice".into()))
        );
        assert_eq!(
            lookup_attr(fired, "senko.operation.id"),
            Some(AnyValue::String("op-sync".into()))
        );
        // Exactly one `enduser.id` attribute — fallback must not double-attach.
        let dup_count = fired
            .attributes_iter()
            .filter(|(k, _)| k.as_str() == "enduser.id")
            .count();
        assert_eq!(
            dup_count, 1,
            "tokio task-local present → thread-local fallback must NOT also attach"
        );
    }
}
