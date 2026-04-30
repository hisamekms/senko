use anyhow::{Context, Result, bail};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{
    CONFIG_TEMPLATE, Cli, ContractAction, ContractDodCommand, ContractNoteCommand, DryRunOperation,
    HooksCommand, MemberAction, MetadataFieldAction, OutputFormat, ProjectAction, TaskDepsCommand,
    TaskDodCommand, UserAction, print_dry_run,
};
use crate::application::{ContractOperations, UserOperations};
use crate::application::{HookTrigger, ProjectOperations};
#[cfg(test)]
use crate::bootstrap::DEFAULT_PROJECT_ID;
use crate::bootstrap::hook as hooks;
use crate::bootstrap::resolve_project_root;
use crate::bootstrap::{
    create_backend, create_contract_service, create_hook_test_service, create_project_service,
    create_remote_contract_operations, create_remote_hook_data,
    create_remote_metadata_field_operations, create_remote_project_operations,
    create_remote_user_operations, create_task_operations, create_user_service, resolve_project_id,
    resolve_user_id,
};
use crate::domain::contract::{
    ContractId, CreateContractParams, ListContractNotesFilter, ListContractsFilter,
    UpdateContractArrayParams, UpdateContractParams,
};
use crate::domain::metadata_field::{
    CreateMetadataFieldParams, ListMetadataFieldsFilter, MetadataFieldType, validate_field_name,
};
use crate::domain::pagination::Cursor;
use crate::domain::project::{
    CreateProjectParams, ListProjectMembersFilter, ListProjectsFilter, ProjectId,
};
use crate::domain::task::{
    AssigneeUserId, CreateTaskParams, ListTaskDepsFilter, ListTasksFilter, MetadataUpdate,
    Priority, TaskId, TaskStatus, UpdateTaskArrayParams, UpdateTaskParams,
};
use crate::domain::user::{
    AddProjectMemberParams, CreateUserParams, ListUsersFilter, UpdateUserParams, UserId,
};
use crate::infra::config::{CliOverrides, Config};
use crate::presentation::dto::{ContractNoteResponse, ContractResponse};

fn build_cli_overrides(cli: &Cli) -> CliOverrides {
    CliOverrides {
        log_dir: cli
            .log_dir
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
        db_path: cli
            .db_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
        postgres_url: cli.postgres_url.clone(),
        project: cli.project.clone(),
        user: cli.user.clone(),
        ..Default::default()
    }
}

fn load_config(cli: &Cli, root: &std::path::Path) -> Result<Config> {
    let xdg = crate::infra::xdg::XdgDirs::from_env();
    load_config_with_xdg(cli, root, &xdg)
}

fn load_config_with_xdg(
    cli: &Cli,
    root: &std::path::Path,
    xdg: &crate::infra::xdg::XdgDirs,
) -> Result<Config> {
    let mut config = crate::bootstrap::load_config(root, cli.config.as_deref(), xdg)?;
    config.apply_cli(&build_cli_overrides(cli));
    ensure_cli_token(&mut config);
    Ok(config)
}

/// If the config targets a remote server and no token is set yet,
/// look up the cached auth_mode and load the appropriate token from the keychain.
fn ensure_cli_token(config: &mut Config) {
    let api_url = match config.cli.remote.url.as_deref() {
        Some(url) if config.cli.remote.token.is_none() => url.to_string(),
        _ => return,
    };

    let auth_mode = match super::auth_cache::get_cached_auth_mode(&config.xdg, &api_url) {
        Some(mode) => mode,
        None => return,
    };

    let token = match auth_mode.as_str() {
        "trusted_headers" => super::keychain::load_access_token(&api_url).ok(),
        _ => super::keychain::load(&api_url).ok(),
    };

    if let Some(t) = token {
        config.cli.remote.token = Some(t);
    }
}

async fn resolve_current_user_id(
    root: &Path,
    config: &Config,
    cli_attrs: &[(String, String)],
) -> Result<Option<UserId>> {
    if config.user.name.is_none() {
        return Ok(None);
    }
    let user_ops: Arc<dyn UserOperations> = if config.cli.remote.url.is_some() {
        Arc::new(create_remote_user_operations(config, cli_attrs))
    } else {
        let backend = create_backend(root, config)?;
        Arc::new(create_user_service(backend))
    };
    Ok(Some(resolve_user_id(&*user_ops, config).await?))
}

fn parse_assignee_user_id(value: &str) -> Result<AssigneeUserId> {
    if value == "self" {
        Ok(AssigneeUserId::SelfUser)
    } else {
        value
            .parse::<i64>()
            .map(|n| AssigneeUserId::Id(UserId(n)))
            .context("--assignee-user-id は 'self' または数値IDです")
    }
}

/// Resolve [`AssigneeUserId::SelfUser`] to a numeric ID.
/// Only needed in local mode; in remote mode the API resolves "self".
async fn resolve_assignee(
    value: AssigneeUserId,
    root: &Path,
    config: &Config,
    cli_attrs: &[(String, String)],
) -> Result<UserId> {
    match value {
        AssigneeUserId::Id(id) => Ok(id),
        AssigneeUserId::SelfUser => resolve_current_user_id(root, config, cli_attrs)
            .await?
            .context("'self' を解決できません: user.name が未設定です"),
    }
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
    depends_on: Vec<TaskId>,
    branch: Option<String>,
    metadata: Option<String>,
    from_json: bool,
    from_json_file: Option<PathBuf>,
    assignee_user_id: Option<String>,
) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (task_ops, project_ops) = create_task_operations(&root, &config, &cli.attr)?;
    let project_id = resolve_project_id(&*project_ops, &config).await?;

    let parsed_assignee = match assignee_user_id {
        Some(ref val) => Some(parse_assignee_user_id(val)?),
        None => None,
    };

    let mut params = if from_json {
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
                let val: serde_json::Value =
                    serde_json::from_str(&m).context("invalid JSON for --metadata")?;
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
            assignee_user_id: parsed_assignee.clone(),
            contract_id: None,
        }
    };

    // CLI --assignee-user-id overrides JSON input
    if parsed_assignee.is_some() {
        params.assignee_user_id = parsed_assignee;
    }

    // In local mode, resolve SelfUser to numeric ID before passing to backend
    if config.cli.remote.url.is_none()
        && let Some(AssigneeUserId::SelfUser) = &params.assignee_user_id
    {
        let uid = resolve_current_user_id(&root, &config, &cli.attr)
            .await?
            .context("'self' を解決できません: user.name が未設定です")?;
        params.assignee_user_id = Some(AssigneeUserId::Id(uid));
    }

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
            let deps: Vec<String> = params
                .dependencies
                .iter()
                .map(|d| format!("#{d}"))
                .collect();
            operations.push(format!("Set dependencies: {}", deps.join(", ")));
        }
        if !params.definition_of_done.is_empty() {
            operations.push(format!(
                "Set definition of done: {}",
                params.definition_of_done.join(", ")
            ));
        }
        if !params.in_scope.is_empty() {
            operations.push(format!("Set in scope: {}", params.in_scope.join(", ")));
        }
        if !params.out_of_scope.is_empty() {
            operations.push(format!(
                "Set out of scope: {}",
                params.out_of_scope.join(", ")
            ));
        }
        if let Some(ref b) = params.branch {
            operations.push(format!("Set branch to \"{}\"", b));
        }
        if let Some(ref m) = params.metadata {
            operations.push(format!("Set metadata to {}", m));
        }
        return print_dry_run(
            &cli.output,
            &DryRunOperation {
                command: "add".into(),
                operations,
            },
        );
    }

    let task = task_ops.create_task(project_id, &params).await?;

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

#[allow(clippy::too_many_arguments)]
pub async fn cmd_list(
    cli: &Cli,
    status: Vec<String>,
    tag: Vec<String>,
    depends_on: Option<TaskId>,
    ready: bool,
    include_unassigned: bool,
    metadata: Vec<String>,
    contract: Option<ContractId>,
    id_min: Option<TaskId>,
    id_max: Option<TaskId>,
    limit: Option<u32>,
    after: Option<String>,
) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (task_ops, project_ops) = create_task_operations(&root, &config, &cli.attr)?;
    let project_id = resolve_project_id(&*project_ops, &config).await?;

    let statuses = status
        .into_iter()
        .map(|s| s.parse::<TaskStatus>())
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("invalid status value")?;

    // For `--ready` we want to filter tasks assigned to the current user.
    // In local mode we resolve the numeric id up front. In remote mode we
    // cannot (the cluster-wide username→id lookup is master-gated), so we
    // signal the intent via `assignee_self` and let the upstream resolve it.
    let (assignee_user_id, assignee_self) = if ready && config.user.name.is_some() {
        if config.cli.remote.url.is_some() {
            (None, true)
        } else {
            (
                resolve_current_user_id(&root, &config, &cli.attr).await?,
                false,
            )
        }
    } else {
        (None, false)
    };

    let mut metadata_map = std::collections::HashMap::new();
    for entry in &metadata {
        let (key, value) = entry
            .split_once('=')
            .context("metadata filter must be in key=value format")?;
        metadata_map.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }

    if let Some(n) = limit
        && !(1..=200).contains(&n)
    {
        anyhow::bail!("--limit must be between 1 and 200");
    }
    let effective_limit = limit.or(Some(50));

    let after_id = match after.as_deref() {
        Some(raw) => {
            Some(Cursor::decode(raw).map_err(|_| anyhow::anyhow!("invalid --after cursor"))?)
        }
        None => None,
    };

    let filter = ListTasksFilter {
        statuses,
        tags: tag,
        depends_on,
        ready,
        assignee_user_id,
        assignee_self,
        include_unassigned,
        metadata: metadata_map,
        contract_id: contract,
        id_min,
        id_max,
        limit: effective_limit,
        after: after_id,
    };

    let page = task_ops.list_tasks(project_id, &filter).await?;

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&page)?);
        }
        OutputFormat::Text => {
            for task in &page.items {
                println!(
                    "[{}] #{} {} ({})",
                    task.status(),
                    task.id(),
                    task.title(),
                    task.priority()
                );
            }
            if let Some(cursor) = &page.next_cursor {
                println!("... more: --after {cursor}");
            }
        }
    }
    Ok(())
}

pub async fn cmd_get(cli: &Cli, task_id: TaskId) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (task_ops, project_ops) = create_task_operations(&root, &config, &cli.attr)?;
    let project_id = resolve_project_id(&*project_ops, &config).await?;
    let task = task_ops.get_task(project_id, task_id).await?;

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

pub async fn cmd_publish(cli: &Cli, id: TaskId) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (task_ops, project_ops) = create_task_operations(&root, &config, &cli.attr)?;
    let project_id = resolve_project_id(&*project_ops, &config).await?;

    if cli.dry_run {
        let result = task_ops
            .preview_transition(project_id, id, TaskStatus::Todo)
            .await?;
        if !result.allowed {
            anyhow::bail!("{}", result.reason.unwrap_or_default());
        }
        return print_dry_run(
            &cli.output,
            &DryRunOperation {
                command: "publish".into(),
                operations: result.operations,
            },
        );
    }

    let updated = task_ops.publish_task(project_id, id).await?;

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&updated)?);
        }
        OutputFormat::Text => {
            println!("Published task #{}: {}", updated.id(), updated.title());
        }
    }

    Ok(())
}

pub async fn cmd_start(
    cli: &Cli,
    id: TaskId,
    session_id: Option<String>,
    metadata: Option<String>,
) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (task_ops, project_ops) = create_task_operations(&root, &config, &cli.attr)?;
    let project_id = resolve_project_id(&*project_ops, &config).await?;
    // In remote mode, the server derives user_id from the authenticated caller;
    // the CLI must not hit GET /api/v1/users (master-gated) just to convert
    // config.user.name. In local mode we still need the numeric id so the
    // domain's auto-assign-on-start works (see test_assignee_user_id.sh).
    let user_id = if config.cli.remote.url.is_some() {
        None
    } else {
        resolve_current_user_id(&root, &config, &cli.attr).await?
    };
    let metadata: Option<MetadataUpdate> = metadata
        .map(|s| -> Result<MetadataUpdate> {
            let val: serde_json::Value = serde_json::from_str(&s)
                .map_err(|e| anyhow::anyhow!("invalid metadata JSON: {}", e))?;
            Ok(MetadataUpdate::Merge(val))
        })
        .transpose()?;

    if cli.dry_run {
        let mut result = task_ops
            .preview_transition(project_id, id, TaskStatus::InProgress)
            .await?;
        if !result.allowed {
            anyhow::bail!("{}", result.reason.unwrap_or_default());
        }
        if let Some(ref sid) = session_id {
            result
                .operations
                .push(format!("Set assignee_session_id to \"{}\"", sid));
        }
        if metadata.is_some() {
            result.operations.push("Merge metadata".to_string());
        }
        return print_dry_run(
            &cli.output,
            &DryRunOperation {
                command: "start".into(),
                operations: result.operations,
            },
        );
    }

    let updated = task_ops
        .start_task(project_id, id, session_id, user_id, metadata)
        .await?;

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

pub async fn cmd_resume(
    cli: &Cli,
    id: TaskId,
    session_id: Option<String>,
    metadata: Option<String>,
) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (task_ops, project_ops) = create_task_operations(&root, &config, &cli.attr)?;
    let project_id = resolve_project_id(&*project_ops, &config).await?;
    let metadata: Option<MetadataUpdate> = metadata
        .map(|s| -> Result<MetadataUpdate> {
            let val: serde_json::Value = serde_json::from_str(&s)
                .map_err(|e| anyhow::anyhow!("invalid metadata JSON: {}", e))?;
            Ok(MetadataUpdate::Merge(val))
        })
        .transpose()?;

    if cli.dry_run {
        let mut result = task_ops
            .preview_transition(project_id, id, TaskStatus::InProgress)
            .await?;
        if !result.allowed {
            anyhow::bail!("{}", result.reason.unwrap_or_default());
        }
        if let Some(ref sid) = session_id {
            result
                .operations
                .push(format!("Set assignee_session_id to \"{}\"", sid));
        }
        if metadata.is_some() {
            result.operations.push("Merge metadata".to_string());
        }
        return print_dry_run(
            &cli.output,
            &DryRunOperation {
                command: "resume".into(),
                operations: result.operations,
            },
        );
    }

    let updated = task_ops
        .resume_task(project_id, id, session_id, metadata)
        .await?;

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&updated)?);
        }
        OutputFormat::Text => {
            println!("Resumed task #{}: {}", updated.id(), updated.title());
        }
    }

    Ok(())
}

pub async fn cmd_next(
    cli: &Cli,
    session_id: Option<String>,
    metadata: Option<String>,
    include_unassigned: bool,
) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (task_ops, project_ops) = create_task_operations(&root, &config, &cli.attr)?;
    let project_id = resolve_project_id(&*project_ops, &config).await?;
    // In remote mode, the server derives user_id from the authenticated caller;
    // see cmd_start for the rationale.
    let user_id = if config.cli.remote.url.is_some() {
        None
    } else {
        resolve_current_user_id(&root, &config, &cli.attr).await?
    };
    let metadata: Option<MetadataUpdate> = metadata
        .map(|s| -> Result<MetadataUpdate> {
            let val: serde_json::Value = serde_json::from_str(&s)
                .map_err(|e| anyhow::anyhow!("invalid metadata JSON: {}", e))?;
            Ok(MetadataUpdate::Merge(val))
        })
        .transpose()?;

    if cli.dry_run {
        let result = task_ops.preview_next(project_id).await?;
        let mut operations = result.operations;
        if let Some(ref sid) = session_id {
            operations.push(format!("Set assignee_session_id to \"{}\"", sid));
        }
        if metadata.is_some() {
            operations.push("Merge metadata".to_string());
        }
        return print_dry_run(
            &cli.output,
            &DryRunOperation {
                command: "next".into(),
                operations,
            },
        );
    }

    let updated = task_ops
        .next_task(
            project_id,
            session_id,
            user_id,
            include_unassigned,
            metadata,
        )
        .await?;

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

pub async fn cmd_complete(cli: &Cli, id: TaskId, skip_pr_check: bool) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (task_ops, project_ops) = create_task_operations(&root, &config, &cli.attr)?;
    let project_id = resolve_project_id(&*project_ops, &config).await?;

    if cli.dry_run {
        let result = task_ops
            .preview_transition(project_id, id, TaskStatus::Completed)
            .await?;
        if !result.allowed {
            anyhow::bail!("{}", result.reason.unwrap_or_default());
        }
        return print_dry_run(
            &cli.output,
            &DryRunOperation {
                command: "complete".into(),
                operations: result.operations,
            },
        );
    }

    let result = task_ops
        .complete_task(project_id, id, skip_pr_check)
        .await?;

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&result.task)?);
        }
        OutputFormat::Text => {
            println!(
                "Completed task #{}: {}",
                result.task.id(),
                result.task.title()
            );
        }
    }

    Ok(())
}

pub async fn cmd_cancel(cli: &Cli, id: TaskId, reason: Option<String>) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (task_ops, project_ops) = create_task_operations(&root, &config, &cli.attr)?;
    let project_id = resolve_project_id(&*project_ops, &config).await?;

    if cli.dry_run {
        let mut result = task_ops
            .preview_transition(project_id, id, TaskStatus::Canceled)
            .await?;
        if !result.allowed {
            anyhow::bail!("{}", result.reason.unwrap_or_default());
        }
        if let Some(ref r) = reason {
            result
                .operations
                .push(format!("Set cancel reason: \"{}\"", r));
        }
        return print_dry_run(
            &cli.output,
            &DryRunOperation {
                command: "cancel".into(),
                operations: result.operations,
            },
        );
    }

    let updated = task_ops.cancel_task(project_id, id, reason).await?;

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

    let xdg = crate::infra::xdg::XdgDirs::from_env();
    let config = crate::bootstrap::load_config(&root, cli.config.as_deref(), &xdg)?;
    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&config)?);
        }
        OutputFormat::Text => {
            println!("Configuration (.senko/config.toml):");
            println!("  [workflow]");
            println!("    merge_via: {}", config.workflow.merge_via);
            println!("    auto_merge: {}", config.workflow.auto_merge);
            println!("    branch_mode: {}", config.workflow.branch_mode);
            println!("    merge_strategy: {}", config.workflow.merge_strategy);
            println!("  [cli hooks]");
            print_task_action_hooks("cli", &config.cli.hooks);
            println!("  [server.relay hooks]");
            print_task_action_hooks("server.relay", &config.server.relay.hooks);
            println!("  [server.remote hooks]");
            print_task_action_hooks("server.remote", &config.server.remote.hooks);
            println!("  [workflow stages]");
            if config.workflow.stages.is_empty() {
                println!("    (none)");
            } else {
                let mut stage_names: Vec<&String> = config.workflow.stages.keys().collect();
                stage_names.sort();
                for stage_name in stage_names {
                    let stage = &config.workflow.stages[stage_name];
                    println!(
                        "    {stage_name}: {} hook(s), {} instruction(s)",
                        stage.hooks.len(),
                        stage.instructions.len()
                    );
                    for (name, def) in &stage.hooks {
                        let status = if def.enabled { "" } else { " [disabled]" };
                        println!("      {name}: {}{status}", def.command);
                    }
                }
            }
            println!("  [cli]");
            println!("    browser: {}", config.cli.browser);
            println!("  [cli.remote]");
            match config.cli.remote.url {
                Some(ref url) => println!("    url: {url}"),
                None => println!("    url: (none, using local backend)"),
            }
            println!("  [server.relay]");
            match config.server.relay.url {
                Some(ref url) => println!("    url: {url}"),
                None => println!("    url: (none, relay mode disabled)"),
            }
            println!("  [server.auth.oidc]");
            match config.server.auth.oidc.issuer_url {
                Some(ref url) => println!("    issuer_url: {url}"),
                None => println!("    issuer_url: (none)"),
            }
            match config.server.auth.oidc.client_id {
                Some(ref id) => println!("    client_id: {id}"),
                None => println!("    client_id: (none)"),
            }
            println!(
                "    scopes: [{}]",
                config.server.auth.oidc.scopes.join(", ")
            );
            if config.server.auth.oidc.callback_ports.is_empty() {
                println!("    callback_ports: (none)");
            } else {
                println!(
                    "    callback_ports: [{}]",
                    config.server.auth.oidc.callback_ports.join(", ")
                );
            }
            println!("  [server.auth.oidc.session]");
            match config.server.auth.oidc.session.ttl {
                Some(ref ttl) => println!("    ttl: {ttl}"),
                None => println!("    ttl: (none)"),
            }
            match config.server.auth.oidc.session.inactive_ttl {
                Some(ref ttl) => println!("    inactive_ttl: {ttl}"),
                None => println!("    inactive_ttl: (none)"),
            }
            match config.server.auth.oidc.session.max_per_user {
                Some(n) => println!("    max_per_user: {n}"),
                None => println!("    max_per_user: (none)"),
            }
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
    } else if path == "~"
        && let Ok(home) = std::env::var("HOME")
    {
        return home;
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

fn print_task_action_hooks(label: &str, hooks: &crate::infra::config::TaskActionHooks) {
    let sections: [(&str, &crate::infra::config::ActionConfig); 7] = [
        ("task_add", &hooks.task_add),
        ("task_publish", &hooks.task_publish),
        ("task_start", &hooks.task_start),
        ("task_resume", &hooks.task_resume),
        ("task_complete", &hooks.task_complete),
        ("task_cancel", &hooks.task_cancel),
        ("task_select", &hooks.task_select),
    ];
    let mut any = false;
    for (action, action_cfg) in sections {
        if action_cfg.hooks.is_empty() {
            continue;
        }
        any = true;
        println!("    {label}.{action}:");
        for (name, def) in &action_cfg.hooks {
            let status = if def.enabled { "" } else { " [disabled]" };
            println!("      {name}: {}{status}", def.command);
        }
    }
    if !any {
        println!("    (no hooks)");
    }
}

fn run_hook_checks(def: &crate::infra::config::HookDef) -> Vec<CheckResult> {
    let mut checks = Vec::new();

    // Check required env_vars (required=true + unset + no default)
    for spec in &def.env_vars {
        if !spec.required {
            continue;
        }
        if std::env::var(&spec.name).is_ok() || spec.default.is_some() {
            checks.push(CheckResult {
                check: "env_var".to_string(),
                target: spec.name.clone(),
                status: CheckStatus::Ok,
                message: None,
            });
        } else {
            checks.push(CheckResult {
                check: "env_var".to_string(),
                target: spec.name.clone(),
                status: CheckStatus::Error,
                message: Some(format!("{} is not set and has no default", spec.name)),
            });
        }
    }

    // Check script existence and permissions
    if let Some(script_path) = extract_script_path(&def.command) {
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
    let xdg = crate::infra::xdg::XdgDirs::from_env();
    let config = crate::bootstrap::load_config(&root, cli.config.as_deref(), &xdg)?;

    let runtime_sections: [(&str, &crate::infra::config::TaskActionHooks); 3] = [
        ("cli", &config.cli.hooks),
        ("server.relay", &config.server.relay.hooks),
        ("server.remote", &config.server.remote.hooks),
    ];

    let mut diagnostics = Vec::new();
    for (runtime_label, action_hooks) in runtime_sections {
        let actions: [(&str, &crate::infra::config::ActionConfig); 7] = [
            ("task_add", &action_hooks.task_add),
            ("task_publish", &action_hooks.task_publish),
            ("task_start", &action_hooks.task_start),
            ("task_resume", &action_hooks.task_resume),
            ("task_complete", &action_hooks.task_complete),
            ("task_cancel", &action_hooks.task_cancel),
            ("task_select", &action_hooks.task_select),
        ];
        for (action, action_cfg) in actions {
            for (name, def) in &action_cfg.hooks {
                if !def.enabled {
                    continue;
                }
                let checks = run_hook_checks(def);
                diagnostics.push(HookDiagnostic {
                    event: format!("{runtime_label}.{action}"),
                    name: name.clone(),
                    command: def.command.clone(),
                    checks,
                });
            }
        }
    }

    for (stage_name, stage) in &config.workflow.stages {
        for (name, def) in &stage.hooks {
            if !def.enabled {
                continue;
            }
            let checks = run_hook_checks(def);
            diagnostics.push(HookDiagnostic {
                event: format!("workflow.{stage_name}"),
                name: name.clone(),
                command: def.command.clone(),
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

/// Generate the OpenAPI 3.x JSON document and write it to disk.
///
/// Default output: `docs/openapi/openapi.json` (relative to the current
/// working directory). Writes the file via a tmp-then-rename so partial
/// writes never leave a half-baked artifact behind.
pub fn cmd_openapi_dump(output: Option<&std::path::Path>) -> Result<()> {
    use std::io::Write;

    let default_path = std::path::PathBuf::from("docs/openapi/openapi.json");
    let path = output
        .map(std::path::Path::to_path_buf)
        .unwrap_or(default_path);

    let api = crate::presentation::api::build_openapi();
    let body = serde_json::to_string_pretty(&api)
        .context("failed to serialize OpenAPI document to JSON")?;

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {parent:?}"))?;
    }

    let tmp_path = path.with_extension("json.tmp");
    {
        let mut tmp = std::fs::File::create(&tmp_path)
            .with_context(|| format!("failed to open {tmp_path:?} for writing"))?;
        tmp.write_all(body.as_bytes())
            .with_context(|| format!("failed to write {tmp_path:?}"))?;
        tmp.write_all(b"\n").ok();
    }
    std::fs::rename(&tmp_path, &path)
        .with_context(|| format!("failed to rename {tmp_path:?} -> {path:?}"))?;

    println!("wrote OpenAPI spec to {}", path.display());
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
            let log_path = hooks::log_file_path_with_dir(config.log.dir.as_deref(), &config.xdg)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "cannot determine log path: neither XDG_STATE_HOME nor HOME is set"
                    )
                })?;

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

            let content = std::fs::read_to_string(&log_path).context("failed to read hook log")?;
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
            // Validate event name (input validation stays in presentation layer)
            if HookTrigger::from_event_name(event_name).is_none() {
                bail!(
                    "unknown event: {event_name}. Valid events: {}",
                    HookTrigger::valid_event_names().join(", ")
                );
            }

            let root = resolve_project_root(cli.project_root.as_deref())?;
            let config = load_config(cli, &root)?;
            let (hook_data, project_ops): (
                std::sync::Arc<dyn crate::application::port::HookDataSource>,
                std::sync::Arc<dyn ProjectOperations>,
            ) = if config.cli.remote.url.is_some() {
                (
                    create_remote_hook_data(&config, &cli.attr),
                    std::sync::Arc::new(create_remote_project_operations(&config, &cli.attr)),
                )
            } else {
                let backend = create_backend(&root, &config)?;
                let hook_data: std::sync::Arc<dyn crate::application::port::HookDataSource> =
                    std::sync::Arc::new(crate::application::port::BackendHookData(backend.clone()));
                (
                    hook_data,
                    std::sync::Arc::new(crate::bootstrap::create_project_service(backend)),
                )
            };
            let project_id = resolve_project_id(&*project_ops, &config).await?;

            let hook_test_service = create_hook_test_service(hook_data, &config, &root);
            let output = hook_test_service
                .test_event(project_id, event_name, *task_id, *dry_run)
                .await?;

            use crate::application::hook_test_service::HookTestOutput;
            match output {
                HookTestOutput::DryRun { envelope_json } => {
                    println!("{envelope_json}");
                }
                HookTestOutput::NoHooksConfigured => {
                    eprintln!("No hooks configured for event: {event_name}");
                }
                HookTestOutput::Executed { results } => {
                    for r in &results {
                        if results.len() > 1 {
                            eprintln!("--- hook {}/{}: {} ---", r.index, r.total, r.command);
                        }
                        match &r.error {
                            Some(e) => eprintln!("hook error: {e}"),
                            None => {
                                eprintln!("exit code: {}", r.exit_code.unwrap_or(-1));
                            }
                        }
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

    let mut file = std::fs::File::open(log_path).context("failed to open hook log")?;
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

#[allow(clippy::too_many_arguments)]
pub async fn cmd_edit(
    cli: &Cli,
    id: TaskId,
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
    contract: &Option<ContractId>,
    clear_contract: bool,
    metadata: &Option<String>,
    replace_metadata: &Option<String>,
    clear_metadata: bool,
    assignee_user_id: &Option<String>,
    clear_assignee_user_id: bool,
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
    let (task_ops, project_ops) = create_task_operations(&project_root, &config, &cli.attr)?;
    let project_id = resolve_project_id(&*project_ops, &config).await?;

    // Verify task exists (even in dry-run)
    let _task = task_ops.get_task(project_id, id).await?;

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
            operations.push(format!(
                "Update task #{}: set description to \"{}\"",
                id, desc
            ));
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
        if clear_contract {
            operations.push(format!("Update task #{}: clear contract", id));
        } else if let Some(cid) = contract {
            operations.push(format!("Update task #{}: set contract to {}", id, cid));
        }
        if clear_metadata {
            operations.push(format!("Update task #{}: clear metadata", id));
        } else if let Some(m) = replace_metadata {
            operations.push(format!("Update task #{}: replace metadata with {}", id, m));
        } else if let Some(m) = metadata {
            operations.push(format!("Update task #{}: merge metadata with {}", id, m));
        }
        if let Some(tags) = set_tags {
            operations.push(format!(
                "Update task #{}: set tags to [{}]",
                id,
                tags.join(", ")
            ));
        }
        if !add_tag.is_empty() {
            operations.push(format!(
                "Update task #{}: add tags [{}]",
                id,
                add_tag.join(", ")
            ));
        }
        if !remove_tag.is_empty() {
            operations.push(format!(
                "Update task #{}: remove tags [{}]",
                id,
                remove_tag.join(", ")
            ));
        }
        if operations.is_empty() {
            operations.push(format!("Update task #{}: no changes", id));
        }
        return print_dry_run(
            &cli.output,
            &DryRunOperation {
                command: "edit".into(),
                operations,
            },
        );
    }

    let branch_value = if clear_branch {
        Some(None)
    } else {
        branch
            .as_ref()
            .map(|b| Some(b.replace("${task_id}", &id.to_string())))
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
        priority: *priority,
        assignee_session_id: None,
        assignee_user_id: if clear_assignee_user_id {
            Some(None)
        } else if let Some(val) = assignee_user_id {
            let parsed = parse_assignee_user_id(val)?;
            if config.cli.remote.url.is_none() {
                // Local mode: resolve SelfUser to numeric ID
                Some(Some(AssigneeUserId::Id(
                    resolve_assignee(parsed, &project_root, &config, &cli.attr).await?,
                )))
            } else {
                // Remote mode: send as-is (API resolves "self")
                Some(Some(parsed))
            }
        } else {
            None
        },
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
        contract_id: if clear_contract {
            Some(None)
        } else {
            contract.map(Some)
        },
        metadata: if clear_metadata {
            Some(MetadataUpdate::Clear)
        } else if let Some(m) = replace_metadata {
            let val: serde_json::Value =
                serde_json::from_str(m).context("invalid JSON for --replace-metadata")?;
            Some(MetadataUpdate::Replace(val))
        } else {
            match metadata {
                Some(m) => {
                    let val: serde_json::Value =
                        serde_json::from_str(m).context("invalid JSON for --metadata")?;
                    Some(MetadataUpdate::Merge(val))
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

    task_ops.edit_task(project_id, id, &scalar_params).await?;
    task_ops
        .edit_task_arrays(project_id, id, &array_params)
        .await?;
    let task = task_ops.get_task(project_id, id).await?;

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

pub async fn cmd_dod(cli: &Cli, command: &TaskDodCommand) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (task_ops, project_ops) = create_task_operations(&root, &config, &cli.attr)?;
    let project_id = resolve_project_id(&*project_ops, &config).await?;

    match command {
        TaskDodCommand::Check { task_id, index } => {
            let (task_id, index) = (*task_id, *index);
            if cli.dry_run {
                let operations = vec![format!("Check DoD item #{index} of task #{task_id}")];
                return print_dry_run(
                    &cli.output,
                    &DryRunOperation {
                        command: "dod check".into(),
                        operations,
                    },
                );
            }
            let task = task_ops.check_dod(project_id, task_id, index).await?;
            match cli.output {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&task)?),
                OutputFormat::Text => {
                    println!("Checked DoD item #{index} of task #{task_id}");
                    print_dod_items(task.definition_of_done());
                }
            }
        }
        TaskDodCommand::Uncheck { task_id, index } => {
            let (task_id, index) = (*task_id, *index);
            if cli.dry_run {
                let operations = vec![format!("Uncheck DoD item #{index} of task #{task_id}")];
                return print_dry_run(
                    &cli.output,
                    &DryRunOperation {
                        command: "dod uncheck".into(),
                        operations,
                    },
                );
            }
            let task = task_ops.uncheck_dod(project_id, task_id, index).await?;
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

pub async fn cmd_deps(cli: &Cli, command: &TaskDepsCommand) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let (task_ops, project_ops) = create_task_operations(&root, &config, &cli.attr)?;
    let project_id = resolve_project_id(&*project_ops, &config).await?;

    match command {
        TaskDepsCommand::Add { task_id, on } => {
            let (task_id, on) = (*task_id, *on);
            if cli.dry_run {
                let operations = vec![format!(
                    "Add dependency: task #{} depends on #{}",
                    task_id, on
                )];
                return print_dry_run(
                    &cli.output,
                    &DryRunOperation {
                        command: "deps add".into(),
                        operations,
                    },
                );
            }
            let task = task_ops.add_dependency(project_id, task_id, on).await?;
            match cli.output {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&task)?),
                OutputFormat::Text => {
                    println!("Added dependency: task #{} depends on #{}", task_id, on)
                }
            }
        }
        TaskDepsCommand::Remove { task_id, on } => {
            let (task_id, on) = (*task_id, *on);
            if cli.dry_run {
                let operations = vec![format!(
                    "Remove dependency: task #{} no longer depends on #{}",
                    task_id, on
                )];
                return print_dry_run(
                    &cli.output,
                    &DryRunOperation {
                        command: "deps remove".into(),
                        operations,
                    },
                );
            }
            let task = task_ops.remove_dependency(project_id, task_id, on).await?;
            match cli.output {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&task)?),
                OutputFormat::Text => println!(
                    "Removed dependency: task #{} no longer depends on #{}",
                    task_id, on
                ),
            }
        }
        TaskDepsCommand::Set { task_id, on } => {
            let task_id = *task_id;
            if cli.dry_run {
                let dep_strs: Vec<String> = on.iter().map(|d| format!("#{d}")).collect();
                let operations = vec![format!(
                    "Set dependencies for task #{}: [{}]",
                    task_id,
                    dep_strs.join(", ")
                )];
                return print_dry_run(
                    &cli.output,
                    &DryRunOperation {
                        command: "deps set".into(),
                        operations,
                    },
                );
            }
            let task = task_ops.set_dependencies(project_id, task_id, on).await?;
            match cli.output {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&task)?),
                OutputFormat::Text => {
                    if task.dependencies().is_empty() {
                        println!("Cleared all dependencies for task #{}", task_id);
                    } else {
                        let dep_strs: Vec<String> = task
                            .dependencies()
                            .iter()
                            .map(|d| format!("#{d}"))
                            .collect();
                        println!(
                            "Set dependencies for task #{}: {}",
                            task_id,
                            dep_strs.join(", ")
                        );
                    }
                }
            }
        }
        TaskDepsCommand::List {
            task_id,
            limit,
            after,
        } => {
            // Read-only: ignore --dry-run
            let after = match after.as_deref() {
                Some(raw) => Some(
                    Cursor::decode::<TaskId>(raw)
                        .map_err(|_| anyhow::anyhow!("invalid --after cursor"))?,
                ),
                None => None,
            };
            let filter = ListTaskDepsFilter {
                limit: *limit,
                after,
            };
            let page = task_ops
                .list_dependencies(project_id, *task_id, &filter)
                .await?;
            match cli.output {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&page)?),
                OutputFormat::Text => {
                    for task in &page.items {
                        println!(
                            "[{}] #{} {} ({})",
                            task.status(),
                            task.id(),
                            task.title(),
                            task.priority()
                        );
                    }
                    if let Some(c) = &page.next_cursor {
                        println!("... more: --after {c}");
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

    let (project_ops, metadata_ops): (
        std::sync::Arc<dyn ProjectOperations>,
        std::sync::Arc<dyn crate::application::MetadataFieldOperations>,
    ) = if config.cli.remote.url.is_some() {
        (
            std::sync::Arc::new(create_remote_project_operations(&config, &cli.attr)),
            std::sync::Arc::new(create_remote_metadata_field_operations(&config, &cli.attr)),
        )
    } else {
        let backend = create_backend(&root, &config)?;
        (
            std::sync::Arc::new(create_project_service(backend.clone())),
            std::sync::Arc::new(crate::bootstrap::create_metadata_field_service(backend)),
        )
    };

    match action {
        ProjectAction::List { limit, after } => {
            let after = match after.as_deref() {
                Some(raw) => Some(
                    Cursor::decode::<ProjectId>(raw)
                        .map_err(|_| anyhow::anyhow!("invalid --after cursor"))?,
                ),
                None => None,
            };
            let filter = ListProjectsFilter {
                limit: *limit,
                after,
            };
            let page = project_ops.list_projects(&filter).await?;
            match cli.output {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&page)?);
                }
                OutputFormat::Text => {
                    for project in &page.items {
                        let desc = project.description().unwrap_or("");
                        println!("#{} {} {}", project.id(), project.name(), desc);
                    }
                    if let Some(c) = &page.next_cursor {
                        println!("... more: --after {c}");
                    }
                }
            }
        }
        ProjectAction::Create { name, description } => {
            let params = CreateProjectParams {
                name: name.clone(),
                description: description.clone(),
            };
            let (project, _events) = project_ops.create_project(&params, None).await?;
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
            project_ops.delete_project(*id, None).await?;
            match cli.output {
                OutputFormat::Json => {
                    println!("{}", serde_json::json!({"deleted": id}));
                }
                OutputFormat::Text => {
                    println!("Deleted project #{}", id);
                }
            }
        }
        ProjectAction::MetadataField { action: mf_action } => {
            let project_id = resolve_project_id(&*project_ops, &config).await?;
            match mf_action {
                MetadataFieldAction::Add {
                    name,
                    field_type,
                    required_on_complete,
                    description,
                } => {
                    validate_field_name(name)?;
                    let ft: MetadataFieldType = field_type.parse()?;
                    let params = CreateMetadataFieldParams {
                        name: name.clone(),
                        field_type: ft,
                        required_on_complete: *required_on_complete,
                        description: description.clone(),
                    };
                    let field = metadata_ops
                        .create_metadata_field(project_id, &params)
                        .await?;
                    match cli.output {
                        OutputFormat::Json => {
                            println!("{}", serde_json::to_string_pretty(&field)?);
                        }
                        OutputFormat::Text => {
                            println!(
                                "Added metadata field #{}: {} (type: {}, required: {})",
                                field.id(),
                                field.name(),
                                field.field_type(),
                                field.required_on_complete()
                            );
                        }
                    }
                }
                MetadataFieldAction::List { limit, after } => {
                    let after = match after.as_deref() {
                        Some(raw) => Some(
                            Cursor::decode::<i64>(raw)
                                .map_err(|_| anyhow::anyhow!("invalid --after cursor"))?,
                        ),
                        None => None,
                    };
                    let filter = ListMetadataFieldsFilter {
                        limit: *limit,
                        after,
                    };
                    let page = metadata_ops
                        .list_metadata_fields(project_id, &filter)
                        .await?;
                    match cli.output {
                        OutputFormat::Json => {
                            println!("{}", serde_json::to_string_pretty(&page)?);
                        }
                        OutputFormat::Text => {
                            for f in &page.items {
                                let desc = f.description().unwrap_or("");
                                let req = if f.required_on_complete() {
                                    " [required]"
                                } else {
                                    ""
                                };
                                println!(
                                    "#{} {} ({}){} {}",
                                    f.id(),
                                    f.name(),
                                    f.field_type(),
                                    req,
                                    desc
                                );
                            }
                            if let Some(c) = &page.next_cursor {
                                println!("... more: --after {c}");
                            }
                        }
                    }
                }
                MetadataFieldAction::Remove { name } => {
                    metadata_ops
                        .delete_metadata_field_by_name(project_id, name)
                        .await?;
                    match cli.output {
                        OutputFormat::Json => {
                            println!("{}", serde_json::json!({"deleted": name}));
                        }
                        OutputFormat::Text => {
                            println!("Removed metadata field: {}", name);
                        }
                    }
                }
            }
        }
        ProjectAction::Members { action: m_action } => {
            cmd_members(cli, m_action).await?;
        }
    }
    Ok(())
}

pub async fn cmd_user(cli: &Cli, action: &UserAction) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let user_service: std::sync::Arc<dyn UserOperations> = if config.cli.remote.url.is_some() {
        std::sync::Arc::new(create_remote_user_operations(&config, &cli.attr))
    } else {
        let backend = create_backend(&root, &config)?;
        std::sync::Arc::new(create_user_service(backend))
    };

    match action {
        UserAction::List { limit, after } => {
            let after = match after.as_deref() {
                Some(raw) => Some(
                    Cursor::decode::<i64>(raw)
                        .map_err(|_| anyhow::anyhow!("invalid --after cursor"))?,
                ),
                None => None,
            };
            let filter = ListUsersFilter {
                limit: *limit,
                after,
            };
            let page = user_service.list_users(&filter).await?;
            match cli.output {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&page)?);
                }
                OutputFormat::Text => {
                    for user in &page.items {
                        let display = user.display_name().unwrap_or("");
                        println!("#{} {} {}", user.id(), user.username(), display);
                    }
                    if let Some(c) = &page.next_cursor {
                        println!("... more: --after {c}");
                    }
                }
            }
        }
        UserAction::Create {
            username,
            sub,
            display_name,
            email,
        } => {
            let params = CreateUserParams {
                username: username.clone(),
                sub: sub.clone(),
                display_name: display_name.clone(),
                email: email.clone(),
            };
            let (user, _events) = user_service
                .create_user(&params, crate::domain::user::UserCreationSource::Manual)
                .await?;
            match cli.output {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&user)?);
                }
                OutputFormat::Text => {
                    println!("Created user #{}: {}", user.id(), user.username());
                }
            }
        }
        UserAction::Update {
            id,
            username,
            display_name,
        } => {
            let params = UpdateUserParams {
                username: username.clone(),
                display_name: display_name.as_ref().map(|v| Some(v.clone())),
            };
            let (user, _events) = user_service.update_user(*id, &params).await?;
            match cli.output {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&user)?);
                }
                OutputFormat::Text => {
                    let display = user.display_name().unwrap_or("");
                    println!(
                        "Updated user #{}: {} {}",
                        user.id(),
                        user.username(),
                        display
                    );
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
    let (_task_ops, project_ops) = create_task_operations(&root, &config, &cli.attr)?;
    let project_id = resolve_project_id(&*project_ops, &config).await?;

    match action {
        MemberAction::List { limit, after } => {
            let after = match after.as_deref() {
                Some(raw) => Some(
                    Cursor::decode::<i64>(raw)
                        .map_err(|_| anyhow::anyhow!("invalid --after cursor"))?,
                ),
                None => None,
            };
            let filter = ListProjectMembersFilter {
                limit: *limit,
                after,
            };
            let page = project_ops
                .list_project_members(project_id, &filter)
                .await?;
            match cli.output {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&page)?);
                }
                OutputFormat::Text => {
                    for member in &page.items {
                        println!("user #{} — role: {}", member.user_id(), member.role());
                    }
                    if let Some(c) = &page.next_cursor {
                        println!("... more: --after {c}");
                    }
                }
            }
        }
        MemberAction::Add { user_id, role } => {
            let params = AddProjectMemberParams::new(*user_id, role.map(|r| r.into()));
            let (member, _events) = project_ops
                .add_project_member(project_id, &params, None)
                .await?;
            match cli.output {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&member)?);
                }
                OutputFormat::Text => {
                    println!(
                        "Added user #{} to project as {}",
                        member.user_id(),
                        member.role()
                    );
                }
            }
        }
        MemberAction::Remove { user_id } => {
            let _events = project_ops
                .remove_project_member(project_id, *user_id, None)
                .await?;
            match cli.output {
                OutputFormat::Json => {
                    println!("{}", serde_json::json!({"removed_user_id": user_id}));
                }
                OutputFormat::Text => {
                    println!("Removed user #{} from project", user_id);
                }
            }
        }
        MemberAction::SetRole { user_id, role } => {
            let (member, _events) = project_ops
                .update_member_role(project_id, *user_id, (*role).into(), None)
                .await?;
            match cli.output {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&member)?);
                }
                OutputFormat::Text => {
                    println!(
                        "Updated user #{} role to {}",
                        member.user_id(),
                        member.role()
                    );
                }
            }
        }
    }
    Ok(())
}

pub async fn cmd_auth_login(cli: &Cli, device_name: Option<String>) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;

    let api_url = config.cli.remote.url.as_deref().context(
        "cli.remote.url is not configured. Set it in config to point to the senko API server.",
    )?;

    // Fetch OIDC config from server
    let auth_config_url = format!("{}/auth/config", api_url.trim_end_matches('/'));
    let http = reqwest::Client::new();
    let resp = http
        .get(&auth_config_url)
        .send()
        .await
        .context("failed to fetch auth config from server")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("GET /auth/config failed ({status}): {body}");
    }
    let auth_config: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse /auth/config response")?;
    let auth_mode = auth_config
        .get("auth_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("oidc");
    let server_oidc = auth_config
        .get("oidc")
        .and_then(|v| if v.is_null() { None } else { Some(v) })
        .context("OIDC is not configured on the server")?;

    // Build OidcConfig from server response + local CLI settings
    let oidc_config = crate::infra::config::OidcConfig {
        issuer_url: server_oidc
            .get("issuer_url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        client_id: server_oidc
            .get("client_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        scopes: server_oidc
            .get("scopes")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        username_claim: None,
        required_claims: Default::default(),
        callback_ports: server_oidc
            .get("callback_ports")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        session: Default::default(),
        groups_claim: "groups".to_string(),
        master_group: None,
    };

    let result = super::oidc_login::perform_login(
        &oidc_config,
        api_url,
        device_name.as_deref(),
        auth_mode,
        config.cli.browser,
    )
    .await?;

    // Cache auth_mode so regular CLI commands know which token to load
    super::auth_cache::cache_auth_mode(&config.xdg, api_url, auth_mode)?;

    match result {
        super::oidc_login::LoginResult::Oidc {
            key_prefix,
            expires_at,
        } => match cli.output {
            OutputFormat::Json => {
                println!(
                    "{}",
                    serde_json::json!({
                        "auth_mode": "oidc",
                        "key_prefix": key_prefix,
                        "expires_at": expires_at,
                    })
                );
            }
            OutputFormat::Text => {
                eprintln!("Login successful!");
                eprintln!("  API key: {}...", key_prefix);
                if let Some(ref exp) = expires_at {
                    eprintln!("  Expires: {exp}");
                }
                eprintln!("  Saved to OS keychain.");
            }
        },
        super::oidc_login::LoginResult::TrustedHeaders => match cli.output {
            OutputFormat::Json => {
                println!(
                    "{}",
                    serde_json::json!({
                        "auth_mode": "trusted_headers",
                    })
                );
            }
            OutputFormat::Text => {
                eprintln!("Login successful!");
                eprintln!("  Access token saved to OS keychain.");
            }
        },
    }
    Ok(())
}

// --- Auth subcommand helpers ---

fn require_api_url_and_token(cli: &Cli) -> Result<(String, String)> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let api_url = config
        .cli
        .remote
        .url
        .as_deref()
        .context(
            "cli.remote.url is not configured. Set it in config to point to the senko API server.",
        )?
        .to_string();
    // ensure_cli_token (called by load_config) may have already resolved the token
    if let Some(token) = config.cli.remote.token {
        return Ok((api_url, token));
    }
    // Fallback: try API key, then access token
    let token = super::keychain::load(&api_url)
        .or_else(|_| super::keychain::load_access_token(&api_url))
        .context("Not logged in. Run `senko auth login` first.")?;
    Ok((api_url, token))
}

fn api_url_and_optional_token(cli: &Cli) -> Result<(String, Option<String>)> {
    let xdg = crate::infra::xdg::XdgDirs::from_env();
    api_url_and_optional_token_with_xdg(cli, &xdg)
}

fn api_url_and_optional_token_with_xdg(
    cli: &Cli,
    xdg: &crate::infra::xdg::XdgDirs,
) -> Result<(String, Option<String>)> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config_with_xdg(cli, &root, xdg)?;
    let api_url = config
        .cli
        .remote
        .url
        .as_deref()
        .context(
            "cli.remote.url is not configured. Set it in config to point to the senko API server.",
        )?
        .to_string();
    if let Some(token) = config.cli.remote.token {
        return Ok((api_url, Some(token)));
    }
    let token = super::keychain::load(&api_url)
        .or_else(|_| super::keychain::load_access_token(&api_url))
        .ok();
    Ok((api_url, token))
}

fn http_client() -> reqwest::Client {
    reqwest::Client::new()
}

fn api_url_base(api_url: &str) -> &str {
    api_url.trim_end_matches('/')
}

// CLI-local response types for deserializing server responses

#[derive(serde::Deserialize)]
struct CliMeResponse {
    user: CliUserInfo,
    session: Option<CliSessionInfo>,
}

#[derive(serde::Deserialize)]
struct CliUserInfo {
    // Wire-read from /auth/me; keep as String so an older server's response
    // doesn't fail Username validation on the client.
    username: String,
    display_name: Option<String>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct CliSessionInfo {
    id: i64,
    key_prefix: String,
    device_name: Option<String>,
    created_at: String,
    last_used_at: Option<String>,
}

// --- Auth subcommand handlers ---

async fn fetch_auth_mode(api_url: &str) -> Result<String> {
    let url = format!("{}/auth/config", api_url_base(api_url));
    let resp = http_client().get(&url).send().await?;
    if !resp.status().is_success() {
        bail!("GET /auth/config failed ({})", resp.status());
    }
    let config: serde_json::Value = resp.json().await?;
    Ok(config
        .get("auth_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("oidc")
        .to_string())
}

pub async fn cmd_auth_token(cli: &Cli) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let api_url = config.cli.remote.url.as_deref().context(
        "cli.remote.url is not configured. Set it in config to point to the senko API server.",
    )?;

    let token = match fetch_auth_mode(api_url).await {
        Ok(auth_mode) => match auth_mode.as_str() {
            "trusted_headers" => super::keychain::load_access_token(api_url)
                .context("Not logged in. Run `senko auth login` first.")?,
            _ => super::keychain::load(api_url)
                .context("Not logged in. Run `senko auth login` first.")?,
        },
        Err(_) => {
            // Server unreachable: try API key first, then access token
            super::keychain::load(api_url)
                .or_else(|_| super::keychain::load_access_token(api_url))
                .context("Not logged in. Run `senko auth login` first.")?
        }
    };

    match cli.output {
        OutputFormat::Json => println!("{}", serde_json::json!({"token": token})),
        OutputFormat::Text => print!("{token}"),
    }
    Ok(())
}

pub async fn cmd_auth_status(cli: &Cli) -> Result<()> {
    let (api_url, token) = api_url_and_optional_token(cli)?;
    let client = http_client();
    let mut req = client.get(format!("{}/auth/me", api_url_base(&api_url)));
    if let Some(ref token) = token {
        req = req.bearer_auth(token);
    }
    let resp = req.send().await.context("failed to connect to server")?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        bail!("Not logged in. Run `senko auth login` to authenticate.");
    }
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("GET /auth/me failed ({status}): {body}");
    }
    let me: CliMeResponse = resp
        .json()
        .await
        .context("failed to parse /auth/me response")?;
    match cli.output {
        OutputFormat::Json => {
            let mut obj = serde_json::json!({
                "logged_in": true,
                "username": me.user.username,
                "display_name": me.user.display_name,
                "api_url": api_url,
            });
            if let Some(ref session) = me.session {
                obj["session_id"] = serde_json::json!(session.id);
                obj["key_prefix"] = serde_json::json!(session.key_prefix);
            }
            println!("{obj}");
        }
        OutputFormat::Text => {
            eprintln!("Logged in as: {}", me.user.username);
            if let Some(ref dn) = me.user.display_name {
                eprintln!("  Display name: {dn}");
            }
            if let Some(ref session) = me.session {
                eprintln!("  Session ID: {}", session.id);
                eprintln!("  Key prefix: {}...", session.key_prefix);
            }
            eprintln!("  API URL: {api_url}");
        }
    }
    Ok(())
}

pub async fn cmd_auth_logout(cli: &Cli) -> Result<()> {
    let (api_url, token) = require_api_url_and_token(cli)?;
    let xdg = crate::infra::xdg::XdgDirs::from_env();
    let client = http_client();
    let base = api_url_base(&api_url);

    // Try to get current session ID and revoke on server
    let mut server_revoked = false;
    let me_resp = client
        .get(format!("{base}/auth/me"))
        .bearer_auth(&token)
        .send()
        .await;
    if let Ok(resp) = me_resp
        && resp.status().is_success()
        && let Ok(me) = resp.json::<CliMeResponse>().await
        && let Some(session) = me.session
    {
        let revoke_resp = client
            .delete(format!("{base}/auth/sessions/{}", session.id))
            .bearer_auth(&token)
            .send()
            .await;
        server_revoked = revoke_resp
            .map(|r| r.status().is_success())
            .unwrap_or(false);
    }

    // Always delete from keychain
    super::keychain::delete(&api_url)?;
    // Also delete access token if present (ignore error if not found)
    let _ = super::keychain::delete_access_token(&api_url);
    // Delete cached auth_mode
    let _ = super::auth_cache::delete_cached_auth_mode(&xdg, &api_url);

    match cli.output {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::json!({
                    "status": "logged_out",
                    "server_revoked": server_revoked,
                })
            );
        }
        OutputFormat::Text => {
            if server_revoked {
                eprintln!("Logged out (session revoked on server).");
            } else {
                eprintln!("Logged out (local token removed; server revocation may have failed).");
            }
        }
    }
    Ok(())
}

pub async fn cmd_auth_sessions(cli: &Cli, limit: Option<u32>, after: Option<&str>) -> Result<()> {
    let (api_url, token) = require_api_url_and_token(cli)?;
    let client = http_client();

    let mut url = format!("{}/auth/sessions", api_url_base(&api_url));
    let mut params: Vec<String> = Vec::new();
    if let Some(l) = limit {
        params.push(format!("limit={l}"));
    }
    if let Some(raw_after) = after {
        // Validate cursor client-side so a bad input fails fast (the API will
        // also reject, but CLI users get a nicer error).
        Cursor::decode::<i64>(raw_after).map_err(|_| anyhow::anyhow!("invalid --after cursor"))?;
        params.push(format!(
            "after={}",
            percent_encoding::utf8_percent_encode(raw_after, percent_encoding::NON_ALPHANUMERIC,)
        ));
    }
    if !params.is_empty() {
        url = format!("{url}?{}", params.join("&"));
    }

    let resp = client
        .get(url)
        .bearer_auth(&token)
        .send()
        .await
        .context("failed to connect to server")?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        bail!("Session expired or invalid. Run `senko auth login` to re-authenticate.");
    }
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("GET /auth/sessions failed ({status}): {body}");
    }
    #[derive(serde::Deserialize, serde::Serialize)]
    struct SessionsPage {
        items: Vec<CliSessionInfo>,
        next_cursor: Option<String>,
    }
    let page: SessionsPage = resp
        .json()
        .await
        .context("failed to parse /auth/sessions response")?;
    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string(&page).unwrap_or_default());
        }
        OutputFormat::Text => {
            if page.items.is_empty() {
                eprintln!("No active sessions.");
            } else {
                eprintln!(
                    "{:<6} {:<14} {:<16} {:<22} LAST USED",
                    "ID", "KEY PREFIX", "DEVICE", "CREATED"
                );
                for s in &page.items {
                    eprintln!(
                        "{:<6} {:<14} {:<16} {:<22} {}",
                        s.id,
                        format!("{}...", s.key_prefix),
                        s.device_name.as_deref().unwrap_or("-"),
                        &s.created_at,
                        s.last_used_at.as_deref().unwrap_or("-"),
                    );
                }
            }
            if let Some(c) = &page.next_cursor {
                eprintln!("... more: --after {c}");
            }
        }
    }
    Ok(())
}

pub async fn cmd_auth_revoke(cli: &Cli, id: Option<i64>, all: bool) -> Result<()> {
    let (api_url, token) = require_api_url_and_token(cli)?;
    let client = http_client();
    let base = api_url_base(&api_url);

    if all {
        let resp = client
            .delete(format!("{base}/auth/sessions"))
            .bearer_auth(&token)
            .send()
            .await
            .context("failed to connect to server")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("DELETE /auth/sessions failed ({status}): {body}");
        }
        match cli.output {
            OutputFormat::Json => println!("{}", serde_json::json!({"revoked": "all"})),
            OutputFormat::Text => {
                eprintln!("All sessions revoked.");
                eprintln!(
                    "Note: your current session has also been revoked. Run `senko auth login` to re-authenticate."
                );
            }
        }
    } else if let Some(session_id) = id {
        let resp = client
            .delete(format!("{base}/auth/sessions/{session_id}"))
            .bearer_auth(&token)
            .send()
            .await
            .context("failed to connect to server")?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("Session {session_id} not found.");
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("DELETE /auth/sessions/{session_id} failed ({status}): {body}");
        }
        match cli.output {
            OutputFormat::Json => println!("{}", serde_json::json!({"revoked": session_id})),
            OutputFormat::Text => eprintln!("Session {session_id} revoked."),
        }
    } else {
        bail!("Provide a session ID or use --all to revoke all sessions.");
    }
    Ok(())
}

// --- Contract commands ---

fn build_contract_ops(
    config: &Config,
    root: &Path,
    cli_attrs: &[(String, String)],
) -> Result<Arc<dyn ContractOperations>> {
    if config.cli.remote.url.is_some() {
        Ok(Arc::new(create_remote_contract_operations(
            config, root, cli_attrs,
        )))
    } else {
        let backend = create_backend(root, config)?;
        Ok(Arc::new(create_contract_service(backend, config, root)))
    }
}

fn print_contract_text(c: &crate::domain::contract::Contract) {
    println!("Contract #{}", c.id());
    println!("  title: {}", c.title());
    if let Some(desc) = c.description() {
        println!("  description: {desc}");
    }
    if !c.tags().is_empty() {
        println!("  tags: {}", c.tags().join(", "));
    }
    if let Some(meta) = c.metadata()
        && let Ok(s) = serde_json::to_string(meta)
    {
        println!("  metadata: {s}");
    }
    if !c.definition_of_done().is_empty() {
        println!("  definition_of_done:");
        print_dod_items_contract(c.definition_of_done());
    }
    if !c.notes().is_empty() {
        println!("  notes: {}", c.notes().len());
    }
    println!("  is_completed: {}", c.is_completed());
}

fn print_dod_items_contract(items: &[crate::domain::task::DodItem]) {
    for (i, item) in items.iter().enumerate() {
        let mark = if item.checked() { "x" } else { " " };
        println!("    {}. [{mark}] {}", i + 1, item.content());
    }
}

pub async fn cmd_contract(cli: &Cli, action: &ContractAction) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config(cli, &root)?;
    let contract_ops = build_contract_ops(&config, &root, &cli.attr)?;
    let (_task_ops_unused, project_ops) = build_project_ops_pair(&config, &root, &cli.attr)?;
    let project_id = resolve_project_id(&*project_ops, &config).await?;

    match action {
        ContractAction::Add {
            title,
            description,
            definition_of_done,
            tag,
            metadata,
            from_json,
            from_json_file,
        } => {
            let params = if *from_json {
                let mut buf = String::new();
                std::io::stdin()
                    .read_to_string(&mut buf)
                    .context("failed to read from stdin")?;
                serde_json::from_str::<CreateContractParams>(&buf)
                    .context("invalid JSON from stdin")?
            } else if let Some(path) = from_json_file {
                let content = fs::read_to_string(path)
                    .with_context(|| format!("failed to read file: {}", path.display()))?;
                serde_json::from_str::<CreateContractParams>(&content)
                    .context("invalid JSON in file")?
            } else {
                let Some(title) = title.clone() else {
                    bail!("--title is required when not using --from-json or --from-json-file");
                };
                let metadata_val = match metadata {
                    Some(m) => Some(
                        serde_json::from_str::<serde_json::Value>(m)
                            .context("invalid JSON for --metadata")?,
                    ),
                    None => None,
                };
                CreateContractParams {
                    title,
                    description: description.clone(),
                    definition_of_done: definition_of_done.clone(),
                    tags: tag.clone(),
                    metadata: metadata_val,
                }
            };

            if cli.dry_run {
                let operations = vec![format!("Create contract with title \"{}\"", params.title)];
                return print_dry_run(
                    &cli.output,
                    &DryRunOperation {
                        command: "contract add".into(),
                        operations,
                    },
                );
            }

            let contract = contract_ops.create_contract(project_id, &params).await?;
            let response = ContractResponse::from(contract.clone());
            match cli.output {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&response)?),
                OutputFormat::Text => {
                    println!("Created contract #{}", contract.id());
                    print_contract_text(&contract);
                }
            }
        }
        ContractAction::List { tag, limit, after } => {
            let after = match after.as_deref() {
                Some(raw) => Some(
                    Cursor::decode::<ContractId>(raw)
                        .map_err(|_| anyhow::anyhow!("invalid --after cursor"))?,
                ),
                None => None,
            };
            let filter = ListContractsFilter {
                tags: tag.clone(),
                limit: *limit,
                after,
            };
            let page = contract_ops.list_contracts(project_id, &filter).await?;
            match cli.output {
                OutputFormat::Json => {
                    let items: Vec<ContractResponse> =
                        page.items.into_iter().map(ContractResponse::from).collect();
                    let wrapped = serde_json::json!({
                        "items": items,
                        "next_cursor": page.next_cursor,
                    });
                    println!("{}", serde_json::to_string_pretty(&wrapped)?);
                }
                OutputFormat::Text => {
                    if page.items.is_empty() {
                        println!("No contracts.");
                    } else {
                        for c in &page.items {
                            let tags = if c.tags().is_empty() {
                                String::new()
                            } else {
                                format!(" [{}]", c.tags().join(", "))
                            };
                            let status = if c.is_completed() { "done" } else { "open" };
                            println!("#{} ({status}) {}{}", c.id(), c.title(), tags);
                        }
                    }
                    if let Some(c) = &page.next_cursor {
                        println!("... more: --after {c}");
                    }
                }
            }
        }
        ContractAction::Get { id } => {
            let contract = contract_ops.get_contract(project_id, *id).await?;
            match cli.output {
                OutputFormat::Json => {
                    let response = ContractResponse::from(contract);
                    println!("{}", serde_json::to_string_pretty(&response)?);
                }
                OutputFormat::Text => print_contract_text(&contract),
            }
        }
        ContractAction::Edit {
            id,
            title,
            description,
            clear_description,
            metadata,
            replace_metadata,
            clear_metadata,
            set_tags,
            set_definition_of_done,
            add_tag,
            add_definition_of_done,
            remove_tag,
            remove_definition_of_done,
        } => {
            let id = *id;
            if cli.dry_run {
                let mut operations = Vec::new();
                if let Some(t) = title {
                    operations.push(format!("Update contract #{}: set title to \"{}\"", id, t));
                }
                if *clear_description {
                    operations.push(format!("Update contract #{}: clear description", id));
                } else if let Some(d) = description {
                    operations.push(format!(
                        "Update contract #{}: set description to \"{}\"",
                        id, d
                    ));
                }
                if *clear_metadata {
                    operations.push(format!("Update contract #{}: clear metadata", id));
                } else if let Some(m) = replace_metadata {
                    operations.push(format!(
                        "Update contract #{}: replace metadata with {}",
                        id, m
                    ));
                } else if let Some(m) = metadata {
                    operations.push(format!(
                        "Update contract #{}: merge metadata with {}",
                        id, m
                    ));
                }
                if let Some(tags) = set_tags {
                    operations.push(format!(
                        "Update contract #{}: set tags to [{}]",
                        id,
                        tags.join(", ")
                    ));
                }
                if !add_tag.is_empty() {
                    operations.push(format!(
                        "Update contract #{}: add tags [{}]",
                        id,
                        add_tag.join(", ")
                    ));
                }
                if !remove_tag.is_empty() {
                    operations.push(format!(
                        "Update contract #{}: remove tags [{}]",
                        id,
                        remove_tag.join(", ")
                    ));
                }
                if let Some(dod) = set_definition_of_done {
                    operations.push(format!(
                        "Update contract #{}: set DoD to [{}]",
                        id,
                        dod.join(", ")
                    ));
                }
                if !add_definition_of_done.is_empty() {
                    operations.push(format!(
                        "Update contract #{}: add DoD [{}]",
                        id,
                        add_definition_of_done.join(", ")
                    ));
                }
                if !remove_definition_of_done.is_empty() {
                    operations.push(format!(
                        "Update contract #{}: remove DoD [{}]",
                        id,
                        remove_definition_of_done.join(", ")
                    ));
                }
                if operations.is_empty() {
                    operations.push(format!("Update contract #{}: no changes", id));
                }
                return print_dry_run(
                    &cli.output,
                    &DryRunOperation {
                        command: "contract edit".into(),
                        operations,
                    },
                );
            }

            let scalar = UpdateContractParams {
                title: title.clone(),
                description: if *clear_description {
                    Some(None)
                } else {
                    description.clone().map(Some)
                },
                metadata: if *clear_metadata {
                    Some(MetadataUpdate::Clear)
                } else if let Some(m) = replace_metadata {
                    let val: serde_json::Value =
                        serde_json::from_str(m).context("invalid JSON for --replace-metadata")?;
                    Some(MetadataUpdate::Replace(val))
                } else {
                    match metadata {
                        Some(m) => {
                            let val: serde_json::Value =
                                serde_json::from_str(m).context("invalid JSON for --metadata")?;
                            Some(MetadataUpdate::Merge(val))
                        }
                        None => None,
                    }
                },
            };

            let array = UpdateContractArrayParams {
                set_tags: set_tags.clone(),
                add_tags: add_tag.clone(),
                remove_tags: remove_tag.clone(),
                set_definition_of_done: set_definition_of_done.clone(),
                add_definition_of_done: add_definition_of_done.clone(),
                remove_definition_of_done: remove_definition_of_done.clone(),
            };

            let contract = contract_ops
                .edit_contract(project_id, id, &scalar, &array)
                .await?;
            match cli.output {
                OutputFormat::Json => {
                    let response = ContractResponse::from(contract);
                    println!("{}", serde_json::to_string_pretty(&response)?);
                }
                OutputFormat::Text => {
                    println!("Updated contract #{}", contract.id());
                    print_contract_text(&contract);
                }
            }
        }
        ContractAction::Delete { id } => {
            let id = *id;
            if cli.dry_run {
                return print_dry_run(
                    &cli.output,
                    &DryRunOperation {
                        command: "contract delete".into(),
                        operations: vec![format!("Delete contract #{}", id)],
                    },
                );
            }
            contract_ops.delete_contract(project_id, id).await?;
            match cli.output {
                OutputFormat::Json => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "deleted": true,
                            "id": id,
                        }))?
                    );
                }
                OutputFormat::Text => println!("Deleted contract #{}", id),
            }
        }
        ContractAction::Dod { command } => match command {
            ContractDodCommand::Check { contract_id, index } => {
                let (cid, idx) = (*contract_id, *index);
                if cli.dry_run {
                    return print_dry_run(
                        &cli.output,
                        &DryRunOperation {
                            command: "contract dod check".into(),
                            operations: vec![format!("Check DoD item #{idx} of contract #{cid}")],
                        },
                    );
                }
                let contract = contract_ops.check_dod(project_id, cid, idx).await?;
                match cli.output {
                    OutputFormat::Json => {
                        let response = ContractResponse::from(contract);
                        println!("{}", serde_json::to_string_pretty(&response)?);
                    }
                    OutputFormat::Text => {
                        println!("Checked DoD item #{idx} of contract #{cid}");
                        print_dod_items_contract(contract.definition_of_done());
                    }
                }
            }
            ContractDodCommand::Uncheck { contract_id, index } => {
                let (cid, idx) = (*contract_id, *index);
                if cli.dry_run {
                    return print_dry_run(
                        &cli.output,
                        &DryRunOperation {
                            command: "contract dod uncheck".into(),
                            operations: vec![format!("Uncheck DoD item #{idx} of contract #{cid}")],
                        },
                    );
                }
                let contract = contract_ops.uncheck_dod(project_id, cid, idx).await?;
                match cli.output {
                    OutputFormat::Json => {
                        let response = ContractResponse::from(contract);
                        println!("{}", serde_json::to_string_pretty(&response)?);
                    }
                    OutputFormat::Text => {
                        println!("Unchecked DoD item #{idx} of contract #{cid}");
                        print_dod_items_contract(contract.definition_of_done());
                    }
                }
            }
        },
        ContractAction::Note { command } => match command {
            ContractNoteCommand::Add {
                contract_id,
                content,
                source_task,
            } => {
                let cid = *contract_id;
                if cli.dry_run {
                    return print_dry_run(
                        &cli.output,
                        &DryRunOperation {
                            command: "contract note add".into(),
                            operations: vec![format!(
                                "Add note to contract #{cid}: \"{}\"",
                                content
                            )],
                        },
                    );
                }
                let note = contract_ops
                    .add_note(project_id, cid, content.clone(), *source_task)
                    .await?;
                let response = ContractNoteResponse::from(&note);
                match cli.output {
                    OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&response)?),
                    OutputFormat::Text => {
                        println!("Added note to contract #{cid}");
                        println!("  content: {}", note.content());
                        if let Some(src) = note.source_task_id() {
                            println!("  source_task_id: {src}");
                        }
                        println!("  created_at: {}", note.created_at());
                    }
                }
            }
            ContractNoteCommand::List {
                contract_id,
                limit,
                after,
            } => {
                let after = match after.as_deref() {
                    Some(raw) => Some(
                        Cursor::decode::<i64>(raw)
                            .map_err(|_| anyhow::anyhow!("invalid --after cursor"))?,
                    ),
                    None => None,
                };
                let filter = ListContractNotesFilter {
                    limit: *limit,
                    after,
                };
                let page = contract_ops
                    .list_notes(project_id, *contract_id, &filter)
                    .await?;
                match cli.output {
                    OutputFormat::Json => {
                        let items: Vec<ContractNoteResponse> =
                            page.items.iter().map(ContractNoteResponse::from).collect();
                        let wrapped = serde_json::json!({
                            "items": items,
                            "next_cursor": page.next_cursor,
                        });
                        println!("{}", serde_json::to_string_pretty(&wrapped)?);
                    }
                    OutputFormat::Text => {
                        if page.items.is_empty() {
                            println!("No notes.");
                        } else {
                            for n in &page.items {
                                let src = match n.source_task_id() {
                                    Some(id) => format!(" (source: task #{id})"),
                                    None => String::new(),
                                };
                                println!("[{}]{src} {}", n.created_at(), n.content());
                            }
                        }
                        if let Some(c) = &page.next_cursor {
                            println!("... more: --after {c}");
                        }
                    }
                }
            }
        },
    }

    Ok(())
}

fn build_project_ops_pair(
    config: &Config,
    root: &Path,
    cli_attrs: &[(String, String)],
) -> Result<(
    Arc<dyn crate::application::TaskOperations>,
    Arc<dyn ProjectOperations>,
)> {
    // Reuse existing task_operations bootstrap to satisfy resolve_project_id with the same
    // local/remote switching semantics.
    create_task_operations(root, config, cli_attrs)
}

#[cfg(test)]
mod tests {
    use super::super::{Command, OutputFormat, TaskAction};
    use super::*;
    use crate::domain::TaskRepository;

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
            attr: vec![],
            command: Command::Task {
                action: TaskAction::Add {
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
                    assignee_user_id: None,
                },
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
            None,
        )
        .await
        .unwrap();

        let backend = crate::infra::sqlite::SqliteBackend::new(
            tmp.path(),
            Some(&tmp.path().join("data.db")),
            None,
            &crate::infra::xdg::XdgDirs::default(),
        )
        .unwrap();
        let task = backend
            .get_task(DEFAULT_PROJECT_ID, crate::domain::task::TaskId(1))
            .await
            .unwrap();
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
            attr: vec![],
            command: Command::Task {
                action: TaskAction::Add {
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
                    assignee_user_id: None,
                },
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
            None,
        )
        .await
        .unwrap();

        let backend = crate::infra::sqlite::SqliteBackend::new(
            tmp.path(),
            Some(&tmp.path().join("data.db")),
            None,
            &crate::infra::xdg::XdgDirs::default(),
        )
        .unwrap();
        let task = backend
            .get_task(DEFAULT_PROJECT_ID, crate::domain::task::TaskId(1))
            .await
            .unwrap();
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
            attr: vec![],
            command: Command::Task {
                action: TaskAction::Add {
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
                    assignee_user_id: None,
                },
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
            None,
        )
        .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("--title is required")
        );
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
            attr: vec![],
            command: Command::Task {
                action: TaskAction::Add {
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
                    assignee_user_id: None,
                },
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
            None,
        )
        .await
        .unwrap();
        let backend = crate::infra::sqlite::SqliteBackend::new(
            tmp.path(),
            Some(&tmp.path().join("data.db")),
            None,
            &crate::infra::xdg::XdgDirs::default(),
        )
        .unwrap();
        let task = backend
            .get_task(DEFAULT_PROJECT_ID, crate::domain::task::TaskId(1))
            .await
            .unwrap();
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
            attr: vec![],
            command: Command::Task {
                action: TaskAction::Add {
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
                    assignee_user_id: None,
                },
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
            None,
        )
        .await
        .unwrap();
        let backend = crate::infra::sqlite::SqliteBackend::new(
            tmp.path(),
            Some(&tmp.path().join("data.db")),
            None,
            &crate::infra::xdg::XdgDirs::default(),
        )
        .unwrap();
        let task = backend
            .get_task(DEFAULT_PROJECT_ID, crate::domain::task::TaskId(1))
            .await
            .unwrap();
        assert_eq!(task.title(), "json out");
    }

    // --- Doctor tests ---

    #[test]
    fn expand_tilde_with_home() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(
            super::expand_tilde("~/foo/bar.sh"),
            format!("{home}/foo/bar.sh")
        );
    }

    #[test]
    fn expand_tilde_no_tilde() {
        assert_eq!(
            super::expand_tilde("/usr/bin/script.sh"),
            "/usr/bin/script.sh"
        );
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

    fn make_hook_def(command: &str, required_env: Vec<&str>) -> crate::infra::config::HookDef {
        crate::infra::config::HookDef {
            command: command.to_string(),
            when: crate::infra::config::HookWhen::Post,
            mode: crate::infra::config::HookMode::Async,
            on_failure: crate::infra::config::OnFailure::Abort,
            enabled: true,
            env_vars: required_env
                .into_iter()
                .map(|name| crate::infra::config::EnvVarSpec {
                    name: name.to_string(),
                    required: true,
                    default: None,
                    description: None,
                })
                .collect(),
            on_result: None,
            prompt: None,
            timeout_secs: 30,
        }
    }

    #[test]
    fn run_hook_checks_env_missing() {
        let def = make_hook_def("echo test", vec!["SENKO_DOCTOR_TEST_NONEXISTENT_VAR_12345"]);
        let checks = super::run_hook_checks(&def);
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].check, "env_var");
        assert_eq!(checks[0].status, super::CheckStatus::Error);
    }

    #[test]
    fn run_hook_checks_env_set() {
        unsafe {
            std::env::set_var("SENKO_DOCTOR_TEST_VAR_OK", "1");
        }
        let def = make_hook_def("echo test", vec!["SENKO_DOCTOR_TEST_VAR_OK"]);
        let checks = super::run_hook_checks(&def);
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].status, super::CheckStatus::Ok);
        unsafe {
            std::env::remove_var("SENKO_DOCTOR_TEST_VAR_OK");
        }
    }

    #[test]
    fn run_hook_checks_script_not_found() {
        let def = make_hook_def("/nonexistent/path/hook.sh", vec![]);
        let checks = super::run_hook_checks(&def);
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
        let def = make_hook_def(script.to_str().unwrap(), vec![]);
        let checks = super::run_hook_checks(&def);
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
        let def = make_hook_def(script.to_str().unwrap(), vec![]);
        let checks = super::run_hook_checks(&def);
        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].check, "script_exists");
        assert_eq!(checks[0].status, super::CheckStatus::Ok);
        assert_eq!(checks[1].check, "script_executable");
        assert_eq!(checks[1].status, super::CheckStatus::Error);
    }

    #[test]
    fn run_hook_checks_bare_command_no_file_checks() {
        let def = make_hook_def("echo hello world", vec![]);
        let checks = super::run_hook_checks(&def);
        assert!(checks.is_empty());
    }

    #[test]
    fn cli_me_response_deserializes_with_null_session() {
        let json = r#"{"user":{"username":"alice","display_name":"Alice"},"session":null}"#;
        let me: super::CliMeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(me.user.username, "alice");
        assert!(me.session.is_none());
    }

    #[test]
    fn cli_me_response_deserializes_with_session() {
        let json = r#"{"user":{"username":"alice","display_name":null},"session":{"id":1,"key_prefix":"abc","device_name":null,"created_at":"2026-01-01T00:00:00Z","last_used_at":null}}"#;
        let me: super::CliMeResponse = serde_json::from_str(json).unwrap();
        assert!(me.session.is_some());
        let session = me.session.unwrap();
        assert_eq!(session.id, 1);
        assert_eq!(session.key_prefix, "abc");
    }

    /// Clear SENKO-side env vars that `load_config` still reads via `env_config_path`.
    /// XDG isolation is handled via the injected `XdgDirs` below.
    fn clear_senko_env() {
        unsafe {
            std::env::remove_var("SENKO_CONFIG");
            std::env::remove_var("SENKO_USER");
            std::env::remove_var("SENKO_PROJECT");
            std::env::remove_var("SENKO_CLI_REMOTE_URL");
            std::env::remove_var("SENKO_CLI_REMOTE_TOKEN");
        }
    }

    /// `XdgDirs` pointing at a non-existent directory so no user-level config
    /// is ever loaded. Must be used together with `clear_senko_env`.
    fn isolated_xdg(project_root: &std::path::Path) -> crate::infra::xdg::XdgDirs {
        crate::infra::xdg::XdgDirs {
            config_home: Some(project_root.join("__no_user_config__")),
            ..Default::default()
        }
    }

    #[test]
    #[serial_test::serial]
    fn api_url_and_optional_token_returns_none_when_no_token() {
        let tmp = tempfile::tempdir().unwrap();
        clear_senko_env();
        let senko_dir = tmp.path().join(".senko");
        std::fs::create_dir_all(&senko_dir).unwrap();
        std::fs::write(
            senko_dir.join("config.toml"),
            r#"
[cli.remote]
url = "http://localhost:3142"
"#,
        )
        .unwrap();

        let cli = Cli {
            output: OutputFormat::Text,
            project_root: Some(tmp.path().to_path_buf()),
            config: None,
            dry_run: false,
            log_dir: None,
            db_path: None,
            postgres_url: None,
            project: None,
            user: None,
            attr: vec![],
            command: Command::Auth {
                command: super::super::AuthCommand::Status,
            },
        };
        let (url, token) =
            super::api_url_and_optional_token_with_xdg(&cli, &isolated_xdg(tmp.path())).unwrap();
        assert_eq!(url, "http://localhost:3142");
        assert!(token.is_none(), "expected no token for relay-like config");
    }

    #[test]
    #[serial_test::serial]
    fn api_url_and_optional_token_returns_token_from_config() {
        let tmp = tempfile::tempdir().unwrap();
        clear_senko_env();
        let senko_dir = tmp.path().join(".senko");
        std::fs::create_dir_all(&senko_dir).unwrap();
        std::fs::write(
            senko_dir.join("config.toml"),
            r#"
[cli.remote]
url = "http://localhost:3142"
token = "my-api-key"
"#,
        )
        .unwrap();

        let cli = Cli {
            output: OutputFormat::Text,
            project_root: Some(tmp.path().to_path_buf()),
            config: None,
            dry_run: false,
            log_dir: None,
            db_path: None,
            postgres_url: None,
            project: None,
            user: None,
            attr: vec![],
            command: Command::Auth {
                command: super::super::AuthCommand::Status,
            },
        };
        let (url, token) =
            super::api_url_and_optional_token_with_xdg(&cli, &isolated_xdg(tmp.path())).unwrap();
        assert_eq!(url, "http://localhost:3142");
        assert_eq!(token.as_deref(), Some("my-api-key"));
    }
}
