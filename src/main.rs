use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use senko::application::port::HookExecutor;
use senko::application::{ProjectService, TaskService, UserService};
use senko::application::HookTrigger;
use senko::infra::config::{CliOverrides, Config, HookEntry, HookMode};
use senko::domain::project::CreateProjectParams;
use senko::domain::repository::TaskBackend;
use senko::domain::task::{
    CreateTaskParams, ListTasksFilter, Priority, Task, TaskEvent, TaskStatus,
    UpdateTaskArrayParams, UpdateTaskParams,
};
use senko::domain::user::{AddProjectMemberParams, CreateUserParams, Role};
use senko::infra::hook as hooks;
use senko::infra::hook::{RuntimeMode, BackendInfo};
use senko::infra::http::HttpBackend;
use senko::infra::hook::executor::ShellHookExecutor;
use senko::infra::pr_verifier::GhCliPrVerifier;
use senko::infra::project_root::resolve_project_root;
use senko::infra::sqlite as db;

use senko::domain::DEFAULT_PROJECT_ID;

/// Create the appropriate backend based on config (env + CLI already applied).
/// Returns (backend, is_http) where is_http indicates HTTP mode for hook control.
fn create_backend(
    project_root: &std::path::Path,
    config: &Config,
) -> Result<(Arc<dyn TaskBackend>, bool)> {
    // 1. HTTP backend (api_url from env or config.toml)
    if let Some(ref url) = config.backend.api_url {
        let backend = match config.backend.api_key.as_ref() {
            Some(key) => HttpBackend::with_api_key(url, key.clone()),
            None => HttpBackend::new(url),
        };
        return Ok((Arc::new(backend), true));
    }

    // 2. DynamoDB backend
    #[cfg(feature = "dynamodb")]
    {
        use senko::infra::dynamodb::DynamoDbBackend;

        if let Some(ref ddb_config) = config.backend.dynamodb {
            if let Some(ref table_name) = ddb_config.table_name {
                return Ok((
                    Arc::new(DynamoDbBackend::new(
                        table_name.clone(),
                        ddb_config.region.clone(),
                    )),
                    false,
                ));
            }
        }
    }

    // 3. PostgreSQL backend
    #[cfg(feature = "postgres")]
    {
        use senko::infra::postgres::PostgresBackend;

        if let Some(ref pg_config) = config.backend.postgres {
            if let Some(ref database_url) = pg_config.url {
                return Ok((Arc::new(PostgresBackend::new(database_url.clone())), false));
            }
        }
    }

    // 4. Default: SqliteBackend
    let sqlite = db::SqliteBackend::new(project_root, None, config.storage.db_path.as_deref())?;
    sqlite.sync_config_defaults(config)?;
    Ok((Arc::new(sqlite), false))
}

fn build_cli_overrides(cli: &Cli) -> CliOverrides {
    CliOverrides {
        log_dir: cli.log_dir.as_ref().map(|p| p.to_string_lossy().into_owned()),
        db_path: cli.db_path.as_ref().map(|p| p.to_string_lossy().into_owned()),
        postgres_url: cli.postgres_url.clone(),
        project: cli.project.clone(),
        ..Default::default()
    }
}

fn load_config_with_cli(root: &std::path::Path, cli: &Cli) -> Result<Config> {
    let mut config = hooks::load_config(root, cli.config.as_deref())?;
    config.apply_cli(&build_cli_overrides(cli));
    Ok(config)
}

fn should_fire_client_hooks(config: &Config, using_http: bool) -> bool {
    match config.backend.hook_mode {
        HookMode::Server => !using_http,
        HookMode::Client | HookMode::Both => true,
    }
}

fn resolve_backend_info(config: &Config, project_root: &std::path::Path) -> BackendInfo {
    if let Some(ref url) = config.backend.api_url {
        return BackendInfo::Http { api_url: url.clone() };
    }
    #[cfg(feature = "dynamodb")]
    if config.backend.dynamodb.as_ref().and_then(|d| d.table_name.as_ref()).is_some() {
        return BackendInfo::Dynamodb;
    }
    #[cfg(feature = "postgres")]
    if config.backend.postgres.as_ref().and_then(|p| p.url.as_ref()).is_some() {
        return BackendInfo::Postgresql;
    }
    let db_path = senko::infra::sqlite::resolve_db_path_preview(project_root, config.storage.db_path.as_deref())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());
    BackendInfo::Sqlite { db_file_path: db_path }
}

fn create_hook_executor(config: Config, using_http: bool, runtime_mode: RuntimeMode, backend_info: BackendInfo) -> Arc<dyn HookExecutor> {
    let should_fire = should_fire_client_hooks(&config, using_http);
    Arc::new(ShellHookExecutor::new(config, should_fire, runtime_mode, backend_info))
}

fn create_task_service(
    backend: Arc<dyn TaskBackend>,
    config: &Config,
    using_http: bool,
    project_root: &std::path::Path,
) -> TaskService {
    let backend_info = resolve_backend_info(config, project_root);
    let hooks = create_hook_executor(config.clone(), using_http, RuntimeMode::Cli, backend_info);
    let pr_verifier = Arc::new(GhCliPrVerifier);
    TaskService::new(backend, hooks, pr_verifier, config.workflow.clone())
}

fn create_project_service(backend: Arc<dyn TaskBackend>) -> ProjectService {
    ProjectService::new(backend)
}

fn create_user_service(backend: Arc<dyn TaskBackend>) -> UserService {
    UserService::new(backend)
}

/// Resolve the project ID from CLI flag, config, or default.
///
/// Priority: CLI flag / SENKO_PROJECT env > config.toml [project] name > DEFAULT_PROJECT_ID
async fn resolve_project_id(
    backend: &dyn TaskBackend,
    config: &Config,
) -> Result<i64> {
    match config.project.name.as_deref() {
        Some(n) => {
            let project = backend
                .get_project_by_name(n)
                .await
                .with_context(|| format!("project not found: {n}"))?;
            Ok(project.id())
        }
        None => Ok(DEFAULT_PROJECT_ID),
    }
}

#[derive(Debug, Clone, ValueEnum)]
enum OutputFormat {
    Json,
    Text,
}

#[derive(Debug, Parser)]
#[command(name = "senko", about = "Local task management CLI", version)]
struct Cli {
    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    output: OutputFormat,

    /// Project root directory
    #[arg(long)]
    project_root: Option<PathBuf>,

    /// Path to config file (default: .senko/config.toml)
    #[arg(long)]
    config: Option<PathBuf>,

    /// Dry run mode: show what would be done without executing
    #[arg(long)]
    dry_run: bool,

    /// Override log output directory
    #[arg(long)]
    log_dir: Option<PathBuf>,

    /// Path to SQLite database file (env: SENKO_DB_PATH)
    #[arg(long)]
    db_path: Option<PathBuf>,

    /// PostgreSQL connection URL (env: SENKO_POSTGRES_URL)
    #[arg(long)]
    postgres_url: Option<String>,

    /// Project name to operate on (overrides config; env: SENKO_PROJECT)
    #[arg(long)]
    project: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, serde::Serialize)]
struct DryRunOperation {
    command: String,
    operations: Vec<String>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Add a new task
    Add {
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        background: Option<String>,
        #[arg(long)]
        description: Option<String>,
        /// Priority (p0-p3)
        #[arg(long)]
        priority: Option<String>,
        #[arg(long)]
        definition_of_done: Vec<String>,
        #[arg(long)]
        in_scope: Vec<String>,
        #[arg(long)]
        out_of_scope: Vec<String>,
        #[arg(long)]
        tag: Vec<String>,
        #[arg(long)]
        depends_on: Vec<i64>,
        /// Git branch name (supports ${task_id} template)
        #[arg(long)]
        branch: Option<String>,
        /// Arbitrary JSON metadata
        #[arg(long)]
        metadata: Option<String>,
        /// Read JSON from stdin
        #[arg(long, conflicts_with_all = ["title", "background", "description", "priority", "definition_of_done", "in_scope", "out_of_scope", "tag", "depends_on", "branch", "metadata"])]
        from_json: bool,
        /// Read JSON from file
        #[arg(long, conflicts_with_all = ["title", "background", "description", "priority", "definition_of_done", "in_scope", "out_of_scope", "tag", "depends_on", "branch", "metadata", "from_json"])]
        from_json_file: Option<PathBuf>,
    },
    /// List tasks
    List {
        /// Filter by status (draft, todo, in_progress, completed, canceled); repeatable
        #[arg(long)]
        status: Vec<String>,
        /// Filter by tag; repeatable
        #[arg(long)]
        tag: Vec<String>,
        /// Filter by dependency (show tasks that depend on this task ID)
        #[arg(long)]
        depends_on: Option<i64>,
        /// Show only ready tasks (todo with all deps completed)
        #[arg(long)]
        ready: bool,
    },
    /// Get task details
    Get {
        /// Task ID
        task_id: i64,
    },
    /// Show the next task to work on
    Next {
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        user_id: Option<i64>,
    },
    /// Transition a task from draft to todo
    Ready {
        /// Task ID
        id: i64,
    },
    /// Transition a task from todo to in_progress
    Start {
        /// Task ID
        id: i64,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        user_id: Option<i64>,
    },
    /// Edit a task
    Edit {
        /// Task ID
        id: i64,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        background: Option<String>,
        #[arg(long)]
        clear_background: bool,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        clear_description: bool,
        #[arg(long)]
        plan: Option<String>,
        /// Read plan text from file
        #[arg(long, conflicts_with = "plan")]
        plan_file: Option<PathBuf>,
        #[arg(long)]
        clear_plan: bool,
        #[arg(long, value_enum)]
        priority: Option<Priority>,
        /// Git branch name (supports ${task_id} template)
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        clear_branch: bool,
        /// PR URL associated with this task
        #[arg(long)]
        pr_url: Option<String>,
        #[arg(long)]
        clear_pr_url: bool,
        /// Arbitrary JSON metadata
        #[arg(long)]
        metadata: Option<String>,
        #[arg(long)]
        clear_metadata: bool,
        // Array set
        #[arg(long, num_args = 0..)]
        set_tags: Option<Vec<String>>,
        #[arg(long, num_args = 0..)]
        set_definition_of_done: Option<Vec<String>>,
        #[arg(long, num_args = 0..)]
        set_in_scope: Option<Vec<String>>,
        #[arg(long, num_args = 0..)]
        set_out_of_scope: Option<Vec<String>>,
        // Array add
        #[arg(long)]
        add_tag: Vec<String>,
        #[arg(long)]
        add_definition_of_done: Vec<String>,
        #[arg(long)]
        add_in_scope: Vec<String>,
        #[arg(long)]
        add_out_of_scope: Vec<String>,
        // Array remove
        #[arg(long)]
        remove_tag: Vec<String>,
        #[arg(long)]
        remove_definition_of_done: Vec<String>,
        #[arg(long)]
        remove_in_scope: Vec<String>,
        #[arg(long)]
        remove_out_of_scope: Vec<String>,
    },
    /// Mark a task as complete
    Complete {
        /// Task ID
        id: i64,
        /// Skip PR merge/review verification
        #[arg(long)]
        skip_pr_check: bool,
    },
    /// Cancel a task
    Cancel {
        /// Task ID
        id: i64,
        /// Cancellation reason
        #[arg(long)]
        reason: Option<String>,
    },
    /// Manage Definition of Done items
    Dod {
        #[command(subcommand)]
        command: DodCommand,
    },
    /// Manage task dependencies
    Deps {
        #[command(subcommand)]
        command: DepsCommand,
    },
    /// Start a read-only web viewer
    Web {
        /// Port to listen on (env: SENKO_PORT, default: 3141)
        #[arg(long)]
        port: Option<u16>,
        /// Bind address, e.g. 0.0.0.0 or 192.168.1.5 (env: SENKO_HOST, default: 127.0.0.1)
        #[arg(long)]
        host: Option<String>,
    },
    /// Start a JSON REST API server
    Serve {
        /// Port to listen on (env: SENKO_PORT, default: 3142)
        #[arg(long)]
        port: Option<u16>,
        /// Bind address, e.g. 0.0.0.0 or 192.168.1.5 (env: SENKO_HOST, default: 127.0.0.1)
        #[arg(long)]
        host: Option<String>,
    },
    /// Install a skill
    SkillInstall {
        /// Output directory for SKILL.md
        #[arg(long)]
        output_dir: Option<PathBuf>,
        /// Skip confirmation prompts
        #[arg(long)]
        yes: bool,
    },
    /// Manage hooks
    Hooks {
        #[command(subcommand)]
        command: HooksCommand,
    },
    /// Check hook configuration for issues
    Doctor,
    /// Show or initialize workflow configuration
    #[command(name = "config")]
    Config {
        /// Generate a template config.toml
        #[arg(long)]
        init: bool,
    },
    /// Manage projects
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },
    /// Manage users
    User {
        #[command(subcommand)]
        action: UserAction,
    },
    /// Manage project members
    Members {
        #[command(subcommand)]
        action: MemberAction,
    },
}

#[derive(Debug, Subcommand)]
enum HooksCommand {
    /// View hook execution log
    Log {
        /// Number of recent entries to show (default: 20)
        #[arg(short, long, default_value_t = 20)]
        n: usize,
        /// Follow log output (like tail -f)
        #[arg(short, long)]
        follow: bool,
        /// Clear the log file
        #[arg(long)]
        clear: bool,
        /// Print the log file path
        #[arg(long)]
        path: bool,
    },
    /// Test hooks by running them synchronously
    Test {
        /// Event name (task_added, task_ready, task_started, task_completed, task_canceled)
        event_name: String,
        /// Task ID to use for building the event (uses a sample task if omitted)
        task_id: Option<i64>,
        /// Show event JSON without executing hooks
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
enum DepsCommand {
    /// Add a dependency
    Add {
        /// Task ID
        task_id: i64,
        /// Dependency task ID
        #[arg(long)]
        on: i64,
    },
    /// Remove a dependency
    Remove {
        /// Task ID
        task_id: i64,
        /// Dependency task ID
        #[arg(long)]
        on: i64,
    },
    /// Replace all dependencies
    Set {
        /// Task ID
        task_id: i64,
        /// Dependency task IDs
        #[arg(long, num_args = 1..)]
        on: Vec<i64>,
    },
    /// List dependencies
    List {
        /// Task ID
        task_id: i64,
    },
}

#[derive(Debug, Subcommand)]
enum ProjectAction {
    /// List all projects
    List,
    /// Create a new project
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a project
    Delete {
        /// Project ID
        id: i64,
    },
}

#[derive(Debug, Subcommand)]
enum UserAction {
    /// List all users
    List,
    /// Create a new user
    Create {
        #[arg(long)]
        username: String,
        #[arg(long)]
        display_name: Option<String>,
        #[arg(long)]
        email: Option<String>,
    },
    /// Delete a user
    Delete {
        /// User ID
        id: i64,
    },
}

#[derive(Debug, Subcommand)]
enum MemberAction {
    /// List project members
    List,
    /// Add a member to the project
    Add {
        #[arg(long)]
        user_id: i64,
        #[arg(long)]
        role: Option<Role>,
    },
    /// Remove a member from the project
    Remove {
        #[arg(long)]
        user_id: i64,
    },
    /// Update a member's role
    SetRole {
        #[arg(long)]
        user_id: i64,
        #[arg(long)]
        role: Role,
    },
}

#[derive(Debug, Subcommand)]
enum DodCommand {
    /// Mark a DoD item as checked
    Check {
        /// Task ID
        task_id: i64,
        /// DoD item index (1-based)
        index: usize,
    },
    /// Unmark a DoD item
    Uncheck {
        /// Task ID
        task_id: i64,
        /// DoD item index (1-based)
        index: usize,
    },
}

fn print_dry_run(output: &OutputFormat, ops: &DryRunOperation) -> Result<()> {
    match output {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(ops)?),
        OutputFormat::Text => {
            for op in &ops.operations {
                println!("{}", op);
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let output_format = cli.output.clone();

    if let Err(e) = run(cli).await {
        match output_format {
            OutputFormat::Json => {
                println!("{}", serde_json::json!({"error": format!("{:#}", e)}));
                std::process::exit(1);
            }
            OutputFormat::Text => {
                eprintln!("Error: {:#}", e);
                std::process::exit(1);
            }
        }
    }
}

async fn run(cli: Cli) -> Result<()> {
    let dry_run = cli.dry_run;
    let output_format = cli.output.clone();

    match cli.command {
        Command::Add {
            ref title,
            ref background,
            ref description,
            ref priority,
            ref definition_of_done,
            ref in_scope,
            ref out_of_scope,
            ref tag,
            ref depends_on,
            ref branch,
            ref metadata,
            from_json,
            ref from_json_file,
        } => cmd_add(
            &cli,
            title.clone(),
            background.clone(),
            description.clone(),
            priority.clone(),
            definition_of_done.clone(),
            in_scope.clone(),
            out_of_scope.clone(),
            tag.clone(),
            depends_on.clone(),
            branch.clone(),
            metadata.clone(),
            from_json,
            from_json_file.clone(),
        ).await,
        Command::List {
            ref status,
            ref tag,
            ref depends_on,
            ready,
        } => cmd_list(&cli, status.clone(), tag.clone(), *depends_on, ready).await,
        Command::Get { task_id } => cmd_get(&cli, task_id).await,
        Command::Next { ref session_id, user_id } => cmd_next(&cli, session_id.clone(), user_id).await,
        Command::Ready { id } => cmd_ready(&cli, id).await,
        Command::Start { id, ref session_id, user_id } => cmd_start(&cli, id, session_id.clone(), user_id).await,
        Command::Edit {
            id,
            ref title,
            ref background,
            clear_background,
            ref description,
            clear_description,
            ref plan,
            ref plan_file,
            clear_plan,
            ref priority,
            ref branch,
            clear_branch,
            ref pr_url,
            clear_pr_url,
            ref metadata,
            clear_metadata,
            ref set_tags,
            ref set_definition_of_done,
            ref set_in_scope,
            ref set_out_of_scope,
            ref add_tag,
            ref add_definition_of_done,
            ref add_in_scope,
            ref add_out_of_scope,
            ref remove_tag,
            ref remove_definition_of_done,
            ref remove_in_scope,
            ref remove_out_of_scope,
        } => {
            let project_root = resolve_project_root(cli.project_root.as_deref())?;
            let config = load_config_with_cli(&project_root, &cli)?;
            let (backend, _) = create_backend(&project_root, &config)?;
            let project_id = resolve_project_id(&*backend, &config).await?;

            // Verify task exists (even in dry-run)
            let _task = backend.get_task(project_id, id).await?;

            // Resolve effective plan: --plan-file reads from file, --plan uses inline value
            let effective_plan = if let Some(path) = plan_file {
                Some(std::fs::read_to_string(path)?)
            } else {
                plan.clone()
            };

            if dry_run {
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
                return print_dry_run(&output_format, &DryRunOperation { command: "edit".into(), operations });
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
                add_tags: add_tag.clone(),
                remove_tags: remove_tag.clone(),
                set_definition_of_done: set_definition_of_done.clone(),
                add_definition_of_done: add_definition_of_done.clone(),
                remove_definition_of_done: remove_definition_of_done.clone(),
                set_in_scope: set_in_scope.clone(),
                add_in_scope: add_in_scope.clone(),
                remove_in_scope: remove_in_scope.clone(),
                set_out_of_scope: set_out_of_scope.clone(),
                add_out_of_scope: add_out_of_scope.clone(),
                remove_out_of_scope: remove_out_of_scope.clone(),
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
        Command::Complete { id, skip_pr_check } => cmd_complete(&cli, id, skip_pr_check).await,
        Command::Cancel { id, ref reason } => cmd_cancel(&cli, id, reason.clone()).await,
        Command::Dod { ref command } => cmd_dod(&cli, command).await,
        Command::Deps { ref command } => cmd_deps(&cli, command).await,
        Command::Web { port, ref host } => {
            let root = resolve_project_root(cli.project_root.as_deref())?;
            let mut config = load_config_with_cli(&root, &cli)?;
            config.apply_cli(&CliOverrides { port, host: host.clone(), ..Default::default() });
            let (backend, _) = create_backend(&root, &config)?;
            let port_is_explicit = config.web_port_is_explicit();
            let effective_port = config.web_port_or(3141);
            senko::presentation::web::serve(root, effective_port, port_is_explicit, &config, backend).await?;
            Ok(())
        }
        Command::Serve { port, ref host } => {
            let root = resolve_project_root(cli.project_root.as_deref())?;
            let mut config = load_config_with_cli(&root, &cli)?;
            config.apply_cli(&CliOverrides { port, host: host.clone(), ..Default::default() });
            let (backend, _) = create_backend(&root, &config)?;
            let port_is_explicit = config.web_port_is_explicit();
            let effective_port = config.web_port_or(3142);
            senko::presentation::api::serve(root, effective_port, port_is_explicit, &config, cli.config.clone(), backend).await?;
            Ok(())
        }
        Command::SkillInstall { ref output_dir, yes } => {
            skill_install(&cli, output_dir.clone(), yes)
        }
        Command::Hooks { ref command } => cmd_hooks(&cli, command).await,
        Command::Doctor => cmd_doctor(&cli),
        Command::Config { init } => cmd_config(&cli, init),
        Command::Project { ref action } => cmd_project(&cli, action).await,
        Command::User { ref action } => cmd_user(&cli, action).await,
        Command::Members { ref action } => cmd_members(&cli, action).await,
    }
}

#[allow(clippy::too_many_arguments)]
async fn cmd_add(
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
    let config = load_config_with_cli(&root, cli)?;
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

async fn cmd_list(
    cli: &Cli,
    status: Vec<String>,
    tag: Vec<String>,
    depends_on: Option<i64>,
    ready: bool,
) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config_with_cli(&root, cli)?;
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

async fn cmd_get(cli: &Cli, task_id: i64) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config_with_cli(&root, cli)?;
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

async fn cmd_ready(cli: &Cli, id: i64) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config_with_cli(&root, cli)?;
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

async fn cmd_start(cli: &Cli, id: i64, session_id: Option<String>, user_id: Option<i64>) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config_with_cli(&root, cli)?;
    let (backend, using_http) = create_backend(&root, &config)?;
    let project_id = resolve_project_id(&*backend, &config).await?;

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

async fn cmd_next(cli: &Cli, session_id: Option<String>, user_id: Option<i64>) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config_with_cli(&root, cli)?;
    let (backend, using_http) = create_backend(&root, &config)?;
    let project_id = resolve_project_id(&*backend, &config).await?;

    if cli.dry_run {
        let backend_info = resolve_backend_info(&config, &root);
        let hook_executor = create_hook_executor(config, using_http, RuntimeMode::Cli, backend_info);
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
        let hook_executor = create_hook_executor(config, using_http, RuntimeMode::Cli, backend_info);
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

async fn cmd_complete(cli: &Cli, id: i64, skip_pr_check: bool) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config_with_cli(&root, cli)?;
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

async fn cmd_cancel(cli: &Cli, id: i64, reason: Option<String>) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config_with_cli(&root, cli)?;
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

const CONFIG_TEMPLATE: &str = r#"# senko configuration
# See: https://github.com/hisamekms/senko
#
# Config layering (priority high → low):
#   1. CLI flag (--config)
#   2. SENKO_CONFIG env var
#   3. Project config (.senko/config.toml)
#   4. User config (~/.config/senko/config.toml)

# Named hooks: [hooks.<event>.<name>]
# Each hook has a `command` and optional `enabled` (default: true).
# Set `enabled = false` to disable a hook inherited from user config.
#
# [hooks.on_task_added.my-hook]
# command = "echo 'task added'"
#
# [hooks.on_task_ready.my-hook]
# command = "echo 'task ready'"
#
# [hooks.on_task_completed.my-hook]
# command = "echo 'task completed'"

[workflow]
# completion_mode = "merge_then_complete"  # or "pr_then_complete"
# auto_merge = true

[backend]
# api_url = "http://127.0.0.1:3142"  # uncomment to use HTTP backend
# hook_mode = "server"  # "server" (default), "client", or "both"

[log]
# dir = "/custom/path/to/logs"  # override log output directory (default: $XDG_STATE_HOME/senko)

[project]
# name = "default"  # project name to operate on (overrides with --project flag or SENKO_PROJECT env)
"#;

async fn cmd_hooks(cli: &Cli, command: &HooksCommand) -> Result<()> {
    match command {
        HooksCommand::Log {
            n,
            follow,
            clear,
            path,
        } => {
            let root = resolve_project_root(cli.project_root.as_deref())?;
            let config = load_config_with_cli(&root, cli)?;
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
            let config = load_config_with_cli(&root, cli)?;
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
                    runtime: RuntimeMode::Cli,
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
                use senko::domain::task::{Priority, TaskStatus};
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
                runtime: RuntimeMode::Cli,
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
    checks: Vec<DoctorCheckResult>,
}

#[derive(Debug, serde::Serialize)]
struct DoctorCheckResult {
    check: String,
    target: String,
    status: DoctorCheckStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum DoctorCheckStatus {
    Ok,
    Error,
}

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

fn extract_script_path(command: &str) -> Option<String> {
    let first_token = command.split_whitespace().next()?;
    let expanded = expand_tilde(first_token);
    if expanded.contains('/') || expanded.starts_with('.') {
        Some(expanded)
    } else {
        None
    }
}

fn run_hook_checks(entry: &HookEntry) -> Vec<DoctorCheckResult> {
    let mut checks = Vec::new();

    for var in &entry.requires_env {
        let (status, message) = if std::env::var(var).is_ok() {
            (DoctorCheckStatus::Ok, None)
        } else {
            (DoctorCheckStatus::Error, Some(format!("{var} is not set")))
        };
        checks.push(DoctorCheckResult {
            check: "env_var".to_string(),
            target: var.clone(),
            status,
            message,
        });
    }

    if let Some(script_path) = extract_script_path(&entry.command) {
        let path = std::path::Path::new(&script_path);
        if path.exists() {
            checks.push(DoctorCheckResult {
                check: "script_exists".to_string(),
                target: script_path.clone(),
                status: DoctorCheckStatus::Ok,
                message: None,
            });

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let is_executable = path
                    .metadata()
                    .map(|m| m.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false);
                let (status, message) = if is_executable {
                    (DoctorCheckStatus::Ok, None)
                } else {
                    (DoctorCheckStatus::Error, Some("not executable".to_string()))
                };
                checks.push(DoctorCheckResult {
                    check: "script_executable".to_string(),
                    target: script_path,
                    status,
                    message,
                });
            }
        } else {
            checks.push(DoctorCheckResult {
                check: "script_exists".to_string(),
                target: script_path,
                status: DoctorCheckStatus::Error,
                message: Some("file not found".to_string()),
            });
        }
    }

    checks
}

fn cmd_doctor(cli: &Cli) -> Result<()> {
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
        .any(|d| d.checks.iter().any(|c| c.status == DoctorCheckStatus::Error));

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
                            DoctorCheckStatus::Ok => "\u{2713}",
                            DoctorCheckStatus::Error => "\u{2717}",
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
                .filter(|c| c.status == DoctorCheckStatus::Error)
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

fn cmd_config(cli: &Cli, init: bool) -> Result<()> {
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

async fn cmd_dod(cli: &Cli, command: &DodCommand) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config_with_cli(&root, cli)?;
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

fn print_dod_items(items: &[senko::domain::task::DodItem]) {
    for item in items {
        let mark = if item.checked() { "x" } else { " " };
        println!("  [{mark}] {}", item.content());
    }
}

async fn cmd_deps(cli: &Cli, command: &DepsCommand) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config_with_cli(&root, cli)?;
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

const SKILL_MD_CONTENT: &str = include_str!("skill_md.txt");
const DOD_VERIFIER_AGENT_CONTENT: &str = include_str!("dod_verifier_agent.md");

/// File to install with its relative path under `.claude/` and content.
struct InstallableFile {
    /// Path segments under `.claude/` (e.g. `["skills", "senko", "SKILL.md"]`)
    segments: &'static [&'static str],
    content: &'static str,
}

const INSTALLABLE_FILES: &[InstallableFile] = &[
    InstallableFile {
        segments: &["skills", "senko", "SKILL.md"],
        content: SKILL_MD_CONTENT,
    },
    InstallableFile {
        segments: &["agents", "dod-verifier.md"],
        content: DOD_VERIFIER_AGENT_CONTENT,
    },
];

/// Check if a file needs to be written and optionally prompt for overwrite confirmation.
/// Returns `true` if the file should be written.
fn should_write_file(path: &std::path::Path, content: &str, yes: bool) -> Result<bool> {
    if !path.exists() {
        return Ok(true);
    }
    let existing = fs::read_to_string(path)
        .with_context(|| format!("failed to read existing file: {}", path.display()))?;
    if existing == content {
        println!("{} is up to date", path.display());
        return Ok(false);
    }
    if yes {
        return Ok(true);
    }
    eprint!(
        "{} already exists and differs. Overwrite? [y/N] ",
        path.display()
    );
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .context("failed to read from stdin")?;
    if input.trim().eq_ignore_ascii_case("y") {
        Ok(true)
    } else {
        println!("Skipped {}", path.display());
        Ok(false)
    }
}

fn skill_install(cli: &Cli, output_dir: Option<PathBuf>, yes: bool) -> Result<()> {
    if cli.dry_run {
        let operations: Vec<String> = if let Some(ref dir) = output_dir {
            INSTALLABLE_FILES
                .iter()
                .map(|f| {
                    let filename = f.segments.last().unwrap();
                    format!("Write {} to {}", filename, dir.join(filename).display())
                })
                .collect()
        } else {
            let project_root = resolve_project_root(cli.project_root.as_deref())?;
            let claude_dir = project_root.join(".claude");
            INSTALLABLE_FILES
                .iter()
                .map(|f| {
                    let path = f.segments.iter().fold(claude_dir.clone(), |p, s| p.join(s));
                    format!("Write {}", path.display())
                })
                .collect()
        };
        return print_dry_run(&cli.output, &DryRunOperation { command: "skill-install".into(), operations });
    }

    if let Some(dir) = output_dir {
        for file in INSTALLABLE_FILES {
            let filename = file.segments.last().unwrap();
            let path = dir.join(filename);
            if should_write_file(&path, file.content, yes)? {
                fs::write(&path, file.content)?;
                println!("{} written to {}", filename, path.display());
            }
        }
        return Ok(());
    }

    let project_root = resolve_project_root(cli.project_root.as_deref())?;
    let claude_dir = project_root.join(".claude");
    let created_claude_dir = !claude_dir.exists();

    if created_claude_dir && !yes {
        eprint!(
            ".claude/ directory does not exist. Create it at {}? [y/N] ",
            claude_dir.display()
        );
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .context("failed to read from stdin")?;
        if !input.trim().eq_ignore_ascii_case("y") {
            bail!("aborted");
        }
    }

    for file in INSTALLABLE_FILES {
        let path = file.segments.iter().fold(claude_dir.clone(), |p, s| p.join(s));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory: {}", parent.display()))?;
        }
        if should_write_file(&path, file.content, yes)? {
            fs::write(&path, file.content)?;
            println!("{} written", path.display());
        }
    }

    if created_claude_dir {
        println!("Created .claude/ directory at {}", claude_dir.display());
    }

    Ok(())
}

async fn cmd_project(cli: &Cli, action: &ProjectAction) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config_with_cli(&root, cli)?;
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
            project_service.delete_project(*id, None).await?;
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

async fn cmd_user(cli: &Cli, action: &UserAction) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config_with_cli(&root, cli)?;
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

async fn cmd_members(cli: &Cli, action: &MemberAction) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let config = load_config_with_cli(&root, cli)?;
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
                .add_project_member(DEFAULT_PROJECT_ID, &params, None)
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
                .remove_project_member(DEFAULT_PROJECT_ID, *user_id, None)
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
                .update_member_role(DEFAULT_PROJECT_ID, *user_id, *role, None)
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
    use clap::Parser;

    use super::*;
    use senko::domain::repository::{ProjectRepository, TaskRepository};

    #[test]
    fn parse_add_subcommand() {
        let cli = Cli::parse_from(["senko", "add"]);
        assert!(matches!(cli.command, Command::Add { .. }));
    }

    #[test]
    fn parse_add_with_title() {
        let cli = Cli::parse_from(["senko", "add", "--title", "my task"]);
        match cli.command {
            Command::Add { title, .. } => assert_eq!(title, Some("my task".to_string())),
            _ => panic!("expected Add"),
        }
    }

    #[test]
    fn parse_add_with_all_flags() {
        let cli = Cli::parse_from([
            "senko",
            "add",
            "--title",
            "task",
            "--background",
            "bg",
            "--description",
            "det",
            "--priority",
            "p1",
            "--definition-of-done",
            "done1",
            "--definition-of-done",
            "done2",
            "--in-scope",
            "s1",
            "--out-of-scope",
            "o1",
            "--tag",
            "rust",
            "--tag",
            "cli",
            "--depends-on",
            "1",
            "--depends-on",
            "2",
        ]);
        match cli.command {
            Command::Add {
                title,
                background,
                description,
                priority,
                definition_of_done,
                in_scope,
                out_of_scope,
                tag,
                depends_on,
                branch,
                metadata: _,
                from_json,
                from_json_file,
            } => {
                assert_eq!(title, Some("task".to_string()));
                assert_eq!(background, Some("bg".to_string()));
                assert_eq!(description, Some("det".to_string()));
                assert_eq!(priority, Some("p1".to_string()));
                assert_eq!(definition_of_done, vec!["done1", "done2"]);
                assert_eq!(in_scope, vec!["s1"]);
                assert_eq!(out_of_scope, vec!["o1"]);
                assert_eq!(tag, vec!["rust", "cli"]);
                assert_eq!(depends_on, vec![1, 2]);
                assert!(branch.is_none());
                assert!(!from_json);
                assert!(from_json_file.is_none());
            }
            _ => panic!("expected Add"),
        }
    }

    #[test]
    fn parse_add_with_from_json() {
        let cli = Cli::parse_from(["senko", "add", "--from-json"]);
        match cli.command {
            Command::Add {
                from_json, title, ..
            } => {
                assert!(from_json);
                assert!(title.is_none());
            }
            _ => panic!("expected Add"),
        }
    }

    #[test]
    fn parse_add_with_from_json_file() {
        let cli = Cli::parse_from(["senko", "add", "--from-json-file", "/tmp/task.json"]);
        match cli.command {
            Command::Add {
                from_json_file,
                from_json,
                title,
                ..
            } => {
                assert_eq!(from_json_file, Some(PathBuf::from("/tmp/task.json")));
                assert!(!from_json);
                assert!(title.is_none());
            }
            _ => panic!("expected Add"),
        }
    }

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

        let backend = db::SqliteBackend::new(tmp.path(), Some(&tmp.path().join("data.db")), None).unwrap();
        let task = backend.get_task(DEFAULT_PROJECT_ID, 1).await.unwrap();
        assert_eq!(task.title(), "test task");
        assert_eq!(task.background(), Some("bg"));
        assert_eq!(task.priority(), senko::domain::task::Priority::P1);
        assert_eq!(task.definition_of_done().len(), 1);
        assert_eq!(task.definition_of_done()[0].content(), "done");
        assert_eq!(task.definition_of_done()[0].checked(), false);
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

        let backend = db::SqliteBackend::new(tmp.path(), Some(&tmp.path().join("data.db")), None).unwrap();
        let task = backend.get_task(DEFAULT_PROJECT_ID, 1).await.unwrap();
        assert_eq!(task.title(), "file task");
        assert_eq!(task.priority(), senko::domain::task::Priority::P0);
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
        let backend = db::SqliteBackend::new(tmp.path(), Some(&tmp.path().join("data.db")), None).unwrap();
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
        let backend = db::SqliteBackend::new(tmp.path(), Some(&tmp.path().join("data.db")), None).unwrap();
        let task = backend.get_task(DEFAULT_PROJECT_ID, 1).await.unwrap();
        assert_eq!(task.title(), "json out");
    }

    #[test]
    fn parse_list_subcommand() {
        let cli = Cli::parse_from(["senko", "list"]);
        assert!(matches!(cli.command, Command::List { .. }));
    }

    #[test]
    fn parse_list_with_filters() {
        let cli = Cli::parse_from([
            "senko", "list", "--status", "todo", "--status", "in_progress",
            "--tag", "rust", "--tag", "web", "--depends-on", "3",
            "--ready",
        ]);
        match cli.command {
            Command::List {
                status,
                tag,
                depends_on,
                ready,
            } => {
                assert_eq!(status, vec!["todo", "in_progress"]);
                assert_eq!(tag, vec!["rust", "web"]);
                assert_eq!(depends_on, Some(3));
                assert!(ready);
            }
            _ => panic!("expected List"),
        }
    }

    #[test]
    fn parse_get_subcommand() {
        let cli = Cli::parse_from(["senko", "get", "42"]);
        match cli.command {
            Command::Get { task_id } => assert_eq!(task_id, 42),
            _ => panic!("expected Get"),
        }
    }

    #[test]
    fn parse_next_subcommand() {
        let cli = Cli::parse_from(["senko", "next"]);
        assert!(matches!(cli.command, Command::Next { .. }));
    }

    #[test]
    fn parse_next_with_session_id() {
        let cli = Cli::parse_from(["senko", "next", "--session-id", "abc-123"]);
        match cli.command {
            Command::Next { session_id, .. } => {
                assert_eq!(session_id, Some("abc-123".to_string()));
            }
            _ => panic!("expected Next"),
        }
    }

    #[test]
    fn parse_edit_subcommand() {
        let cli = Cli::parse_from(["senko", "edit", "1"]);
        assert!(matches!(cli.command, Command::Edit { id: 1, .. }));
    }

    #[test]
    fn parse_edit_with_scalar_args() {
        let cli = Cli::parse_from([
            "senko", "edit", "5", "--title", "new title", "--priority", "p0",
        ]);
        match cli.command {
            Command::Edit {
                id,
                title,
                priority,
                ..
            } => {
                assert_eq!(id, 5);
                assert_eq!(title.as_deref(), Some("new title"));
                assert_eq!(priority, Some(Priority::P0));
            }
            _ => panic!("expected Edit"),
        }
    }

    #[test]
    fn parse_ready_command() {
        let cli = Cli::parse_from(["senko", "ready", "3"]);
        match cli.command {
            Command::Ready { id } => assert_eq!(id, 3),
            _ => panic!("expected Ready"),
        }
    }

    #[test]
    fn parse_start_command() {
        let cli = Cli::parse_from(["senko", "start", "5", "--session-id", "abc"]);
        match cli.command {
            Command::Start { id, session_id, .. } => {
                assert_eq!(id, 5);
                assert_eq!(session_id.as_deref(), Some("abc"));
            }
            _ => panic!("expected Start"),
        }
    }

    #[test]
    fn parse_edit_with_array_args() {
        let cli = Cli::parse_from([
            "senko",
            "edit",
            "3",
            "--add-tag",
            "rust",
            "--add-tag",
            "cli",
            "--remove-tag",
            "old",
            "--set-in-scope",
            "a",
            "b",
        ]);
        match cli.command {
            Command::Edit {
                id,
                add_tag,
                remove_tag,
                set_in_scope,
                ..
            } => {
                assert_eq!(id, 3);
                assert_eq!(add_tag, vec!["rust", "cli"]);
                assert_eq!(remove_tag, vec!["old"]);
                assert_eq!(set_in_scope, Some(vec!["a".to_string(), "b".to_string()]));
            }
            _ => panic!("expected Edit"),
        }
    }

    #[test]
    fn parse_edit_clear_background() {
        let cli = Cli::parse_from(["senko", "edit", "1", "--clear-background"]);
        match cli.command {
            Command::Edit {
                clear_background, ..
            } => {
                assert!(clear_background);
            }
            _ => panic!("expected Edit"),
        }
    }

    #[test]
    fn parse_complete_subcommand() {
        let cli = Cli::parse_from(["senko", "complete", "1"]);
        assert!(matches!(cli.command, Command::Complete { id: 1, .. }));
    }

    #[test]
    fn parse_cancel_subcommand() {
        let cli = Cli::parse_from(["senko", "cancel", "2"]);
        assert!(matches!(cli.command, Command::Cancel { id: 2, .. }));
    }

    #[test]
    fn parse_cancel_with_reason() {
        let cli = Cli::parse_from(["senko", "cancel", "3", "--reason", "no longer needed"]);
        match cli.command {
            Command::Cancel { id, reason } => {
                assert_eq!(id, 3);
                assert_eq!(reason.as_deref(), Some("no longer needed"));
            }
            _ => panic!("expected Cancel"),
        }
    }

    #[test]
    fn parse_cancel_without_reason() {
        let cli = Cli::parse_from(["senko", "cancel", "4"]);
        match cli.command {
            Command::Cancel { id, reason } => {
                assert_eq!(id, 4);
                assert!(reason.is_none());
            }
            _ => panic!("expected Cancel"),
        }
    }

    #[test]
    fn parse_deps_add() {
        let cli = Cli::parse_from(["senko", "deps", "add", "1", "--on", "2"]);
        match cli.command {
            Command::Deps { command: DepsCommand::Add { task_id, on } } => {
                assert_eq!(task_id, 1);
                assert_eq!(on, 2);
            }
            _ => panic!("expected Deps Add"),
        }
    }

    #[test]
    fn parse_deps_remove() {
        let cli = Cli::parse_from(["senko", "deps", "remove", "3", "--on", "4"]);
        match cli.command {
            Command::Deps { command: DepsCommand::Remove { task_id, on } } => {
                assert_eq!(task_id, 3);
                assert_eq!(on, 4);
            }
            _ => panic!("expected Deps Remove"),
        }
    }

    #[test]
    fn parse_deps_set() {
        let cli = Cli::parse_from(["senko", "deps", "set", "1", "--on", "2", "3", "4"]);
        match cli.command {
            Command::Deps { command: DepsCommand::Set { task_id, on } } => {
                assert_eq!(task_id, 1);
                assert_eq!(on, vec![2, 3, 4]);
            }
            _ => panic!("expected Deps Set"),
        }
    }

    #[test]
    fn parse_deps_list() {
        let cli = Cli::parse_from(["senko", "deps", "list", "5"]);
        match cli.command {
            Command::Deps { command: DepsCommand::List { task_id } } => {
                assert_eq!(task_id, 5);
            }
            _ => panic!("expected Deps List"),
        }
    }

    #[test]
    fn parse_skill_install_subcommand() {
        let cli = Cli::parse_from(["senko", "skill-install"]);
        assert!(matches!(cli.command, Command::SkillInstall { .. }));
    }

    #[test]
    fn parse_skill_install_with_output_dir() {
        let cli = Cli::parse_from(["senko", "skill-install", "--output-dir", "/tmp/out"]);
        match cli.command {
            Command::SkillInstall { output_dir, yes } => {
                assert_eq!(output_dir, Some(PathBuf::from("/tmp/out")));
                assert!(!yes);
            }
            _ => panic!("expected SkillInstall"),
        }
    }

    #[test]
    fn parse_skill_install_without_output_dir() {
        let cli = Cli::parse_from(["senko", "skill-install"]);
        match cli.command {
            Command::SkillInstall { output_dir, yes } => {
                assert!(output_dir.is_none());
                assert!(!yes);
            }
            _ => panic!("expected SkillInstall"),
        }
    }

    #[test]
    fn parse_skill_install_with_yes() {
        let cli = Cli::parse_from(["senko", "skill-install", "--yes"]);
        match cli.command {
            Command::SkillInstall { output_dir, yes } => {
                assert!(output_dir.is_none());
                assert!(yes);
            }
            _ => panic!("expected SkillInstall"),
        }
    }

    #[test]
    fn skill_install_with_output_dir_creates_files() {
        let dir = tempfile::tempdir().unwrap();
        let cli = Cli {
            output: OutputFormat::Text,
            project_root: None,
            config: None,
            dry_run: false,
            log_dir: None,
            db_path: None,
            postgres_url: None,
            project: None,
            command: Command::SkillInstall {
                output_dir: Some(dir.path().to_path_buf()),
                yes: false,
            },
        };
        skill_install(&cli, Some(dir.path().to_path_buf()), false).unwrap();

        let content = std::fs::read_to_string(dir.path().join("SKILL.md")).unwrap();
        assert_eq!(content, SKILL_MD_CONTENT);
        let agent_content =
            std::fs::read_to_string(dir.path().join("dod-verifier.md")).unwrap();
        assert_eq!(agent_content, DOD_VERIFIER_AGENT_CONTENT);
    }

    #[test]
    fn skill_install_default_places_in_claude_skills() {
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
            command: Command::SkillInstall {
                output_dir: None,
                yes: true,
            },
        };
        skill_install(&cli, None, true).unwrap();

        let skill_path = tmp
            .path()
            .join(".claude")
            .join("skills")
            .join("senko")
            .join("SKILL.md");
        assert!(skill_path.exists());
        let content = std::fs::read_to_string(&skill_path).unwrap();
        assert_eq!(content, SKILL_MD_CONTENT);

        let agent_path = tmp
            .path()
            .join(".claude")
            .join("agents")
            .join("dod-verifier.md");
        assert!(agent_path.exists());
        let agent_content = std::fs::read_to_string(&agent_path).unwrap();
        assert_eq!(agent_content, DOD_VERIFIER_AGENT_CONTENT);
    }

    #[test]
    fn skill_install_existing_claude_dir() {
        let tmp = tempfile::tempdir().unwrap();
        // Pre-create .claude/
        std::fs::create_dir_all(tmp.path().join(".claude")).unwrap();

        let cli = Cli {
            output: OutputFormat::Text,
            project_root: Some(tmp.path().to_path_buf()),
            config: None,
            dry_run: false,
            log_dir: None,
            db_path: Some(tmp.path().join("data.db")),
            postgres_url: None,
            project: None,
            command: Command::SkillInstall {
                output_dir: None,
                yes: false,
            },
        };
        // Should not prompt since .claude/ already exists
        skill_install(&cli, None, false).unwrap();

        let skill_path = tmp
            .path()
            .join(".claude")
            .join("skills")
            .join("senko")
            .join("SKILL.md");
        assert!(skill_path.exists());

        let agent_path = tmp
            .path()
            .join(".claude")
            .join("agents")
            .join("dod-verifier.md");
        assert!(agent_path.exists());
    }

    #[test]
    fn skill_install_no_claude_dir_with_yes() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!tmp.path().join(".claude").exists());

        let cli = Cli {
            output: OutputFormat::Text,
            project_root: Some(tmp.path().to_path_buf()),
            config: None,
            dry_run: false,
            log_dir: None,
            db_path: Some(tmp.path().join("data.db")),
            postgres_url: None,
            project: None,
            command: Command::SkillInstall {
                output_dir: None,
                yes: true,
            },
        };
        skill_install(&cli, None, true).unwrap();

        assert!(tmp.path().join(".claude").exists());
        assert!(tmp
            .path()
            .join(".claude/skills/senko/SKILL.md")
            .exists());
        assert!(tmp
            .path()
            .join(".claude/agents/dod-verifier.md")
            .exists());
    }

    #[test]
    fn should_write_file_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.md");
        assert!(should_write_file(&path, "content", false).unwrap());
    }

    #[test]
    fn should_write_file_same_content_skips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("existing.md");
        std::fs::write(&path, "same content").unwrap();
        assert!(!should_write_file(&path, "same content", false).unwrap());
    }

    #[test]
    fn should_write_file_different_content_with_yes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("existing.md");
        std::fs::write(&path, "old content").unwrap();
        assert!(should_write_file(&path, "new content", true).unwrap());
    }

    #[test]
    fn skill_install_skips_up_to_date_files() {
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
            command: Command::SkillInstall {
                output_dir: None,
                yes: true,
            },
        };
        // First install
        skill_install(&cli, None, true).unwrap();
        // Second install should succeed (files are up to date)
        skill_install(&cli, None, true).unwrap();

        let skill_path = tmp.path().join(".claude/skills/senko/SKILL.md");
        let content = std::fs::read_to_string(&skill_path).unwrap();
        assert_eq!(content, SKILL_MD_CONTENT);
    }

    #[test]
    fn skill_install_overwrites_with_yes() {
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
            command: Command::SkillInstall {
                output_dir: None,
                yes: true,
            },
        };
        // First install
        skill_install(&cli, None, true).unwrap();
        // Tamper with the file
        let skill_path = tmp.path().join(".claude/skills/senko/SKILL.md");
        std::fs::write(&skill_path, "modified content").unwrap();
        // Reinstall with --yes should overwrite
        skill_install(&cli, None, true).unwrap();
        let content = std::fs::read_to_string(&skill_path).unwrap();
        assert_eq!(content, SKILL_MD_CONTENT);
    }

    #[test]
    fn skill_md_covers_all_commands() {
        let commands = [
            "senko add",
            "senko list",
            "senko get",
            "senko next",
            "senko ready",
            "senko start",
            "senko edit",
            "senko complete",
            "senko cancel",
            "senko deps add",
            "senko deps remove",
            "senko deps set",
            "senko deps list",
            "senko dod check",
            "senko dod uncheck",
            "senko config",
        ];
        for cmd in commands {
            assert!(
                SKILL_MD_CONTENT.contains(cmd),
                "SKILL.md does not mention: {cmd}"
            );
        }
    }

    #[test]
    fn parse_output_json() {
        let cli = Cli::parse_from(["senko", "--output", "json", "add"]);
        assert!(matches!(cli.output, OutputFormat::Json));
    }

    #[test]
    fn parse_output_json_default() {
        let cli = Cli::parse_from(["senko", "add"]);
        assert!(matches!(cli.output, OutputFormat::Json));
    }

    #[test]
    fn parse_project_root() {
        let cli = Cli::parse_from(["senko", "--project-root", "/tmp/test", "add"]);
        assert_eq!(cli.project_root, Some(PathBuf::from("/tmp/test")));
    }

    #[test]
    fn parse_no_project_root() {
        let cli = Cli::parse_from(["senko", "add"]);
        assert!(cli.project_root.is_none());
    }
}
