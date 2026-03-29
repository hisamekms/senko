pub mod handlers;
pub mod skill;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

use crate::bootstrap::create_backend;
use crate::domain::config::CliOverrides;
use crate::infra::hook as hooks;
use crate::domain::task::Priority;
use crate::domain::user::Role;
use crate::infra::project_root::resolve_project_root;

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputFormat {
    Json,
    Text,
}

#[derive(Debug, Parser)]
#[command(name = "senko", about = "Local task management CLI", version)]
pub struct Cli {
    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    pub output: OutputFormat,

    /// Project root directory
    #[arg(long)]
    pub project_root: Option<PathBuf>,

    /// Path to config file (default: .senko/config.toml)
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Dry run mode: show what would be done without executing
    #[arg(long)]
    pub dry_run: bool,

    /// Override log output directory
    #[arg(long)]
    pub log_dir: Option<PathBuf>,

    /// Path to SQLite database file (env: SENKO_DB_PATH)
    #[arg(long)]
    pub db_path: Option<PathBuf>,

    /// PostgreSQL connection URL (env: SENKO_POSTGRES_URL)
    #[arg(long)]
    pub postgres_url: Option<String>,

    /// Project name to operate on (overrides config; env: SENKO_PROJECT)
    #[arg(long)]
    pub project: Option<String>,

    /// User name to operate as (overrides config; env: SENKO_USER)
    #[arg(long)]
    pub user: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, serde::Serialize)]
pub struct DryRunOperation {
    pub command: String,
    pub operations: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
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
pub enum HooksCommand {
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
pub enum DepsCommand {
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
pub enum DodCommand {
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

#[derive(Debug, Subcommand)]
pub enum ProjectAction {
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
pub enum UserAction {
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
pub enum MemberAction {
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

pub const CONFIG_TEMPLATE: &str = r#"# senko configuration
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

[storage]
# db_path = "/custom/path/to/data.db"  # override SQLite database path (default: $XDG_DATA_HOME/senko/data.db)

[log]
# dir = "/custom/path/to/logs"  # override log output directory (default: $XDG_STATE_HOME/senko)

[project]
# name = "default"  # project name to operate on (overrides with --project flag or SENKO_PROJECT env)

[user]
# name = "default"  # user name to operate as (overrides with --user flag or SENKO_USER env)

[web]
# host = "127.0.0.1"  # bind address for `senko web` / `senko serve` (default: 127.0.0.1, env: SENKO_HOST)
"#;

pub fn print_dry_run(output: &OutputFormat, ops: &DryRunOperation) -> Result<()> {
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

pub async fn run(cli: Cli) -> Result<()> {
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
        } => handlers::cmd_add(
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
        } => handlers::cmd_list(&cli, status.clone(), tag.clone(), *depends_on, ready).await,
        Command::Get { task_id } => handlers::cmd_get(&cli, task_id).await,
        Command::Next { ref session_id, user_id } => handlers::cmd_next(&cli, session_id.clone(), user_id).await,
        Command::Ready { id } => handlers::cmd_ready(&cli, id).await,
        Command::Start { id, ref session_id, user_id } => handlers::cmd_start(&cli, id, session_id.clone(), user_id).await,
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
        } => handlers::cmd_edit(
            &cli,
            id,
            title,
            background,
            clear_background,
            description,
            clear_description,
            plan,
            plan_file,
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
        ).await,
        Command::Complete { id, skip_pr_check } => handlers::cmd_complete(&cli, id, skip_pr_check).await,
        Command::Cancel { id, ref reason } => handlers::cmd_cancel(&cli, id, reason.clone()).await,
        Command::Dod { ref command } => handlers::cmd_dod(&cli, command).await,
        Command::Deps { ref command } => handlers::cmd_deps(&cli, command).await,
        Command::Web { port, host } => {
            let root = resolve_project_root(cli.project_root.as_deref())?;
            let mut config = hooks::load_config(&root, cli.config.as_deref())?;
            config.apply_cli(&CliOverrides {
                log_dir: cli.log_dir.as_ref().map(|p| p.to_string_lossy().into_owned()),
                db_path: cli.db_path.as_ref().map(|p| p.to_string_lossy().into_owned()),
                port, host,
                ..Default::default()
            });
            let (backend, _) = create_backend(&root, &config)?;
            let port_is_explicit = config.web_port_is_explicit();
            let effective_port = config.web_port_or(3141);
            crate::presentation::web::serve(root, effective_port, port_is_explicit, &config, backend).await?;
            Ok(())
        }
        Command::Serve { port, host } => {
            let root = resolve_project_root(cli.project_root.as_deref())?;
            let mut config = hooks::load_config(&root, cli.config.as_deref())?;
            config.apply_cli(&CliOverrides {
                log_dir: cli.log_dir.as_ref().map(|p| p.to_string_lossy().into_owned()),
                db_path: cli.db_path.as_ref().map(|p| p.to_string_lossy().into_owned()),
                port, host,
                ..Default::default()
            });
            let (backend, _) = create_backend(&root, &config)?;
            let port_is_explicit = config.web_port_is_explicit();
            let effective_port = config.web_port_or(3142);
            crate::presentation::api::serve(root, effective_port, port_is_explicit, &config, cli.config.clone(), backend).await?;
            Ok(())
        }
        Command::SkillInstall { ref output_dir, yes } => {
            skill::skill_install(&cli, output_dir.clone(), yes)
        }
        Command::Hooks { ref command } => handlers::cmd_hooks(&cli, command).await,
        Command::Doctor => handlers::cmd_doctor(&cli),
        Command::Config { init } => handlers::cmd_config(&cli, init),
        Command::Project { ref action } => handlers::cmd_project(&cli, action).await,
        Command::User { ref action } => handlers::cmd_user(&cli, action).await,
        Command::Members { ref action } => handlers::cmd_members(&cli, action).await,
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

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
    fn parse_edit_with_plan_file() {
        let cli = Cli::parse_from(["senko", "edit", "1", "--plan-file", "/tmp/plan.md"]);
        match cli.command {
            Command::Edit { plan, plan_file, .. } => {
                assert!(plan.is_none());
                assert_eq!(plan_file, Some(PathBuf::from("/tmp/plan.md")));
            }
            _ => panic!("expected Edit"),
        }
    }

    #[test]
    fn parse_edit_plan_file_conflicts_with_plan() {
        let result = Cli::try_parse_from([
            "senko", "edit", "1", "--plan", "inline", "--plan-file", "/tmp/plan.md",
        ]);
        assert!(result.is_err());
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
