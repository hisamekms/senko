use std::fs;
use std::io::Read;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand, ValueEnum};
use localflow::db::{self, TaskBackend};
use localflow::hooks::{self, HookMode};
use localflow::http_backend::HttpBackend;
use localflow::models::{
    CreateTaskParams, ListTasksFilter, Priority, Task, TaskStatus, UpdateTaskArrayParams,
    UpdateTaskParams,
};
use localflow::project::resolve_project_root;

/// Create the appropriate backend based on env var / config.
/// Returns (backend, is_http) where is_http indicates HTTP mode for hook control.
fn create_backend(
    project_root: &std::path::Path,
) -> Result<(Box<dyn TaskBackend>, bool)> {
    // 1. LOCALFLOW_API_URL env var takes priority
    if let Ok(url) = std::env::var("LOCALFLOW_API_URL") {
        if !url.is_empty() {
            return Ok((Box::new(HttpBackend::new(&url)), true));
        }
    }

    // 2. config.toml [backend] api_url
    let config = hooks::load_config(project_root)?;
    if let Some(ref url) = config.backend.api_url {
        return Ok((Box::new(HttpBackend::new(url)), true));
    }

    // 3. Default: SqliteBackend
    Ok((Box::new(db::SqliteBackend::new(project_root)?), false))
}

fn load_config_with_cli(root: &std::path::Path, cli: &Cli) -> Result<hooks::Config> {
    let mut config = hooks::load_config(root)?;
    if let Some(ref d) = cli.log_dir {
        config.log.dir = Some(d.to_string_lossy().into_owned());
    }
    Ok(config)
}

fn should_fire_client_hooks(config: &hooks::Config, using_http: bool) -> bool {
    match config.backend.hook_mode {
        HookMode::Server => !using_http,
        HookMode::Client | HookMode::Both => true,
    }
}

#[derive(Debug, Clone, ValueEnum)]
enum OutputFormat {
    Json,
    Text,
}

#[derive(Debug, Parser)]
#[command(name = "localflow", about = "Local task management CLI", version)]
struct Cli {
    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    output: OutputFormat,

    /// Project root directory
    #[arg(long)]
    project_root: Option<PathBuf>,

    /// Dry run mode: show what would be done without executing
    #[arg(long)]
    dry_run: bool,

    /// Override log output directory
    #[arg(long)]
    log_dir: Option<PathBuf>,

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
        /// Port to listen on (env: LOCALFLOW_PORT, default: 3141)
        #[arg(long)]
        port: Option<u16>,
        /// Bind address, e.g. 0.0.0.0 or 192.168.1.5 (env: LOCALFLOW_HOST, default: 127.0.0.1)
        #[arg(long)]
        host: Option<String>,
    },
    /// Start a JSON REST API server
    Serve {
        /// Port to listen on (env: LOCALFLOW_PORT, default: 3142)
        #[arg(long)]
        port: Option<u16>,
        /// Bind address, e.g. 0.0.0.0 or 192.168.1.5 (env: LOCALFLOW_HOST, default: 127.0.0.1)
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
    /// Show or initialize workflow configuration
    #[command(name = "config")]
    Config {
        /// Generate a template config.toml
        #[arg(long)]
        init: bool,
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

fn main() {
    let cli = Cli::parse();
    let output_format = cli.output.clone();

    if let Err(e) = run(cli) {
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

fn run(cli: Cli) -> Result<()> {
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
        ),
        Command::List {
            status,
            tag,
            depends_on,
            ready,
        } => cmd_list(
            &cli.output,
            cli.project_root.as_deref(),
            status,
            tag,
            depends_on,
            ready,
        ),
        Command::Get { task_id } => cmd_get(&cli.output, cli.project_root.as_deref(), task_id),
        Command::Next { ref session_id } => cmd_next(&cli, session_id.clone()),
        Command::Ready { id } => cmd_ready(&cli, id),
        Command::Start { id, ref session_id } => cmd_start(&cli, id, session_id.clone()),
        Command::Edit {
            id,
            title,
            background,
            clear_background,
            description,
            clear_description,
            plan,
            clear_plan,
            priority,
            branch,
            clear_branch,
            pr_url,
            clear_pr_url,
            metadata,
            clear_metadata,
            set_tags,
            set_definition_of_done,
            set_in_scope,
            set_out_of_scope,
            add_tag,
            add_definition_of_done,
            add_in_scope,
            add_out_of_scope,
            remove_tag,
            remove_definition_of_done,
            remove_in_scope,
            remove_out_of_scope,
        } => {
            let project_root = resolve_project_root(cli.project_root.as_deref())?;
            let (backend, _) = create_backend(&project_root)?;

            // Verify task exists (even in dry-run)
            let _task = backend.get_task(id)?;

            if dry_run {
                let mut operations = Vec::new();
                if let Some(ref t) = title {
                    operations.push(format!("Update task #{}: set title to \"{}\"", id, t));
                }
                if clear_background {
                    operations.push(format!("Update task #{}: clear background", id));
                } else if let Some(ref bg) = background {
                    operations.push(format!("Update task #{}: set background to \"{}\"", id, bg));
                }
                if clear_description {
                    operations.push(format!("Update task #{}: clear description", id));
                } else if let Some(ref desc) = description {
                    operations.push(format!("Update task #{}: set description to \"{}\"", id, desc));
                }
                if clear_plan {
                    operations.push(format!("Update task #{}: clear plan", id));
                } else if let Some(ref p) = plan {
                    operations.push(format!("Update task #{}: set plan to \"{}\"", id, p));
                }
                if let Some(ref p) = priority {
                    operations.push(format!("Update task #{}: set priority to {}", id, p));
                }
                if clear_branch {
                    operations.push(format!("Update task #{}: clear branch", id));
                } else if let Some(ref b) = branch {
                    operations.push(format!("Update task #{}: set branch to \"{}\"", id, b));
                }
                if clear_pr_url {
                    operations.push(format!("Update task #{}: clear pr_url", id));
                } else if let Some(ref url) = pr_url {
                    operations.push(format!("Update task #{}: set pr_url to \"{}\"", id, url));
                }
                if clear_metadata {
                    operations.push(format!("Update task #{}: clear metadata", id));
                } else if let Some(ref m) = metadata {
                    operations.push(format!("Update task #{}: set metadata to {}", id, m));
                }
                if let Some(ref tags) = set_tags {
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
                branch.map(|b| Some(expand_branch_template(&b, id)))
            };

            let scalar_params = UpdateTaskParams {
                title,
                background: if clear_background {
                    Some(None)
                } else {
                    background.map(Some)
                },
                description: if clear_description {
                    Some(None)
                } else {
                    description.map(Some)
                },
                plan: if clear_plan {
                    Some(None)
                } else {
                    plan.map(Some)
                },
                priority,
                assignee_session_id: None,
                started_at: None,
                completed_at: None,
                canceled_at: None,
                cancel_reason: None,
                branch: branch_value,
                pr_url: if clear_pr_url {
                    Some(None)
                } else {
                    pr_url.map(Some)
                },
                metadata: if clear_metadata {
                    Some(None)
                } else {
                    match metadata {
                        Some(m) => {
                            let val: serde_json::Value = serde_json::from_str(&m)
                                .context("invalid JSON for --metadata")?;
                            Some(Some(val))
                        }
                        None => None,
                    }
                },
            };

            let array_params = UpdateTaskArrayParams {
                set_tags,
                add_tags: add_tag,
                remove_tags: remove_tag,
                set_definition_of_done,
                add_definition_of_done,
                remove_definition_of_done,
                set_in_scope,
                add_in_scope,
                remove_in_scope,
                set_out_of_scope,
                add_out_of_scope,
                remove_out_of_scope,
            };

            backend.update_task(id, &scalar_params)?;
            backend.update_task_arrays(id, &array_params)?;
            let task = backend.get_task(id)?;

            match cli.output {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&task)?);
                }
                OutputFormat::Text => {
                    println!("Updated task {}", task.id);
                    println!("  title: {}", task.title);
                    println!("  status: {}", task.status);
                    println!("  priority: {}", task.priority);
                    if let Some(ref bg) = task.background {
                        println!("  background: {bg}");
                    }
                    if let Some(ref desc) = task.description {
                        println!("  description: {desc}");
                    }
                    if let Some(ref p) = task.plan {
                        println!("  plan: {p}");
                    }
                    if let Some(ref branch) = task.branch {
                        println!("  branch: {branch}");
                    }
                    if let Some(ref pr_url) = task.pr_url {
                        println!("  pr_url: {pr_url}");
                    }
                    if let Some(ref meta) = task.metadata {
                        println!("  metadata: {}", serde_json::to_string(meta)?);
                    }
                    if !task.tags.is_empty() {
                        println!("  tags: {}", task.tags.join(", "));
                    }
                }
            }
            Ok(())
        }
        Command::Complete { id, skip_pr_check } => cmd_complete(&cli, id, skip_pr_check),
        Command::Cancel { id, ref reason } => cmd_cancel(&cli, id, reason.clone()),
        Command::Dod { ref command } => cmd_dod(&cli, command),
        Command::Deps { ref command } => cmd_deps(&cli, command),
        Command::Web { port, host } => {
            let effective_port = port
                .or_else(|| std::env::var("LOCALFLOW_PORT").ok().and_then(|v| v.parse().ok()))
                .unwrap_or(3141);
            let root = resolve_project_root(cli.project_root.as_deref())?;
            let _ = db::SqliteBackend::new(&root)?; // web always uses SQLite directly
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(localflow::web::serve(root, effective_port, host))?;
            Ok(())
        }
        Command::Serve { port, host } => {
            let effective_port = port
                .or_else(|| std::env::var("LOCALFLOW_PORT").ok().and_then(|v| v.parse().ok()))
                .unwrap_or(3142);
            let root = resolve_project_root(cli.project_root.as_deref())?;
            let _ = db::open_db(&root)?; // validate DB exists
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(localflow::api::serve(root, effective_port, host))?;
            Ok(())
        }
        Command::SkillInstall { ref output_dir, yes } => {
            skill_install(&cli, output_dir.clone(), yes)
        }
        Command::Hooks { ref command } => cmd_hooks(&cli, command),
        Command::Config { init } => cmd_config(&cli, init),
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_add(
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
    let (backend, using_http) = create_backend(&root)?;

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

    // If branch contains ${task_id}, create without branch first, then update
    let needs_template = params
        .branch
        .as_ref()
        .is_some_and(|b| b.contains("${task_id}"));

    let task = if needs_template {
        let branch_template = params.branch.clone();
        let mut params_without_branch = params;
        params_without_branch.branch = None;
        let created = backend.create_task(&params_without_branch)?;
        let expanded = expand_branch_template(branch_template.as_deref().unwrap(), created.id);
        backend.update_task(
            created.id,
            &UpdateTaskParams {
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
                branch: Some(Some(expanded)),
                pr_url: None,
                metadata: None,
            },
        )?
    } else {
        backend.create_task(&params)?
    };

    // Fire hooks
    let config = load_config_with_cli(&root, cli)?;
    if should_fire_client_hooks(&config, using_http) {
        hooks::fire_hooks(&config, "task_added", &task, &*backend, None, None);
    }

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&task)?);
        }
        OutputFormat::Text => {
            println!("Created task #{}: \"{}\"", task.id, task.title);
        }
    }

    Ok(())
}

fn expand_branch_template(branch: &str, task_id: i64) -> String {
    branch.replace("${task_id}", &task_id.to_string())
}

fn cmd_list(
    output: &OutputFormat,
    project_root: Option<&std::path::Path>,
    status: Vec<String>,
    tag: Vec<String>,
    depends_on: Option<i64>,
    ready: bool,
) -> Result<()> {
    let root = resolve_project_root(project_root)?;
    let (backend, _) = create_backend(&root)?;

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

    let tasks = backend.list_tasks(&filter)?;

    match output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&tasks)?);
        }
        OutputFormat::Text => {
            for task in &tasks {
                println!(
                    "[{}] #{} {} ({})",
                    task.status, task.id, task.title, task.priority
                );
            }
        }
    }
    Ok(())
}

fn cmd_get(
    output: &OutputFormat,
    project_root: Option<&std::path::Path>,
    task_id: i64,
) -> Result<()> {
    let root = resolve_project_root(project_root)?;
    let (backend, _) = create_backend(&root)?;
    let task = backend.get_task(task_id)?;

    match output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&task)?);
        }
        OutputFormat::Text => {
            println!("ID:       {}", task.id);
            println!("Title:    {}", task.title);
            println!("Status:   {}", task.status);
            println!("Priority: {}", task.priority);
            if let Some(ref bg) = task.background {
                println!("Background: {bg}");
            }
            if let Some(ref desc) = task.description {
                println!("Description: {desc}");
            }
            if let Some(ref p) = task.plan {
                println!("Plan:     {p}");
            }
            if let Some(ref branch) = task.branch {
                println!("Branch:   {branch}");
            }
            if let Some(ref pr_url) = task.pr_url {
                println!("PR URL:   {pr_url}");
            }
            if let Some(ref assignee) = task.assignee_session_id {
                println!("Assignee: {assignee}");
            }
            if !task.tags.is_empty() {
                println!("Tags:     {}", task.tags.join(", "));
            }
            if !task.dependencies.is_empty() {
                let deps: Vec<String> = task.dependencies.iter().map(|d| d.to_string()).collect();
                println!("Deps:     {}", deps.join(", "));
            }
            if let Some(ref meta) = task.metadata {
                println!("Metadata: {}", serde_json::to_string_pretty(meta)?);
            }
            if !task.definition_of_done.is_empty() {
                println!("DoD:");
                for item in &task.definition_of_done {
                    let mark = if item.checked { "x" } else { " " };
                    println!("  [{mark}] {}", item.content);
                }
            }
            if !task.in_scope.is_empty() {
                println!("In scope:");
                for item in &task.in_scope {
                    println!("  - {item}");
                }
            }
            if !task.out_of_scope.is_empty() {
                println!("Out of scope:");
                for item in &task.out_of_scope {
                    println!("  - {item}");
                }
            }
            println!("Created:  {}", task.created_at);
            println!("Updated:  {}", task.updated_at);
            if let Some(ref t) = task.started_at {
                println!("Started:  {t}");
            }
            if let Some(ref t) = task.completed_at {
                println!("Completed: {t}");
            }
            if let Some(ref t) = task.canceled_at {
                println!("Canceled: {t}");
            }
            if let Some(ref reason) = task.cancel_reason {
                println!("Cancel reason: {reason}");
            }
        }
    }
    Ok(())
}

fn cmd_ready(cli: &Cli, id: i64) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let (backend, using_http) = create_backend(&root)?;

    let task = backend.get_task(id)?;

    if cli.dry_run {
        let operations = vec![
            format!("Ready task #{} (status: {} → todo)", id, task.status),
        ];
        return print_dry_run(&cli.output, &DryRunOperation { command: "ready".into(), operations });
    }

    let updated = backend.ready_task(id)?;

    // Fire hooks
    let config = load_config_with_cli(&root, cli)?;
    if should_fire_client_hooks(&config, using_http) {
        hooks::fire_hooks(
            &config, "task_ready", &updated, &*backend,
            Some(TaskStatus::Draft), None,
        );
    }

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&updated)?);
        }
        OutputFormat::Text => {
            println!("Ready task #{}: {}", updated.id, updated.title);
        }
    }

    Ok(())
}

fn cmd_start(cli: &Cli, id: i64, session_id: Option<String>) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let (backend, using_http) = create_backend(&root)?;

    let task = backend.get_task(id)?;

    if cli.dry_run {
        let mut operations = vec![
            format!("Start task #{} (status: {} → in_progress)", id, task.status),
        ];
        if let Some(ref sid) = session_id {
            operations.push(format!("Set assignee_session_id to \"{}\"", sid));
        }
        return print_dry_run(&cli.output, &DryRunOperation { command: "start".into(), operations });
    }

    let prev_status = task.status;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let updated = backend.start_task(id, session_id, &now)?;

    // Fire hooks
    let config = load_config_with_cli(&root, cli)?;
    if should_fire_client_hooks(&config, using_http) {
        hooks::fire_hooks(
            &config, "task_started", &updated, &*backend,
            Some(prev_status), None,
        );
    }

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&updated)?);
        }
        OutputFormat::Text => {
            println!("Started task #{}: {}", updated.id, updated.title);
        }
    }

    Ok(())
}

fn cmd_next(cli: &Cli, session_id: Option<String>) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let (backend, using_http) = create_backend(&root)?;

    let task = backend.next_task()?.ok_or_else(|| anyhow::anyhow!("no eligible task found"))?;

    if cli.dry_run {
        let mut operations = vec![
            format!("Start next eligible task #{}: \"{}\"", task.id, task.title),
            format!("Change status: {} → in_progress", task.status),
        ];
        if let Some(ref sid) = session_id {
            operations.push(format!("Set assignee_session_id to \"{}\"", sid));
        }
        return print_dry_run(&cli.output, &DryRunOperation { command: "next".into(), operations });
    }

    // HttpBackend's next_task() already starts the task (API does next+start atomically),
    // so only call start_task for SqliteBackend where next_task() just selects without starting.
    let prev_status = task.status;
    let updated = if using_http {
        task
    } else {
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        backend.start_task(task.id, session_id, &now)?
    };

    // Fire hooks
    let config = load_config_with_cli(&root, cli)?;
    if should_fire_client_hooks(&config, using_http) {
        hooks::fire_hooks(
            &config, "task_started", &updated, &*backend,
            Some(prev_status), None,
        );
    }

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&updated)?);
        }
        OutputFormat::Text => {
            println!("Started task #{}: {}", updated.id, updated.title);
        }
    }

    Ok(())
}

fn cmd_complete(cli: &Cli, id: i64, skip_pr_check: bool) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let (backend, using_http) = create_backend(&root)?;
    let config = load_config_with_cli(&root, cli)?;

    let task = backend.get_task(id)?;
    task.status.transition_to(TaskStatus::Completed)?;

    let unchecked: Vec<_> = task
        .definition_of_done
        .iter()
        .filter(|d| !d.checked)
        .collect();
    if !unchecked.is_empty() {
        bail!(
            "cannot complete task #{}: {} unchecked DoD item(s)",
            id,
            unchecked.len()
        );
    }

    // PR workflow checks
    if !skip_pr_check
        && config.workflow.completion_mode
            == hooks::CompletionMode::PrThenComplete
    {
        let pr_url = task.pr_url.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "cannot complete task #{}: completion_mode is pr_then_complete but no pr_url is set. \
                 Use `localflow edit {} --pr-url <url>` to set it.",
                id, id
            )
        })?;

        verify_pr_status(pr_url, config.workflow.auto_merge)?;
    }

    if cli.dry_run {
        let operations = vec![
            format!("Complete task #{} (status: {} → completed)", id, task.status),
        ];
        return print_dry_run(&cli.output, &DryRunOperation { command: "complete".into(), operations });
    }

    // Capture ready tasks before completion for unblocked detection
    let prev_ready_ids: std::collections::HashSet<i64> =
        backend.list_ready_tasks()?.iter().map(|t| t.id).collect();

    let prev_status = task.status;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let updated = backend.complete_task(id, &now)?;

    // Fire hooks with unblocked tasks
    if should_fire_client_hooks(&config, using_http) {
        let unblocked = hooks::compute_unblocked(&*backend, &prev_ready_ids);
        let unblocked_opt = if unblocked.is_empty() { None } else { Some(unblocked) };
        hooks::fire_hooks(
            &config, "task_completed", &updated, &*backend,
            Some(prev_status), unblocked_opt,
        );
    }

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&updated)?);
        }
        OutputFormat::Text => {
            println!("Completed task #{}: {}", updated.id, updated.title);
        }
    }

    Ok(())
}

fn verify_pr_status(pr_url: &str, auto_merge: bool) -> Result<()> {
    let mut args = vec!["pr", "view", pr_url, "--json", "state"];
    if !auto_merge {
        args[4] = "state,reviewDecision";
    }

    let output = std::process::Command::new("gh")
        .args(&args)
        .output()
        .context(
            "failed to run 'gh' CLI. gh is required for pr_then_complete mode. \
             Install it from https://cli.github.com/",
        )?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("gh pr view failed: {}", stderr.trim());
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("failed to parse gh output")?;

    let state = json["state"].as_str().unwrap_or("");
    if state != "MERGED" {
        bail!(
            "cannot complete task: PR is not merged (current state: {}). \
             Merge the PR first, then run complete again.",
            state
        );
    }

    if !auto_merge {
        let decision = json["reviewDecision"].as_str().unwrap_or("");
        if decision != "APPROVED" {
            bail!(
                "cannot complete task: PR has not been approved (reviewDecision: {}). \
                 Get the PR reviewed and approved, then run complete again.",
                if decision.is_empty() {
                    "none"
                } else {
                    decision
                }
            );
        }
    }

    Ok(())
}

fn cmd_cancel(cli: &Cli, id: i64, reason: Option<String>) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let (backend, using_http) = create_backend(&root)?;

    let task = backend.get_task(id)?;
    task.status.transition_to(TaskStatus::Canceled)?;

    if cli.dry_run {
        let mut operations = vec![
            format!("Cancel task #{} (status: {} → canceled)", id, task.status),
        ];
        if let Some(ref r) = reason {
            operations.push(format!("Set cancel reason: \"{}\"", r));
        }
        return print_dry_run(&cli.output, &DryRunOperation { command: "cancel".into(), operations });
    }

    let prev_status = task.status;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let updated = backend.cancel_task(id, &now, reason)?;

    // Fire hooks
    let config = load_config_with_cli(&root, cli)?;
    if should_fire_client_hooks(&config, using_http) {
        hooks::fire_hooks(
            &config, "task_canceled", &updated, &*backend,
            Some(prev_status), None,
        );
    }

    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&updated)?);
        }
        OutputFormat::Text => {
            println!("Canceled task #{}: {}", updated.id, updated.title);
            if let Some(ref r) = updated.cancel_reason {
                println!("  reason: {r}");
            }
        }
    }

    Ok(())
}

const CONFIG_TEMPLATE: &str = r#"# localflow configuration
# See: https://github.com/hisamekms/localflow

[hooks]
# on_task_added = "echo 'task added'"
# on_task_ready = "echo 'task ready'"
# on_task_started = "echo 'task started'"
# on_task_completed = "echo 'task completed'"
# on_task_canceled = "echo 'task canceled'"

[workflow]
# completion_mode = "merge_then_complete"  # or "pr_then_complete"
# auto_merge = true

[backend]
# api_url = "http://127.0.0.1:3142"  # uncomment to use HTTP backend
# hook_mode = "server"  # "server" (default), "client", or "both"

[log]
# dir = "/custom/path/to/logs"  # override log output directory (default: $XDG_STATE_HOME/localflow)
"#;

fn cmd_hooks(cli: &Cli, command: &HooksCommand) -> Result<()> {
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
            let valid_events = [
                "task_added",
                "task_ready",
                "task_started",
                "task_completed",
                "task_canceled",
            ];
            if !valid_events.contains(&event_name.as_str()) {
                bail!(
                    "unknown event: {event_name}. Valid events: {}",
                    valid_events.join(", ")
                );
            }

            let root = resolve_project_root(cli.project_root.as_deref())?;
            let config = hooks::load_config(&root)?;
            let (backend, _) = create_backend(&root)?;

            // Build the event using a real task or a sample task
            let task = if let Some(id) = task_id {
                backend.get_task(*id)?
            } else {
                use localflow::models::{Priority, TaskStatus};
                Task {
                    id: 0,
                    title: "Sample task".into(),
                    background: None,
                    description: Some("This is a sample task for hook testing".into()),
                    plan: None,
                    priority: Priority::P2,
                    status: TaskStatus::Todo,
                    assignee_session_id: None,
                    created_at: chrono::Utc::now().to_rfc3339(),
                    updated_at: chrono::Utc::now().to_rfc3339(),
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
                }
            };

            let event = hooks::build_event(event_name, &task, &*backend, None, None);
            let json = serde_json::to_string_pretty(&event)?;

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

            let compact_json = serde_json::to_string(&event)?;
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

fn cmd_config(cli: &Cli, init: bool) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;

    if init {
        let localflow_dir = root.join(".localflow");
        fs::create_dir_all(&localflow_dir)?;
        let config_path = localflow_dir.join("config.toml");
        if config_path.exists() {
            bail!(".localflow/config.toml already exists. Remove it first to re-initialize.");
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

    let config = hooks::load_config(&root)?;
    match cli.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&config)?);
        }
        OutputFormat::Text => {
            println!("Configuration (.localflow/config.toml):");
            println!("  [workflow]");
            println!(
                "    completion_mode: {}",
                config.workflow.completion_mode
            );
            println!("    auto_merge: {}", config.workflow.auto_merge);
            println!("  [hooks]");
            if config.hooks.on_task_added.is_empty() {
                println!("    on_task_added: (none)");
            } else {
                println!(
                    "    on_task_added: {}",
                    config.hooks.on_task_added.join(", ")
                );
            }
            if config.hooks.on_task_ready.is_empty() {
                println!("    on_task_ready: (none)");
            } else {
                println!(
                    "    on_task_ready: {}",
                    config.hooks.on_task_ready.join(", ")
                );
            }
            if config.hooks.on_task_started.is_empty() {
                println!("    on_task_started: (none)");
            } else {
                println!(
                    "    on_task_started: {}",
                    config.hooks.on_task_started.join(", ")
                );
            }
            if config.hooks.on_task_completed.is_empty() {
                println!("    on_task_completed: (none)");
            } else {
                println!(
                    "    on_task_completed: {}",
                    config.hooks.on_task_completed.join(", ")
                );
            }
            if config.hooks.on_task_canceled.is_empty() {
                println!("    on_task_canceled: (none)");
            } else {
                println!(
                    "    on_task_canceled: {}",
                    config.hooks.on_task_canceled.join(", ")
                );
            }
            println!("  [backend]");
            match config.backend.api_url {
                Some(ref url) => println!("    api_url: {url}"),
                None => println!("    api_url: (none, using SQLite)"),
            }
            println!("    hook_mode: {:?}", config.backend.hook_mode);
        }
    }

    Ok(())
}

fn cmd_dod(cli: &Cli, command: &DodCommand) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let (backend, _) = create_backend(&root)?;

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
            let task = backend.check_dod(task_id, index)?;
            match cli.output {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&task)?),
                OutputFormat::Text => {
                    println!("Checked DoD item #{index} of task #{task_id}");
                    print_dod_items(&task.definition_of_done);
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
            let task = backend.uncheck_dod(task_id, index)?;
            match cli.output {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&task)?),
                OutputFormat::Text => {
                    println!("Unchecked DoD item #{index} of task #{task_id}");
                    print_dod_items(&task.definition_of_done);
                }
            }
        }
    }
    Ok(())
}

fn print_dod_items(items: &[localflow::models::DodItem]) {
    for item in items {
        let mark = if item.checked { "x" } else { " " };
        println!("  [{mark}] {}", item.content);
    }
}

fn cmd_deps(cli: &Cli, command: &DepsCommand) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let (backend, _) = create_backend(&root)?;

    match command {
        DepsCommand::Add { task_id, on } => {
            let (task_id, on) = (*task_id, *on);
            if cli.dry_run {
                let operations = vec![format!("Add dependency: task #{} depends on #{}", task_id, on)];
                return print_dry_run(&cli.output, &DryRunOperation { command: "deps add".into(), operations });
            }
            let task = backend.add_dependency(task_id, on)?;
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
            let task = backend.remove_dependency(task_id, on)?;
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
            let task = backend.set_dependencies(task_id, on)?;
            match cli.output {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&task)?),
                OutputFormat::Text => {
                    if task.dependencies.is_empty() {
                        println!("Cleared all dependencies for task #{}", task_id);
                    } else {
                        let dep_strs: Vec<String> = task.dependencies.iter().map(|d| format!("#{d}")).collect();
                        println!("Set dependencies for task #{}: {}", task_id, dep_strs.join(", "));
                    }
                }
            }
        }
        DepsCommand::List { task_id } => {
            // Read-only: ignore --dry-run
            let deps = backend.list_dependencies(*task_id)?;
            match cli.output {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&deps)?),
                OutputFormat::Text => {
                    for task in &deps {
                        println!("[{}] #{} {} ({})", task.status, task.id, task.title, task.priority);
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
    /// Path segments under `.claude/` (e.g. `["skills", "localflow", "SKILL.md"]`)
    segments: &'static [&'static str],
    content: &'static str,
}

const INSTALLABLE_FILES: &[InstallableFile] = &[
    InstallableFile {
        segments: &["skills", "localflow", "SKILL.md"],
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

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn parse_add_subcommand() {
        let cli = Cli::parse_from(["localflow", "add"]);
        assert!(matches!(cli.command, Command::Add { .. }));
    }

    #[test]
    fn parse_add_with_title() {
        let cli = Cli::parse_from(["localflow", "add", "--title", "my task"]);
        match cli.command {
            Command::Add { title, .. } => assert_eq!(title, Some("my task".to_string())),
            _ => panic!("expected Add"),
        }
    }

    #[test]
    fn parse_add_with_all_flags() {
        let cli = Cli::parse_from([
            "localflow",
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
        let cli = Cli::parse_from(["localflow", "add", "--from-json"]);
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
        let cli = Cli::parse_from(["localflow", "add", "--from-json-file", "/tmp/task.json"]);
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

    #[test]
    fn cmd_add_with_flags() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli {
            output: OutputFormat::Text,
            project_root: Some(tmp.path().to_path_buf()),
            dry_run: false,
            log_dir: None,
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
        .unwrap();

        let backend = db::SqliteBackend::new(tmp.path()).unwrap();
        let task = backend.get_task(1).unwrap();
        assert_eq!(task.title, "test task");
        assert_eq!(task.background.as_deref(), Some("bg"));
        assert_eq!(task.priority, localflow::models::Priority::P1);
        assert_eq!(
            task.definition_of_done,
            vec![localflow::models::DodItem { content: "done".to_string(), checked: false }]
        );
        assert_eq!(task.tags, vec!["rust"]);
    }

    #[test]
    fn cmd_add_with_from_json_file() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("task.json");
        std::fs::write(&json_path, r#"{"title":"file task","priority":"P0"}"#).unwrap();

        let cli = Cli {
            output: OutputFormat::Text,
            project_root: Some(tmp.path().to_path_buf()),
            dry_run: false,
            log_dir: None,
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
        .unwrap();

        let backend = db::SqliteBackend::new(tmp.path()).unwrap();
        let task = backend.get_task(1).unwrap();
        assert_eq!(task.title, "file task");
        assert_eq!(task.priority, localflow::models::Priority::P0);
    }

    #[test]
    fn cmd_add_missing_title_error() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli {
            output: OutputFormat::Text,
            project_root: Some(tmp.path().to_path_buf()),
            dry_run: false,
            log_dir: None,
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
        );
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("--title is required"));
    }

    #[test]
    fn cmd_add_text_output() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli {
            output: OutputFormat::Text,
            project_root: Some(tmp.path().to_path_buf()),
            dry_run: false,
            log_dir: None,
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
        .unwrap();
        let backend = db::SqliteBackend::new(tmp.path()).unwrap();
        let task = backend.get_task(1).unwrap();
        assert_eq!(task.title, "my task");
    }

    #[test]
    fn cmd_add_json_output() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli {
            output: OutputFormat::Json,
            project_root: Some(tmp.path().to_path_buf()),
            dry_run: false,
            log_dir: None,
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
        .unwrap();
        let backend = db::SqliteBackend::new(tmp.path()).unwrap();
        let task = backend.get_task(1).unwrap();
        assert_eq!(task.title, "json out");
    }

    #[test]
    fn parse_list_subcommand() {
        let cli = Cli::parse_from(["localflow", "list"]);
        assert!(matches!(cli.command, Command::List { .. }));
    }

    #[test]
    fn parse_list_with_filters() {
        let cli = Cli::parse_from([
            "localflow", "list", "--status", "todo", "--status", "in_progress",
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
        let cli = Cli::parse_from(["localflow", "get", "42"]);
        match cli.command {
            Command::Get { task_id } => assert_eq!(task_id, 42),
            _ => panic!("expected Get"),
        }
    }

    #[test]
    fn parse_next_subcommand() {
        let cli = Cli::parse_from(["localflow", "next"]);
        assert!(matches!(cli.command, Command::Next { .. }));
    }

    #[test]
    fn parse_next_with_session_id() {
        let cli = Cli::parse_from(["localflow", "next", "--session-id", "abc-123"]);
        match cli.command {
            Command::Next { session_id } => {
                assert_eq!(session_id, Some("abc-123".to_string()));
            }
            _ => panic!("expected Next"),
        }
    }

    #[test]
    fn parse_edit_subcommand() {
        let cli = Cli::parse_from(["localflow", "edit", "1"]);
        assert!(matches!(cli.command, Command::Edit { id: 1, .. }));
    }

    #[test]
    fn parse_edit_with_scalar_args() {
        let cli = Cli::parse_from([
            "localflow", "edit", "5", "--title", "new title", "--priority", "p0",
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
        let cli = Cli::parse_from(["localflow", "ready", "3"]);
        match cli.command {
            Command::Ready { id } => assert_eq!(id, 3),
            _ => panic!("expected Ready"),
        }
    }

    #[test]
    fn parse_start_command() {
        let cli = Cli::parse_from(["localflow", "start", "5", "--session-id", "abc"]);
        match cli.command {
            Command::Start { id, session_id } => {
                assert_eq!(id, 5);
                assert_eq!(session_id.as_deref(), Some("abc"));
            }
            _ => panic!("expected Start"),
        }
    }

    #[test]
    fn parse_edit_with_array_args() {
        let cli = Cli::parse_from([
            "localflow",
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
        let cli = Cli::parse_from(["localflow", "edit", "1", "--clear-background"]);
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
        let cli = Cli::parse_from(["localflow", "complete", "1"]);
        assert!(matches!(cli.command, Command::Complete { id: 1, .. }));
    }

    #[test]
    fn parse_cancel_subcommand() {
        let cli = Cli::parse_from(["localflow", "cancel", "2"]);
        assert!(matches!(cli.command, Command::Cancel { id: 2, .. }));
    }

    #[test]
    fn parse_cancel_with_reason() {
        let cli = Cli::parse_from(["localflow", "cancel", "3", "--reason", "no longer needed"]);
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
        let cli = Cli::parse_from(["localflow", "cancel", "4"]);
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
        let cli = Cli::parse_from(["localflow", "deps", "add", "1", "--on", "2"]);
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
        let cli = Cli::parse_from(["localflow", "deps", "remove", "3", "--on", "4"]);
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
        let cli = Cli::parse_from(["localflow", "deps", "set", "1", "--on", "2", "3", "4"]);
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
        let cli = Cli::parse_from(["localflow", "deps", "list", "5"]);
        match cli.command {
            Command::Deps { command: DepsCommand::List { task_id } } => {
                assert_eq!(task_id, 5);
            }
            _ => panic!("expected Deps List"),
        }
    }

    #[test]
    fn parse_skill_install_subcommand() {
        let cli = Cli::parse_from(["localflow", "skill-install"]);
        assert!(matches!(cli.command, Command::SkillInstall { .. }));
    }

    #[test]
    fn parse_skill_install_with_output_dir() {
        let cli = Cli::parse_from(["localflow", "skill-install", "--output-dir", "/tmp/out"]);
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
        let cli = Cli::parse_from(["localflow", "skill-install"]);
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
        let cli = Cli::parse_from(["localflow", "skill-install", "--yes"]);
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
            dry_run: false,
            log_dir: None,
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
            dry_run: false,
            log_dir: None,
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
            .join("localflow")
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
            dry_run: false,
            log_dir: None,
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
            .join("localflow")
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
            dry_run: false,
            log_dir: None,
            command: Command::SkillInstall {
                output_dir: None,
                yes: true,
            },
        };
        skill_install(&cli, None, true).unwrap();

        assert!(tmp.path().join(".claude").exists());
        assert!(tmp
            .path()
            .join(".claude/skills/localflow/SKILL.md")
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
            dry_run: false,
            log_dir: None,
            command: Command::SkillInstall {
                output_dir: None,
                yes: true,
            },
        };
        // First install
        skill_install(&cli, None, true).unwrap();
        // Second install should succeed (files are up to date)
        skill_install(&cli, None, true).unwrap();

        let skill_path = tmp.path().join(".claude/skills/localflow/SKILL.md");
        let content = std::fs::read_to_string(&skill_path).unwrap();
        assert_eq!(content, SKILL_MD_CONTENT);
    }

    #[test]
    fn skill_install_overwrites_with_yes() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli {
            output: OutputFormat::Text,
            project_root: Some(tmp.path().to_path_buf()),
            dry_run: false,
            log_dir: None,
            command: Command::SkillInstall {
                output_dir: None,
                yes: true,
            },
        };
        // First install
        skill_install(&cli, None, true).unwrap();
        // Tamper with the file
        let skill_path = tmp.path().join(".claude/skills/localflow/SKILL.md");
        std::fs::write(&skill_path, "modified content").unwrap();
        // Reinstall with --yes should overwrite
        skill_install(&cli, None, true).unwrap();
        let content = std::fs::read_to_string(&skill_path).unwrap();
        assert_eq!(content, SKILL_MD_CONTENT);
    }

    #[test]
    fn skill_md_covers_all_commands() {
        let commands = [
            "localflow add",
            "localflow list",
            "localflow get",
            "localflow next",
            "localflow ready",
            "localflow start",
            "localflow edit",
            "localflow complete",
            "localflow cancel",
            "localflow deps add",
            "localflow deps remove",
            "localflow deps set",
            "localflow deps list",
            "localflow dod check",
            "localflow dod uncheck",
            "localflow config",
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
        let cli = Cli::parse_from(["localflow", "--output", "json", "add"]);
        assert!(matches!(cli.output, OutputFormat::Json));
    }

    #[test]
    fn parse_output_json_default() {
        let cli = Cli::parse_from(["localflow", "add"]);
        assert!(matches!(cli.output, OutputFormat::Json));
    }

    #[test]
    fn parse_project_root() {
        let cli = Cli::parse_from(["localflow", "--project-root", "/tmp/test", "add"]);
        assert_eq!(cli.project_root, Some(PathBuf::from("/tmp/test")));
    }

    #[test]
    fn parse_no_project_root() {
        let cli = Cli::parse_from(["localflow", "add"]);
        assert!(cli.project_root.is_none());
    }
}
