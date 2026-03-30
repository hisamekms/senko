use std::fs;
use std::io::Read;
use std::path::PathBuf;
use anyhow::{bail, Context, Result};

use super::{
    Cli, DepsCommand, DodCommand, DryRunOperation, HooksCommand, MemberAction,
    OutputFormat, ProjectAction, UserAction, CONFIG_TEMPLATE, print_dry_run,
};
use crate::bootstrap::{
    create_backend, create_hook_executor, create_project_service, create_task_service,
    create_user_service, resolve_project_id, resolve_user_id,
    DEFAULT_PROJECT_ID,
};
use crate::domain::config::{CliOverrides, Config};
use crate::bootstrap::resolve_backend_info;
use crate::infra::hook as hooks;
use crate::domain::project::CreateProjectParams;
use crate::domain::task::{
    CreateTaskParams, HookTrigger, ListTasksFilter, Priority, Task, TaskEvent, TaskStatus,
    UpdateTaskArrayParams, UpdateTaskParams,
};
use crate::domain::user::{AddProjectMemberParams, CreateUserParams};
use crate::infra::project_root::resolve_project_root;

fn build_cli_overrides(cli: &Cli) -> CliOverrides {
    CliOverrides {
        log_dir: cli.log_dir.as_ref().map(|p| p.to_string_lossy().into_owned()),
        db_path: cli.db_path.as_ref().map(|p| p.to_string_lossy().into_owned()),
        postgres_url: cli.postgres_url.clone(),
        project: cli.project.clone(),
        user: cli.user.clone(),
        ..Default::default()
    }
}

fn load_config(cli: &Cli, root: &std::path::Path) -> Result<Config> {
    let mut config = hooks::load_config(root, cli.config.as_deref())?;
    config.apply_cli(&build_cli_overrides(cli));
    Ok(config)
}

#[allow(clippy::too_many_arguments)]
pub async fn cmd_add(
    cli: &Cli,
    title: Option<String>,
    background: Option<String>,
    description: Option<String>,
    priority: Option<String>,
    definition_of_done: Vec<String>,
    in_scope: Vec<String>,
    out_of_scope: Vec<String>,
    tag: Vec<String>,
    depends_on: Vec<i64>,
    branch: Option<String>,
    metadata: Option<String>,
    from_json: bool,
    from_json_file: Option<PathBuf>,
) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (backend, using_http) = create_backend(&root, &config)?;
    let project_id = resolve_project_id(&*backend, &config).await?;

    let params = if from_json {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("failed to read from stdin")?;
        serde_json::from_str::<CreateTaskParams>(&buf).context("invalid JSON from stdin")?
    } else if let Some(path) = from_json_file {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read file: {}", path.display()))?;
        serde_json::from_str::<CreateTaskParams>(&content).context("invalid JSON in file")?
    } else {
        let Some(title) = title else {
            bail!("--title is required when not using --from-json or --from-json-file");
        };
        let priority = match priority {
            Some(s) => Some(s.parse::<Priority>()?),
            None => None,
        };
        let metadata_val = match metadata {
            Some(m) => {
                let val: serde_json::Value = serde_json::from_str(&m)
                    .context("invalid JSON for --metadata")?;
                Some(val)
            }
            None => None,
        };
        CreateTaskParams {
            title,
            background,
            description,
            priority,
            definition_of_done,
            in_scope,
            out_of_scope,
            branch,
            pr_url: None,
            metadata: metadata_val,
            tags: tag,
            dependencies: depends_on,
        }
    };

    if cli.dry_run {
        let mut operations = vec![format!("Create task with title \"{}\"", params.title)];
        if let Some(ref p) = params.priority {
            operations.push(format!("Set priority to {}", p));
        }
        if let Some(ref bg) = params.background {
            operations.push(format!("Set background to \"{}\"", bg));
        }
        if let Some(ref desc) = params.description {
            operations.push(format!("Set description to \"{}\"", desc));
        }
        if !params.tags.is_empty() {
            operations.push(format!("Set tags: {}", params.tags.join(", ")));
        }
        if !params.dependencies.is_empty() {
            let deps: Vec<String> = params.dependencies.iter().map(|d| format!("#{d}")).collect();
            operations.push(format!("Set dependencies: {}", deps.join(", ")));
        }
        if !params.definition_of_done.is_empty() {
            operations.push(format!("Set definition of done: {}", params.definition_of_done.join(", ")));
        }
        if !params.in_scope.is_empty() {
            operations.push(format!("Set in scope: {}", params.in_scope.join(", ")));
        }
        if !params.out_of_scope.is_empty() {
            operations.push(format!("Set out of scope: {}", params.out_of_scope.join(", ")));
        }
        if let Some(ref b) = params.branch {
            operations.push(format!("Set branch to \"{}\"", b));
        }
        if let Some(ref m) = params.metadata {
            operations.push(format!("Set metadata to {}", m));
        }
        return print_dry_run(&cli.output, &DryRunOperation { command: "add".into(), operations });
    }

    let task_service = create_task_service(backend, &config, using_http, &root);
    let task = task_service.create_task(project_id, &params).await?;

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&task)?);
        }
        OutputFormat::Text => {
            println!("Created task #{}: \"{}\"", task.id(), task.title());
        }
    }

    Ok(())
}

pub async fn cmd_list(
    cli: &Cli,
    status: Vec<String>,
    tag: Vec<String>,
    depends_on: Option<i64>,
    ready: bool,
) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (backend, _) = create_backend(&root, &config)?;
    let project_id = resolve_project_id(&*backend, &config).await?;

    let statuses = status
        .into_iter()
        .map(|s| s.parse::<TaskStatus>())
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("invalid status value")?;

    let filter = ListTasksFilter {
        statuses,
        tags: tag,
        depends_on,
        ready,
    };

    let tasks = backend.list_tasks(project_id, &filter).await?;

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&tasks)?);
        }
        OutputFormat::Text => {
            for task in &tasks {
                println!(
                    "[{}] #{} {} ({})",
                    task.status(), task.id(), task.title(), task.priority()
                );
            }
        }
    }
    Ok(())
}

pub async fn cmd_get(cli: &Cli, task_id: i64) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (backend, _) = create_backend(&root, &config)?;
    let project_id = resolve_project_id(&*backend, &config).await?;
    let task = backend.get_task(project_id, task_id).await?;

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&task)?);
        }
        OutputFormat::Text => {
            println!("ID:       {}", task.id());
            println!("Title:    {}", task.title());
            println!("Status:   {}", task.status());
            println!("Priority: {}", task.priority());
            if let Some(bg) = task.background() {
                println!("Background: {bg}");
            }
            if let Some(desc) = task.description() {
                println!("Description: {desc}");
            }
            if let Some(p) = task.plan() {
                println!("Plan:     {p}");
            }
            if let Some(branch) = task.branch() {
                println!("Branch:   {branch}");
            }
            if let Some(pr_url) = task.pr_url() {
                println!("PR URL:   {pr_url}");
            }
            if let Some(assignee) = task.assignee_session_id() {
                println!("Assignee (session): {assignee}");
            }
            if let Some(uid) = task.assignee_user_id() {
                println!("Assignee (user): #{uid}");
            }
            if !task.tags().is_empty() {
                println!("Tags:     {}", task.tags().join(", "));
            }
            if !task.dependencies().is_empty() {
                let deps: Vec<String> = task.dependencies().iter().map(|d| d.to_string()).collect();
                println!("Deps:     {}", deps.join(", "));
            }
            if let Some(meta) = task.metadata() {
                println!("Metadata: {}", serde_json::to_string_pretty(meta)?);
            }
            if !task.definition_of_done().is_empty() {
                println!("DoD:");
                for item in task.definition_of_done() {
                    let mark = if item.checked() { "x" } else { " " };
                    println!("  [{mark}] {}", item.content());
                }
            }
            if !task.in_scope().is_empty() {
                println!("In scope:");
                for item in task.in_scope() {
                    println!("  - {item}");
                }
            }
            if !task.out_of_scope().is_empty() {
                println!("Out of scope:");
                for item in task.out_of_scope() {
                    println!("  - {item}");
                }
            }
            println!("Created:  {}", task.created_at());
            println!("Updated:  {}", task.updated_at());
            if let Some(t) = task.started_at() {
                println!("Started:  {t}");
            }
            if let Some(t) = task.completed_at() {
                println!("Completed: {t}");
            }
            if let Some(t) = task.canceled_at() {
                println!("Canceled: {t}");
            }
            if let Some(reason) = task.cancel_reason() {
                println!("Cancel reason: {reason}");
            }
        }
    }
    Ok(())
}

pub async fn cmd_ready(cli: &Cli, id: i64) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (backend, using_http) = create_backend(&root, &config)?;
    let project_id = resolve_project_id(&*backend, &config).await?;

    if cli.dry_run {
        let task = backend.get_task(project_id, id).await?;
        task.status().transition_to(TaskStatus::Todo)?;
        let operations = vec![
            format!("Ready task #{} (status: {} → todo)", id, task.status()),
        ];
        return print_dry_run(&cli.output, &DryRunOperation { command: "ready".into(), operations });
    }

    let task_service = create_task_service(backend, &config, using_http, &root);
    let updated = task_service.ready_task(project_id, id).await?;

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&updated)?);
        }
        OutputFormat::Text => {
            println!("Ready task #{}: {}", updated.id(), updated.title());
        }
    }

    Ok(())
}

pub async fn cmd_start(cli: &Cli, id: i64, session_id: Option<String>, user_id: Option<i64>) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (backend, using_http) = create_backend(&root, &config)?;
    let project_id = resolve_project_id(&*backend, &config).await?;
    let user_id = match user_id {
        Some(id) => Some(id),
        None => Some(resolve_user_id(&*backend, &config).await?),
    };

    if cli.dry_run {
        let task = backend.get_task(project_id, id).await?;
        task.status().transition_to(TaskStatus::InProgress)?;
        let mut operations = vec![
            format!("Start task #{} (status: {} → in_progress)", id, task.status()),
        ];
        if let Some(ref sid) = session_id {
            operations.push(format!("Set assignee_session_id to \"{}\"", sid));
        }
        if let Some(uid) = user_id {
            operations.push(format!("Set assignee_user_id to {}", uid));
        }
        return print_dry_run(&cli.output, &DryRunOperation { command: "start".into(), operations });
    }

    let task_service = create_task_service(backend, &config, using_http, &root);
    let updated = task_service.start_task(project_id, id, session_id, user_id).await?;

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&updated)?);
        }
        OutputFormat::Text => {
            println!("Started task #{}: {}", updated.id(), updated.title());
        }
    }

    Ok(())
}

pub async fn cmd_next(cli: &Cli, session_id: Option<String>, user_id: Option<i64>) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (backend, using_http) = create_backend(&root, &config)?;
    let project_id = resolve_project_id(&*backend, &config).await?;
    let user_id = match user_id {
        Some(id) => Some(id),
        None => Some(resolve_user_id(&*backend, &config).await?),
    };

    if cli.dry_run {
        let backend_info = resolve_backend_info(&config, &root);
        let hook_executor = create_hook_executor(config, using_http, hooks::RuntimeMode::Cli, backend_info);
        let task = match backend.next_task(project_id).await? {
            Some(t) => t,
            None => {
                hook_executor.fire(&HookTrigger::NoEligibleTask { project_id }, None, backend.as_ref(), None, None).await;
                anyhow::bail!("no eligible task found");
            }
        };
        let mut operations = vec![
            format!("Start next eligible task #{}: \"{}\"", task.id(), task.title()),
            format!("Change status: {} → in_progress", task.status()),
        ];
        if let Some(ref sid) = session_id {
            operations.push(format!("Set assignee_session_id to \"{}\"", sid));
        }
        if let Some(uid) = user_id {
            operations.push(format!("Set assignee_user_id to {}", uid));
        }
        return print_dry_run(&cli.output, &DryRunOperation { command: "next".into(), operations });
    }

    // HttpBackend's next_task() already starts the task atomically,
    // so we handle the using_http case separately to avoid a redundant start_task call.
    if using_http {
        let backend_info = resolve_backend_info(&config, &root);
        let hook_executor = create_hook_executor(config, using_http, hooks::RuntimeMode::Cli, backend_info);
        let task = match backend.next_task(project_id).await? {
            Some(t) => t,
            None => {
                hook_executor.fire(&HookTrigger::NoEligibleTask { project_id }, None, backend.as_ref(), None, None).await;
                anyhow::bail!("no eligible task found");
            }
        };
        let prev_status = task.status();
        hook_executor
            .fire(&HookTrigger::Task(TaskEvent::Started), Some(&task), backend.as_ref(), Some(prev_status), None)
            .await;
        match cli.output {
            OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&task)?),
            OutputFormat::Text => println!("Started task #{}: {}", task.id(), task.title()),
        }
        return Ok(());
    }

    let task_service = create_task_service(backend, &config, using_http, &root);
    let updated = task_service.next_task(project_id, session_id, user_id).await?;

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&updated)?);
        }
        OutputFormat::Text => {
            println!("Started task #{}: {}", updated.id(), updated.title());
        }
    }

    Ok(())
}

pub async fn cmd_complete(cli: &Cli, id: i64, skip_pr_check: bool) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (backend, using_http) = create_backend(&root, &config)?;
    let project_id = resolve_project_id(&*backend, &config).await?;

    if cli.dry_run {
        let task = backend.get_task(project_id, id).await?;
        task.status().transition_to(TaskStatus::Completed)?;
        let operations = vec![
            format!("Complete task #{} (status: {} → completed)", id, task.status()),
        ];
        return print_dry_run(&cli.output, &DryRunOperation { command: "complete".into(), operations });
    }

    let task_service = create_task_service(backend, &config, using_http, &root);
    let updated = task_service.complete_task(project_id, id, skip_pr_check).await?;

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&updated)?);
        }
        OutputFormat::Text => {
            println!("Completed task #{}: {}", updated.id(), updated.title());
        }
    }

    Ok(())
}

pub async fn cmd_cancel(cli: &Cli, id: i64, reason: Option<String>) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (backend, using_http) = create_backend(&root, &config)?;
    let project_id = resolve_project_id(&*backend, &config).await?;

    if cli.dry_run {
        let task = backend.get_task(project_id, id).await?;
        task.status().transition_to(TaskStatus::Canceled)?;
        let mut operations = vec![
            format!("Cancel task #{} (status: {} → canceled)", id, task.status()),
        ];
        if let Some(ref r) = reason {
            operations.push(format!("Set cancel reason: \"{}\"", r));
        }
        return print_dry_run(&cli.output, &DryRunOperation { command: "cancel".into(), operations });
    }

    let task_service = create_task_service(backend, &config, using_http, &root);
    let updated = task_service.cancel_task(project_id, id, reason).await?;

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&updated)?);
        }
        OutputFormat::Text => {
            println!("Canceled task #{}: {}", updated.id(), updated.title());
            if let Some(r) = updated.cancel_reason() {
                println!("  reason: {r}");
            }
        }
    }

    Ok(())
}

pub fn cmd_config(cli: &Cli, init: bool) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;

    if init {
        let senko_dir = root.join(".senko");
        fs::create_dir_all(&senko_dir)?;
        let config_path = senko_dir.join("config.toml");
        if config_path.exists() {
            bail!(".senko/config.toml already exists. Remove it first to re-initialize.");
        }
        fs::write(&config_path, CONFIG_TEMPLATE)?;
        match cli.output {
            OutputFormat::Json => {
                println!(
                    "{}",
                    serde_json::json!({"path": config_path.display().to_string(), "action": "created"})
                );
            }
            OutputFormat::Text => {
                println!("Created {}", config_path.display());
            }
        }
        return Ok(());
    }

    let config = hooks::load_config(&root, cli.config.as_deref())?;
    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&config)?);
        }
        OutputFormat::Text => {
            println!("Configuration (.senko/config.toml):");
            println!("  [workflow]");
            println!(
                "    completion_mode: {}",
                config.workflow.completion_mode
            );
            println!("    auto_merge: {}", config.workflow.auto_merge);
            println!("  [hooks]");
            for (event, hooks) in [
                ("on_task_added", &config.hooks.on_task_added),
                ("on_task_ready", &config.hooks.on_task_ready),
                ("on_task_started", &config.hooks.on_task_started),
                ("on_task_completed", &config.hooks.on_task_completed),
                ("on_task_canceled", &config.hooks.on_task_canceled),
                ("on_no_eligible_task", &config.hooks.on_no_eligible_task),
            ] {
                if hooks.is_empty() {
                    println!("    {event}: (none)");
                } else {
                    println!("    {event}:");
                    for (name, entry) in hooks {
                        let status = if entry.enabled { "" } else { " [disabled]" };
                        println!("      {name}: {}{status}", entry.command);
                    }
                }
            }
            println!("  [backend]");
            match config.backend.api_url {
                Some(ref url) => println!("    api_url: {url}"),
                None => println!("    api_url: (none, using SQLite)"),
            }
            println!("    hook_mode: {:?}", config.backend.hook_mode);
            println!("  [project]");
            match config.project.name {
                Some(ref name) => println!("    name: {name}"),
                None => println!("    name: (none, using default)"),
            }
        }
    }

    Ok(())
}

// --- Doctor command ---

#[derive(Debug, serde::Serialize)]
struct DoctorReport {
    hooks: Vec<HookDiagnostic>,
    has_errors: bool,
}

#[derive(Debug, serde::Serialize)]
struct HookDiagnostic {
    event: String,
    name: String,
    command: String,
    checks: Vec<CheckResult>,
}

#[derive(Debug, serde::Serialize)]
struct CheckResult {
    check: String,
    target: String,
    status: CheckStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum CheckStatus {
    Ok,
    Error,
}

/// Expand leading `~` to the user's home directory.
fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    } else if path == "~" {
        if let Ok(home) = std::env::var("HOME") {
            return home;
        }
    }
    path.to_string()
}

/// Extract the script path from a hook command string, if it looks like a file path.
/// Returns None for shell builtins or commands without path separators.
fn extract_script_path(command: &str) -> Option<String> {
    let first_token = command.split_whitespace().next()?;
    let expanded = expand_tilde(first_token);
    if expanded.contains('/') || expanded.starts_with('.') {
        Some(expanded)
    } else {
        None
    }
}

fn run_hook_checks(entry: &crate::domain::config::HookEntry) -> Vec<CheckResult> {
    let mut checks = Vec::new();

    // Check requires_env
    for var in &entry.requires_env {
        let (status, message) = if std::env::var(var).is_ok() {
            (CheckStatus::Ok, None)
        } else {
            (CheckStatus::Error, Some(format!("{var} is not set")))
        };
        checks.push(CheckResult {
            check: "env_var".to_string(),
            target: var.clone(),
            status,
            message,
        });
    }

    // Check script existence and permissions
    if let Some(script_path) = extract_script_path(&entry.command) {
        let path = std::path::Path::new(&script_path);
        if path.exists() {
            checks.push(CheckResult {
                check: "script_exists".to_string(),
                target: script_path.clone(),
                status: CheckStatus::Ok,
                message: None,
            });

            // Check execute permission
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let is_executable = path
                    .metadata()
                    .map(|m| m.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false);
                let (status, message) = if is_executable {
                    (CheckStatus::Ok, None)
                } else {
                    (CheckStatus::Error, Some("not executable".to_string()))
                };
                checks.push(CheckResult {
                    check: "script_executable".to_string(),
                    target: script_path,
                    status,
                    message,
                });
            }
        } else {
            checks.push(CheckResult {
                check: "script_exists".to_string(),
                target: script_path,
                status: CheckStatus::Error,
                message: Some("file not found".to_string()),
            });
        }
    }

    checks
}

pub fn cmd_doctor(cli: &Cli) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = hooks::load_config(&root, cli.config.as_deref())?;

    let events = [
        ("on_task_added", &config.hooks.on_task_added),
        ("on_task_ready", &config.hooks.on_task_ready),
        ("on_task_started", &config.hooks.on_task_started),
        ("on_task_completed", &config.hooks.on_task_completed),
        ("on_task_canceled", &config.hooks.on_task_canceled),
        ("on_no_eligible_task", &config.hooks.on_no_eligible_task),
    ];

    let mut diagnostics = Vec::new();
    for (event_name, hook_map) in &events {
        for (name, entry) in *hook_map {
            if !entry.enabled {
                continue;
            }
            let checks = run_hook_checks(entry);
            diagnostics.push(HookDiagnostic {
                event: event_name.to_string(),
                name: name.clone(),
                command: entry.command.clone(),
                checks,
            });
        }
    }

    let has_errors = diagnostics
        .iter()
        .any(|d| d.checks.iter().any(|c| c.status == CheckStatus::Error));

    let report = DoctorReport {
        hooks: diagnostics,
        has_errors,
    };

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        OutputFormat::Text => {
            println!("Hook diagnostics");
            println!("================");
            if report.hooks.is_empty() {
                println!("\nNo hooks configured.");
            } else {
                for diag in &report.hooks {
                    println!("\n[{}] {}", diag.event, diag.name);
                    println!("  command: {}", diag.command);
                    for check in &diag.checks {
                        let icon = match check.status {
                            CheckStatus::Ok => "\u{2713}",
                            CheckStatus::Error => "\u{2717}",
                        };
                        let label = match check.check.as_str() {
                            "env_var" => format!("env {}", check.target),
                            "script_exists" => format!("script exists: {}", check.target),
                            "script_executable" => format!("script executable: {}", check.target),
                            _ => check.target.clone(),
                        };
                        match &check.message {
                            Some(msg) => println!("  [{icon}] {label} — {msg}"),
                            None => println!("  [{icon}] {label}"),
                        }
                    }
                }
            }
            let error_count: usize = report
                .hooks
                .iter()
                .flat_map(|d| &d.checks)
                .filter(|c| c.status == CheckStatus::Error)
                .count();
            if error_count > 0 {
                println!("\nResult: {error_count} issue(s) found");
            } else {
                println!("\nResult: all checks passed");
            }
        }
    }

    if has_errors {
        std::process::exit(1);
    }

    Ok(())
}

pub async fn cmd_hooks(cli: &Cli, command: &HooksCommand) -> Result<()> {
    match command {
        HooksCommand::Log {
            n,
            follow,
            clear,
            path,
        } => {
            let root = resolve_project_root(cli.project_root.as_deref())?;
            let config = load_config(cli, &root)?;
            let log_path = hooks::log_file_path_with_dir(config.log.dir.as_deref())
                .ok_or_else(|| anyhow::anyhow!("cannot determine log path: neither XDG_STATE_HOME nor HOME is set"))?;

            if *path {
                println!("{}", log_path.display());
                return Ok(());
            }

            if *clear {
                if log_path.exists() {
                    std::fs::remove_file(&log_path)?;
                    eprintln!("Cleared {}", log_path.display());
                } else {
                    eprintln!("No log file to clear");
                }
                return Ok(());
            }

            if *follow {
                return hooks_log_follow(&log_path);
            }

            // Show last N lines
            if !log_path.exists() {
                eprintln!("No hook log yet ({})", log_path.display());
                return Ok(());
            }

            let content = std::fs::read_to_string(&log_path)
                .context("failed to read hook log")?;
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(*n);
            for line in &lines[start..] {
                println!("{line}");
            }
            Ok(())
        }
        HooksCommand::Test {
            event_name,
            task_id,
            dry_run,
        } => {
            // Validate event name
            if HookTrigger::from_event_name(&event_name).is_none() {
                bail!(
                    "unknown event: {event_name}. Valid events: {}",
                    HookTrigger::valid_event_names().join(", ")
                );
            }

            let root = resolve_project_root(cli.project_root.as_deref())?;
            let config = load_config(cli, &root)?;
            let (backend, _) = create_backend(&root, &config)?;
            let project_id = resolve_project_id(&*backend, &config).await?;

            let backend_info = resolve_backend_info(&config, &root);

            let (envelope_project, envelope_user) = hooks::resolve_envelope_context(&config, &*backend).await;

            // no_eligible_task uses a different event structure (no task object)
            if event_name == "no_eligible_task" {
                let stats = backend.task_stats(project_id).await.unwrap_or_default();
                let ready_count = backend.ready_count(project_id).await.unwrap_or(0);
                let event = hooks::NoEligibleTaskEvent {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    event: "no_eligible_task".into(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    stats,
                    ready_count,
                };
                let envelope = hooks::HookEnvelope {
                    runtime: hooks::RuntimeMode::Cli,
                    backend: backend_info,
                    project: envelope_project,
                    user: envelope_user,
                    event,
                };
                let json = serde_json::to_string_pretty(&envelope)?;

                if *dry_run {
                    println!("{json}");
                    return Ok(());
                }

                let commands = hooks::get_commands_for_event(&config, event_name)
                    .expect("already validated event name");
                if commands.is_empty() {
                    eprintln!("No hooks configured for event: {event_name}");
                    return Ok(());
                }

                let compact_json = serde_json::to_string(&envelope)?;
                for (i, cmd) in commands.iter().enumerate() {
                    if commands.len() > 1 {
                        eprintln!("--- hook {}/{}: {} ---", i + 1, commands.len(), cmd);
                    }
                    match hooks::execute_hook_sync(cmd, &compact_json) {
                        Ok(status) => {
                            eprintln!("exit code: {}", status.code().unwrap_or(-1));
                        }
                        Err(e) => {
                            eprintln!("hook error: {e:#}");
                        }
                    }
                }

                return Ok(());
            }

            // Build the event using a real task or a sample task
            let task = if let Some(id) = task_id {
                backend.get_task(project_id, *id).await?
            } else {
                use crate::domain::task::{Priority, TaskStatus};
                Task::new(
                    0, project_id, "Sample task".into(), None,
                    Some("This is a sample task for hook testing".into()),
                    None, Priority::P2, TaskStatus::Todo, None, None,
                    chrono::Utc::now().to_rfc3339(), chrono::Utc::now().to_rfc3339(),
                    None, None, None, None, None, None, None,
                    vec![], vec![], vec![], vec![], vec![],
                )
            };

            let event = hooks::build_event(event_name, &task, &*backend, None, None).await;
            let envelope = hooks::HookEnvelope {
                runtime: hooks::RuntimeMode::Cli,
                backend: backend_info,
                project: envelope_project,
                user: envelope_user,
                event,
            };
            let json = serde_json::to_string_pretty(&envelope)?;

            if *dry_run {
                println!("{json}");
                return Ok(());
            }

            let commands = hooks::get_commands_for_event(&config, event_name)
                .expect("already validated event name");

            if commands.is_empty() {
                eprintln!("No hooks configured for event: {event_name}");
                return Ok(());
            }

            let compact_json = serde_json::to_string(&envelope)?;
            for (i, cmd) in commands.iter().enumerate() {
                if commands.len() > 1 {
                    eprintln!("--- hook {}/{}: {} ---", i + 1, commands.len(), cmd);
                }
                match hooks::execute_hook_sync(cmd, &compact_json) {
                    Ok(status) => {
                        eprintln!("exit code: {}", status.code().unwrap_or(-1));
                    }
                    Err(e) => {
                        eprintln!("hook error: {e:#}");
                    }
                }
            }

            Ok(())
        }
    }
}

fn hooks_log_follow(log_path: &std::path::Path) -> Result<()> {
    use std::io::{BufRead, BufReader, Seek, SeekFrom};

    // If file doesn't exist yet, wait for it
    if !log_path.exists() {
        eprintln!("Waiting for hook log ({})...", log_path.display());
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if log_path.exists() {
                break;
            }
        }
    }

    let mut file = std::fs::File::open(log_path)
        .context("failed to open hook log")?;
    // Seek to end — only show new lines
    file.seek(SeekFrom::End(0))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();

    eprintln!("Following {} (Ctrl+C to stop)...", log_path.display());
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => {
                // No new data — poll
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Ok(_) => {
                print!("{line}");
            }
            Err(e) => {
                bail!("error reading hook log: {e}");
            }
        }
    }
}

pub async fn cmd_edit(
    cli: &Cli,
    id: i64,
    title: &Option<String>,
    background: &Option<String>,
    clear_background: bool,
    description: &Option<String>,
    clear_description: bool,
    plan: &Option<String>,
    plan_file: &Option<PathBuf>,
    clear_plan: bool,
    priority: &Option<Priority>,
    branch: &Option<String>,
    clear_branch: bool,
    pr_url: &Option<String>,
    clear_pr_url: bool,
    metadata: &Option<String>,
    clear_metadata: bool,
    set_tags: &Option<Vec<String>>,
    set_definition_of_done: &Option<Vec<String>>,
    set_in_scope: &Option<Vec<String>>,
    set_out_of_scope: &Option<Vec<String>>,
    add_tag: &[String],
    add_definition_of_done: &[String],
    add_in_scope: &[String],
    add_out_of_scope: &[String],
    remove_tag: &[String],
    remove_definition_of_done: &[String],
    remove_in_scope: &[String],
    remove_out_of_scope: &[String],
) -> Result<()> {
    let project_root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &project_root)?;
    let (backend, _) = create_backend(&project_root, &config)?;
    let project_id = resolve_project_id(&*backend, &config).await?;

    // Verify task exists (even in dry-run)
    let _task = backend.get_task(project_id, id).await?;

    // Resolve effective plan: --plan-file takes precedence over --plan (they conflict via clap)
    let effective_plan = if let Some(path) = plan_file {
        Some(std::fs::read_to_string(path)?)
    } else {
        plan.clone()
    };

    if cli.dry_run {
        let mut operations = Vec::new();
        if let Some(t) = title {
            operations.push(format!("Update task #{}: set title to \"{}\"", id, t));
        }
        if clear_background {
            operations.push(format!("Update task #{}: clear background", id));
        } else if let Some(bg) = background {
            operations.push(format!("Update task #{}: set background to \"{}\"", id, bg));
        }
        if clear_description {
            operations.push(format!("Update task #{}: clear description", id));
        } else if let Some(desc) = description {
            operations.push(format!("Update task #{}: set description to \"{}\"", id, desc));
        }
        if clear_plan {
            operations.push(format!("Update task #{}: clear plan", id));
        } else if let Some(p) = &effective_plan {
            operations.push(format!("Update task #{}: set plan to \"{}\"", id, p));
        }
        if let Some(p) = priority {
            operations.push(format!("Update task #{}: set priority to {}", id, p));
        }
        if clear_branch {
            operations.push(format!("Update task #{}: clear branch", id));
        } else if let Some(b) = branch {
            operations.push(format!("Update task #{}: set branch to \"{}\"", id, b));
        }
        if clear_pr_url {
            operations.push(format!("Update task #{}: clear pr_url", id));
        } else if let Some(url) = pr_url {
            operations.push(format!("Update task #{}: set pr_url to \"{}\"", id, url));
        }
        if clear_metadata {
            operations.push(format!("Update task #{}: clear metadata", id));
        } else if let Some(m) = metadata {
            operations.push(format!("Update task #{}: set metadata to {}", id, m));
        }
        if let Some(tags) = set_tags {
            operations.push(format!("Update task #{}: set tags to [{}]", id, tags.join(", ")));
        }
        if !add_tag.is_empty() {
            operations.push(format!("Update task #{}: add tags [{}]", id, add_tag.join(", ")));
        }
        if !remove_tag.is_empty() {
            operations.push(format!("Update task #{}: remove tags [{}]", id, remove_tag.join(", ")));
        }
        if operations.is_empty() {
            operations.push(format!("Update task #{}: no changes", id));
        }
        return print_dry_run(&cli.output, &DryRunOperation { command: "edit".into(), operations });
    }

    let branch_value = if clear_branch {
        Some(None)
    } else {
        branch.as_ref().map(|b| Some(b.replace("${task_id}", &id.to_string())))
    };

    let scalar_params = UpdateTaskParams {
        title: title.clone(),
        background: if clear_background {
            Some(None)
        } else {
            background.clone().map(Some)
        },
        description: if clear_description {
            Some(None)
        } else {
            description.clone().map(Some)
        },
        plan: if clear_plan {
            Some(None)
        } else {
            effective_plan.map(Some)
        },
        priority: priority.clone(),
        assignee_session_id: None,
        assignee_user_id: None,
        started_at: None,
        completed_at: None,
        canceled_at: None,
        cancel_reason: None,
        branch: branch_value,
        pr_url: if clear_pr_url {
            Some(None)
        } else {
            pr_url.clone().map(Some)
        },
        metadata: if clear_metadata {
            Some(None)
        } else {
            match metadata {
                Some(m) => {
                    let val: serde_json::Value = serde_json::from_str(m)
                        .context("invalid JSON for --metadata")?;
                    Some(Some(val))
                }
                None => None,
            }
        },
    };

    let array_params = UpdateTaskArrayParams {
        set_tags: set_tags.clone(),
        add_tags: add_tag.to_vec(),
        remove_tags: remove_tag.to_vec(),
        set_definition_of_done: set_definition_of_done.clone(),
        add_definition_of_done: add_definition_of_done.to_vec(),
        remove_definition_of_done: remove_definition_of_done.to_vec(),
        set_in_scope: set_in_scope.clone(),
        add_in_scope: add_in_scope.to_vec(),
        remove_in_scope: remove_in_scope.to_vec(),
        set_out_of_scope: set_out_of_scope.clone(),
        add_out_of_scope: add_out_of_scope.to_vec(),
        remove_out_of_scope: remove_out_of_scope.to_vec(),
    };

    backend.update_task(project_id, id, &scalar_params).await?;
    backend.update_task_arrays(project_id, id, &array_params).await?;
    let task = backend.get_task(project_id, id).await?;

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&task)?);
        }
        OutputFormat::Text => {
            println!("Updated task {}", task.id());
            println!("  title: {}", task.title());
            println!("  status: {}", task.status());
            println!("  priority: {}", task.priority());
            if let Some(bg) = task.background() {
                println!("  background: {bg}");
            }
            if let Some(desc) = task.description() {
                println!("  description: {desc}");
            }
            if let Some(p) = task.plan() {
                println!("  plan: {p}");
            }
            if let Some(branch) = task.branch() {
                println!("  branch: {branch}");
            }
            if let Some(pr_url) = task.pr_url() {
                println!("  pr_url: {pr_url}");
            }
            if let Some(meta) = task.metadata() {
                println!("  metadata: {}", serde_json::to_string(meta)?);
            }
            if !task.tags().is_empty() {
                println!("  tags: {}", task.tags().join(", "));
            }
        }
    }
    Ok(())
}

pub async fn cmd_dod(cli: &Cli, command: &DodCommand) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (backend, using_http) = create_backend(&root, &config)?;
    let project_id = resolve_project_id(&*backend, &config).await?;
    let task_service = create_task_service(backend, &config, using_http, &root);

    match command {
        DodCommand::Check { task_id, index } => {
            let (task_id, index) = (*task_id, *index);
            if cli.dry_run {
                let operations =
                    vec![format!("Check DoD item #{index} of task #{task_id}")];
                return print_dry_run(
                    &cli.output,
                    &DryRunOperation {
                        command: "dod check".into(),
                        operations,
                    },
                );
            }
            let task = task_service.check_dod(project_id, task_id, index).await?;
            match cli.output {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&task)?),
                OutputFormat::Text => {
                    println!("Checked DoD item #{index} of task #{task_id}");
                    print_dod_items(task.definition_of_done());
                }
            }
        }
        DodCommand::Uncheck { task_id, index } => {
            let (task_id, index) = (*task_id, *index);
            if cli.dry_run {
                let operations =
                    vec![format!("Uncheck DoD item #{index} of task #{task_id}")];
                return print_dry_run(
                    &cli.output,
                    &DryRunOperation {
                        command: "dod uncheck".into(),
                        operations,
                    },
                );
            }
            let task = task_service.uncheck_dod(project_id, task_id, index).await?;
            match cli.output {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&task)?),
                OutputFormat::Text => {
                    println!("Unchecked DoD item #{index} of task #{task_id}");
                    print_dod_items(task.definition_of_done());
                }
            }
        }
    }
    Ok(())
}

fn print_dod_items(items: &[crate::domain::task::DodItem]) {
    for item in items {
        let mark = if item.checked() { "x" } else { " " };
        println!("  [{mark}] {}", item.content());
    }
}

pub async fn cmd_deps(cli: &Cli, command: &DepsCommand) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (backend, using_http) = create_backend(&root, &config)?;
    let project_id = resolve_project_id(&*backend, &config).await?;
    let task_service = create_task_service(backend, &config, using_http, &root);

    match command {
        DepsCommand::Add { task_id, on } => {
            let (task_id, on) = (*task_id, *on);
            if cli.dry_run {
                let operations = vec![format!("Add dependency: task #{} depends on #{}", task_id, on)];
                return print_dry_run(&cli.output, &DryRunOperation { command: "deps add".into(), operations });
            }
            let task = task_service.add_dependency(project_id, task_id, on).await?;
            match cli.output {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&task)?),
                OutputFormat::Text => println!("Added dependency: task #{} depends on #{}", task_id, on),
            }
        }
        DepsCommand::Remove { task_id, on } => {
            let (task_id, on) = (*task_id, *on);
            if cli.dry_run {
                let operations = vec![format!("Remove dependency: task #{} no longer depends on #{}", task_id, on)];
                return print_dry_run(&cli.output, &DryRunOperation { command: "deps remove".into(), operations });
            }
            let task = task_service.remove_dependency(project_id, task_id, on).await?;
            match cli.output {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&task)?),
                OutputFormat::Text => println!("Removed dependency: task #{} no longer depends on #{}", task_id, on),
            }
        }
        DepsCommand::Set { task_id, on } => {
            let task_id = *task_id;
            if cli.dry_run {
                let dep_strs: Vec<String> = on.iter().map(|d| format!("#{d}")).collect();
                let operations = vec![format!("Set dependencies for task #{}: [{}]", task_id, dep_strs.join(", "))];
                return print_dry_run(&cli.output, &DryRunOperation { command: "deps set".into(), operations });
            }
            let task = task_service.set_dependencies(project_id, task_id, on).await?;
            match cli.output {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&task)?),
                OutputFormat::Text => {
                    if task.dependencies().is_empty() {
                        println!("Cleared all dependencies for task #{}", task_id);
                    } else {
                        let dep_strs: Vec<String> = task.dependencies().iter().map(|d| format!("#{d}")).collect();
                        println!("Set dependencies for task #{}: {}", task_id, dep_strs.join(", "));
                    }
                }
            }
        }
        DepsCommand::List { task_id } => {
            // Read-only: ignore --dry-run
            let deps = task_service.list_dependencies(project_id, *task_id).await?;
            match cli.output {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&deps)?),
                OutputFormat::Text => {
                    for task in &deps {
                        println!("[{}] #{} {} ({})", task.status(), task.id(), task.title(), task.priority());
                    }
                }
            }
        }
    }
    Ok(())
}

pub async fn cmd_project(cli: &Cli, action: &ProjectAction) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (backend, _) = create_backend(&root, &config)?;
    let project_service = create_project_service(backend);

    match action {
        ProjectAction::List => {
            let projects = project_service.list_projects().await?;
            match cli.output {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&projects)?);
                }
                OutputFormat::Text => {
                    for project in &projects {
                        let desc = project
                            .description()
                            .unwrap_or("");
                        println!("#{} {} {}", project.id(), project.name(), desc);
                    }
                }
            }
        }
        ProjectAction::Create {
            name,
            description,
        } => {
            let params = CreateProjectParams {
                name: name.clone(),
                description: description.clone(),
            };
            let project = project_service.create_project(&params).await?;
            match cli.output {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&project)?);
                }
                OutputFormat::Text => {
                    println!("Created project #{}: {}", project.id(), project.name());
                }
            }
        }
        ProjectAction::Delete { id } => {
            project_service.delete_project(*id).await?;
            match cli.output {
                OutputFormat::Json => {
                    println!("{}", serde_json::json!({"deleted": id}));
                }
                OutputFormat::Text => {
                    println!("Deleted project #{}", id);
                }
            }
        }
    }
    Ok(())
}

pub async fn cmd_user(cli: &Cli, action: &UserAction) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (backend, _) = create_backend(&root, &config)?;
    let user_service = create_user_service(backend);

    match action {
        UserAction::List => {
            let users = user_service.list_users().await?;
            match cli.output {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&users)?);
                }
                OutputFormat::Text => {
                    for user in &users {
                        let display = user
                            .display_name()
                            .unwrap_or("");
                        println!("#{} {} {}", user.id(), user.username(), display);
                    }
                }
            }
        }
        UserAction::Create {
            username,
            display_name,
            email,
        } => {
            let params = CreateUserParams {
                username: username.clone(),
                display_name: display_name.clone(),
                email: email.clone(),
            };
            let user = user_service.create_user(&params).await?;
            match cli.output {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&user)?);
                }
                OutputFormat::Text => {
                    println!("Created user #{}: {}", user.id(), user.username());
                }
            }
        }
        UserAction::Delete { id } => {
            user_service.delete_user(*id).await?;
            match cli.output {
                OutputFormat::Json => {
                    println!("{}", serde_json::json!({"deleted": id}));
                }
                OutputFormat::Text => {
                    println!("Deleted user #{}", id);
                }
            }
        }
    }
    Ok(())
}

pub async fn cmd_members(cli: &Cli, action: &MemberAction) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (backend, _) = create_backend(&root, &config)?;
    let project_service = create_project_service(backend);

    match action {
        MemberAction::List => {
            let members = project_service.list_project_members(DEFAULT_PROJECT_ID).await?;
            match cli.output {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&members)?);
                }
                OutputFormat::Text => {
                    for member in &members {
                        println!(
                            "user #{} — role: {}",
                            member.user_id(), member.role()
                        );
                    }
                }
            }
        }
        MemberAction::Add { user_id, role } => {
            let params = AddProjectMemberParams::new(*user_id, *role);
            let member = project_service
                .add_project_member(DEFAULT_PROJECT_ID, &params)
                .await?;
            match cli.output {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&member)?);
                }
                OutputFormat::Text => {
                    println!(
                        "Added user #{} to project as {}",
                        member.user_id(), member.role()
                    );
                }
            }
        }
        MemberAction::Remove { user_id } => {
            project_service
                .remove_project_member(DEFAULT_PROJECT_ID, *user_id)
                .await?;
            match cli.output {
                OutputFormat::Json => {
                    println!(
                        "{}",
                        serde_json::json!({"removed_user_id": user_id})
                    );
                }
                OutputFormat::Text => {
                    println!("Removed user #{} from project", user_id);
                }
            }
        }
        MemberAction::SetRole { user_id, role } => {
            let member = project_service
                .update_member_role(DEFAULT_PROJECT_ID, *user_id, *role)
                .await?;
            match cli.output {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&member)?);
                }
                OutputFormat::Text => {
                    println!(
                        "Updated user #{} role to {}",
                        member.user_id(), member.role()
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{Command, OutputFormat};
    use crate::domain::repository::TaskRepository;

    #[tokio::test]
    async fn cmd_add_with_flags() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli {
            output: OutputFormat::Text,
            project_root: Some(tmp.path().to_path_buf()),
            config: None,
            dry_run: false,
            log_dir: None,
            db_path: Some(tmp.path().join("data.db")),
            postgres_url: None,
            project: None,
            user: None,
            command: Command::Add {
                title: None,
                background: None,
                description: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                tag: vec![],
                depends_on: vec![],
                from_json: false,
                branch: None,
                metadata: None,
                from_json_file: None,
            },
        };
        cmd_add(
            &cli,
            Some("test task".to_string()),
            Some("bg".to_string()),
            None,
            Some("p1".to_string()),
            vec!["done".to_string()],
            vec![],
            vec![],
            vec!["rust".to_string()],
            vec![],
            None,
            None,
            false,
            None,
        )
        .await
        .unwrap();

        let backend = crate::infra::sqlite::SqliteBackend::new(tmp.path(), Some(&tmp.path().join("data.db")), None).unwrap();
        let task = backend.get_task(DEFAULT_PROJECT_ID, 1).await.unwrap();
        assert_eq!(task.title(), "test task");
        assert_eq!(task.background(), Some("bg"));
        assert_eq!(task.priority(), crate::domain::task::Priority::P1);
        assert_eq!(task.definition_of_done().len(), 1);
        assert_eq!(task.definition_of_done()[0].content(), "done");
        assert!(!task.definition_of_done()[0].checked());
        assert_eq!(task.tags(), &["rust"]);
    }

    #[tokio::test]
    async fn cmd_add_with_from_json_file() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("task.json");
        std::fs::write(&json_path, r#"{"title":"file task","priority":"P0"}"#).unwrap();

        let cli = Cli {
            output: OutputFormat::Text,
            project_root: Some(tmp.path().to_path_buf()),
            config: None,
            dry_run: false,
            log_dir: None,
            db_path: Some(tmp.path().join("data.db")),
            postgres_url: None,
            project: None,
            user: None,
            command: Command::Add {
                title: None,
                background: None,
                description: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                tag: vec![],
                depends_on: vec![],
                from_json: false,
                branch: None,
                metadata: None,
                from_json_file: None,
            },
        };
        cmd_add(
            &cli,
            None,
            None,
            None,
            None,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            None,
            None,
            false,
            Some(json_path),
        )
        .await
        .unwrap();

        let backend = crate::infra::sqlite::SqliteBackend::new(tmp.path(), Some(&tmp.path().join("data.db")), None).unwrap();
        let task = backend.get_task(DEFAULT_PROJECT_ID, 1).await.unwrap();
        assert_eq!(task.title(), "file task");
        assert_eq!(task.priority(), crate::domain::task::Priority::P0);
    }

    #[tokio::test]
    async fn cmd_add_missing_title_error() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli {
            output: OutputFormat::Text,
            project_root: Some(tmp.path().to_path_buf()),
            config: None,
            dry_run: false,
            log_dir: None,
            db_path: Some(tmp.path().join("data.db")),
            postgres_url: None,
            project: None,
            user: None,
            command: Command::Add {
                title: None,
                background: None,
                description: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                tag: vec![],
                depends_on: vec![],
                from_json: false,
                branch: None,
                metadata: None,
                from_json_file: None,
            },
        };
        let result = cmd_add(
            &cli,
            None,
            None,
            None,
            None,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            None,
            None,
            false,
            None,
        ).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("--title is required"));
    }

    #[tokio::test]
    async fn cmd_add_text_output() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli {
            output: OutputFormat::Text,
            project_root: Some(tmp.path().to_path_buf()),
            config: None,
            dry_run: false,
            log_dir: None,
            db_path: Some(tmp.path().join("data.db")),
            postgres_url: None,
            project: None,
            user: None,
            command: Command::Add {
                title: None,
                background: None,
                description: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                tag: vec![],
                depends_on: vec![],
                from_json: false,
                branch: None,
                metadata: None,
                from_json_file: None,
            },
        };
        cmd_add(
            &cli,
            Some("my task".to_string()),
            None,
            None,
            None,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            None,
            None,
            false,
            None,
        )
        .await
        .unwrap();
        let backend = crate::infra::sqlite::SqliteBackend::new(tmp.path(), Some(&tmp.path().join("data.db")), None).unwrap();
        let task = backend.get_task(DEFAULT_PROJECT_ID, 1).await.unwrap();
        assert_eq!(task.title(), "my task");
    }

    #[tokio::test]
    async fn cmd_add_json_output() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli {
            output: OutputFormat::Json,
            project_root: Some(tmp.path().to_path_buf()),
            config: None,
            dry_run: false,
            log_dir: None,
            db_path: Some(tmp.path().join("data.db")),
            postgres_url: None,
            project: None,
            user: None,
            command: Command::Add {
                title: None,
                background: None,
                description: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                tag: vec![],
                depends_on: vec![],
                from_json: false,
                branch: None,
                metadata: None,
                from_json_file: None,
            },
        };
        cmd_add(
            &cli,
            Some("json out".to_string()),
            None,
            None,
            None,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            None,
            None,
            false,
            None,
        )
        .await
        .unwrap();
        let backend = crate::infra::sqlite::SqliteBackend::new(tmp.path(), Some(&tmp.path().join("data.db")), None).unwrap();
        let task = backend.get_task(DEFAULT_PROJECT_ID, 1).await.unwrap();
        assert_eq!(task.title(), "json out");
    }

    // --- Doctor tests ---

    #[test]
    fn expand_tilde_with_home() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(super::expand_tilde("~/foo/bar.sh"), format!("{home}/foo/bar.sh"));
    }

    #[test]
    fn expand_tilde_no_tilde() {
        assert_eq!(super::expand_tilde("/usr/bin/script.sh"), "/usr/bin/script.sh");
    }

    #[test]
    fn extract_script_path_absolute() {
        assert_eq!(
            super::extract_script_path("/usr/bin/my-hook.sh arg1 arg2"),
            Some("/usr/bin/my-hook.sh".to_string())
        );
    }

    #[test]
    fn extract_script_path_tilde() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(
            super::extract_script_path("~/hooks/run.sh --verbose"),
            Some(format!("{home}/hooks/run.sh"))
        );
    }

    #[test]
    fn extract_script_path_relative() {
        assert_eq!(
            super::extract_script_path("./scripts/hook.sh"),
            Some("./scripts/hook.sh".to_string())
        );
    }

    #[test]
    fn extract_script_path_bare_command() {
        // No path separator → not a file path
        assert_eq!(super::extract_script_path("echo hello"), None);
    }

    #[test]
    fn run_hook_checks_env_missing() {
        let entry = crate::domain::config::HookEntry {
            command: "echo test".to_string(),
            enabled: true,
            requires_env: vec!["SENKO_DOCTOR_TEST_NONEXISTENT_VAR_12345".to_string()],
        };
        let checks = super::run_hook_checks(&entry);
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].check, "env_var");
        assert_eq!(checks[0].status, super::CheckStatus::Error);
    }

    #[test]
    fn run_hook_checks_env_set() {
        unsafe { std::env::set_var("SENKO_DOCTOR_TEST_VAR_OK", "1"); }
        let entry = crate::domain::config::HookEntry {
            command: "echo test".to_string(),
            enabled: true,
            requires_env: vec!["SENKO_DOCTOR_TEST_VAR_OK".to_string()],
        };
        let checks = super::run_hook_checks(&entry);
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].status, super::CheckStatus::Ok);
        unsafe { std::env::remove_var("SENKO_DOCTOR_TEST_VAR_OK"); }
    }

    #[test]
    fn run_hook_checks_script_not_found() {
        let entry = crate::domain::config::HookEntry {
            command: "/nonexistent/path/hook.sh".to_string(),
            enabled: true,
            requires_env: vec![],
        };
        let checks = super::run_hook_checks(&entry);
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].check, "script_exists");
        assert_eq!(checks[0].status, super::CheckStatus::Error);
    }

    #[test]
    fn run_hook_checks_script_exists_and_executable() {
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("hook.sh");
        std::fs::write(&script, "#!/bin/sh\necho ok").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let entry = crate::domain::config::HookEntry {
            command: script.to_str().unwrap().to_string(),
            enabled: true,
            requires_env: vec![],
        };
        let checks = super::run_hook_checks(&entry);
        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].check, "script_exists");
        assert_eq!(checks[0].status, super::CheckStatus::Ok);
        assert_eq!(checks[1].check, "script_executable");
        assert_eq!(checks[1].status, super::CheckStatus::Ok);
    }

    #[test]
    fn run_hook_checks_script_not_executable() {
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("hook.sh");
        std::fs::write(&script, "#!/bin/sh\necho ok").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        let entry = crate::domain::config::HookEntry {
            command: script.to_str().unwrap().to_string(),
            enabled: true,
            requires_env: vec![],
        };
        let checks = super::run_hook_checks(&entry);
        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].check, "script_exists");
        assert_eq!(checks[0].status, super::CheckStatus::Ok);
        assert_eq!(checks[1].check, "script_executable");
        assert_eq!(checks[1].status, super::CheckStatus::Error);
    }

    #[test]
    fn run_hook_checks_bare_command_no_file_checks() {
        let entry = crate::domain::config::HookEntry {
            command: "echo hello world".to_string(),
            enabled: true,
            requires_env: vec![],
        };
        let checks = super::run_hook_checks(&entry);
        assert!(checks.is_empty());
    }
}
