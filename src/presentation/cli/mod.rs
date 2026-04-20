mod auth_cache;
pub mod handlers;
pub mod keychain;
mod oidc_login;
pub mod skill;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

use crate::domain::task::Priority;
use crate::domain::user::Role;
use crate::infra::config::CliOverrides;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliPriority {
    P0,
    P1,
    P2,
    P3,
}

impl From<CliPriority> for Priority {
    fn from(p: CliPriority) -> Self {
        match p {
            CliPriority::P0 => Priority::P0,
            CliPriority::P1 => Priority::P1,
            CliPriority::P2 => Priority::P2,
            CliPriority::P3 => Priority::P3,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliRole {
    Owner,
    Member,
    Viewer,
}

impl From<CliRole> for Role {
    fn from(r: CliRole) -> Self {
        match r {
            CliRole::Owner => Role::Owner,
            CliRole::Member => Role::Member,
            CliRole::Viewer => Role::Viewer,
        }
    }
}
use crate::bootstrap::resolve_project_root;

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
#[allow(clippy::large_enum_variant)]
pub enum Command {
    /// Manage tasks
    Task {
        #[command(subcommand)]
        action: TaskAction,
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
        /// Clean install: remove existing install directories before installing
        #[arg(long)]
        force: bool,
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
    /// Authentication commands
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    /// Manage contracts
    Contract {
        #[command(subcommand)]
        action: ContractAction,
    },
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum TaskAction {
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
        /// Assign to user ("self" for current user, or numeric user ID)
        #[arg(long)]
        assignee_user_id: Option<String>,
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
        /// Include unassigned tasks (with --ready)
        #[arg(long)]
        include_unassigned: bool,
        /// Filter by metadata key=value pair; repeatable
        #[arg(long)]
        metadata: Vec<String>,
        /// Filter by contract ID
        #[arg(long)]
        contract: Option<i64>,
        /// Minimum task ID (inclusive)
        #[arg(long)]
        id_min: Option<i64>,
        /// Maximum task ID (inclusive)
        #[arg(long)]
        id_max: Option<i64>,
        /// Maximum number of results (default 50, range 1..=200)
        #[arg(long)]
        limit: Option<u32>,
        /// Skip N results (default 0)
        #[arg(long)]
        offset: Option<u32>,
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
        /// JSON string to set as task metadata
        #[arg(long)]
        metadata: Option<String>,
        /// Include unassigned tasks
        #[arg(long)]
        include_unassigned: bool,
    },
    /// Transition a task from draft to todo
    Publish {
        /// Task ID
        id: i64,
    },
    /// Transition a task from todo to in_progress
    Start {
        /// Task ID
        id: i64,
        #[arg(long)]
        session_id: Option<String>,
        /// JSON string to set as task metadata
        #[arg(long)]
        metadata: Option<String>,
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
        priority: Option<CliPriority>,
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
        /// Link this task to a contract
        #[arg(long, conflicts_with = "clear_contract")]
        contract: Option<i64>,
        /// Remove contract link from this task
        #[arg(long)]
        clear_contract: bool,
        /// JSON metadata (shallow-merged into existing metadata)
        #[arg(long, conflicts_with = "replace_metadata")]
        metadata: Option<String>,
        /// Replace entire metadata with JSON value
        #[arg(long, conflicts_with = "metadata")]
        replace_metadata: Option<String>,
        #[arg(long, conflicts_with_all = ["metadata", "replace_metadata"])]
        clear_metadata: bool,
        /// Assign to user ("self" for current user, or numeric user ID)
        #[arg(long)]
        assignee_user_id: Option<String>,
        /// Remove assignee
        #[arg(long)]
        clear_assignee_user_id: bool,
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
        command: TaskDodCommand,
    },
    /// Manage task dependencies
    Deps {
        #[command(subcommand)]
        command: TaskDepsCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum ContractAction {
    /// Create a new contract
    Add {
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        definition_of_done: Vec<String>,
        #[arg(long)]
        tag: Vec<String>,
        /// Arbitrary JSON metadata
        #[arg(long)]
        metadata: Option<String>,
        /// Read JSON from stdin
        #[arg(long, conflicts_with_all = ["title", "description", "definition_of_done", "tag", "metadata"])]
        from_json: bool,
        /// Read JSON from file
        #[arg(long, conflicts_with_all = ["title", "description", "definition_of_done", "tag", "metadata", "from_json"])]
        from_json_file: Option<PathBuf>,
    },
    /// List contracts
    List {
        /// Filter by tag; repeatable
        #[arg(long)]
        tag: Vec<String>,
    },
    /// Get contract details
    Get {
        /// Contract ID
        id: i64,
    },
    /// Edit a contract
    Edit {
        /// Contract ID
        id: i64,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        clear_description: bool,
        /// JSON metadata (shallow-merged into existing metadata)
        #[arg(long, conflicts_with = "replace_metadata")]
        metadata: Option<String>,
        /// Replace entire metadata with JSON value
        #[arg(long, conflicts_with = "metadata")]
        replace_metadata: Option<String>,
        #[arg(long, conflicts_with_all = ["metadata", "replace_metadata"])]
        clear_metadata: bool,
        #[arg(long, num_args = 0..)]
        set_tags: Option<Vec<String>>,
        #[arg(long, num_args = 0..)]
        set_definition_of_done: Option<Vec<String>>,
        #[arg(long)]
        add_tag: Vec<String>,
        #[arg(long)]
        add_definition_of_done: Vec<String>,
        #[arg(long)]
        remove_tag: Vec<String>,
        #[arg(long)]
        remove_definition_of_done: Vec<String>,
    },
    /// Delete a contract
    Delete {
        /// Contract ID
        id: i64,
    },
    /// Manage Definition of Done items
    Dod {
        #[command(subcommand)]
        command: ContractDodCommand,
    },
    /// Manage contract notes
    Note {
        #[command(subcommand)]
        command: ContractNoteCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum ContractDodCommand {
    /// Mark a DoD item as checked
    Check {
        /// Contract ID
        contract_id: i64,
        /// DoD item index (1-based)
        index: usize,
    },
    /// Unmark a DoD item
    Uncheck {
        /// Contract ID
        contract_id: i64,
        /// DoD item index (1-based)
        index: usize,
    },
}

#[derive(Debug, Subcommand)]
pub enum ContractNoteCommand {
    /// Add a note to a contract
    Add {
        /// Contract ID
        contract_id: i64,
        /// Note content
        #[arg(long)]
        content: String,
        /// Optional task ID that produced this note
        #[arg(long)]
        source_task: Option<i64>,
    },
    /// List notes on a contract
    List {
        /// Contract ID
        contract_id: i64,
    },
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Login via OIDC (OAuth PKCE flow)
    Login {
        /// Device name for the session token
        #[arg(long)]
        device_name: Option<String>,
    },
    /// Print the stored API token to stdout
    Token,
    /// Show login status and current session info
    Status,
    /// Logout: revoke current session and remove token from keychain
    Logout,
    /// List active sessions
    Sessions,
    /// Revoke a session
    Revoke {
        /// Session ID to revoke
        #[arg(conflicts_with = "all")]
        id: Option<i64>,
        /// Revoke all sessions
        #[arg(long, conflicts_with = "id")]
        all: bool,
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
        /// Event name (task_add, task_publish, task_start, task_complete, task_cancel, task_select, contract_add, contract_edit, contract_delete, contract_dod_check, contract_dod_uncheck, contract_note_add)
        event_name: String,
        /// Task ID to use for building the event (uses a sample task if omitted)
        task_id: Option<i64>,
        /// Show event JSON without executing hooks
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum TaskDepsCommand {
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
pub enum TaskDodCommand {
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
    /// Manage metadata fields
    #[command(name = "metadata-field")]
    MetadataField {
        #[command(subcommand)]
        action: MetadataFieldAction,
    },
    /// Manage project members
    Members {
        #[command(subcommand)]
        action: MemberAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum MetadataFieldAction {
    /// Add a metadata field to the project
    Add {
        /// Field name (lowercase letters, digits, underscores, hyphens)
        #[arg(long)]
        name: String,
        /// Field type (string, number, boolean)
        #[arg(long = "type")]
        field_type: String,
        /// Whether field must be filled when completing a task
        #[arg(long)]
        required_on_complete: bool,
        /// Human-readable description
        #[arg(long)]
        description: Option<String>,
    },
    /// List all metadata fields in the project
    List,
    /// Remove a metadata field from the project
    Remove {
        /// Field name to remove
        #[arg(long)]
        name: String,
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
        sub: Option<String>,
        #[arg(long)]
        display_name: Option<String>,
        #[arg(long)]
        email: Option<String>,
    },
    /// Update a user
    Update {
        /// User ID
        id: i64,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        display_name: Option<String>,
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
        role: Option<CliRole>,
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
        role: CliRole,
    },
}

pub const CONFIG_TEMPLATE: &str = r#"# senko configuration
# See: https://github.com/hisamekms/senko
#
# Config layering (priority high → low):
#   1. CLI flag (--config)
#   2. SENKO_CONFIG env var
#   3. Local config (.senko/config.local.toml) — git-ignored, per-user overrides
#   4. Project config (.senko/config.toml)
#   5. User config (~/.config/senko/config.toml)

[project]
# name = "default"  # project name to operate on (overrides with --project flag or SENKO_PROJECT env)

[user]
# name = "default"  # user name to operate as (overrides with --user flag or SENKO_USER env)

[backend.sqlite]
# db_path = "/custom/path/to/data.db"  # override SQLite database path

# --- CLI runtime ---
# Hooks defined here fire when the binary runs as a local CLI
# (i.e. not `senko serve` / `senko serve --proxy`).
#
# Hook shape (common to every runtime):
#   command        = "..."                            # shell command, receives envelope via stdin
#   when           = "pre" | "post"                   # default: post
#   mode           = "sync" | "async"                 # default: async
#   on_failure     = "abort" | "warn" | "ignore"      # default: abort (abort only applies to sync+pre)
#   enabled        = true | false                     # default: true
#   on_result      = "selected" | "none" | "any"      # task_select only; default: any
#   env_vars       = [ { name = "...", required = true, default = "...", description = "..." } ]

[cli]
# browser = true

[cli.remote]
# url = "http://127.0.0.1:3142"
# token = "your-api-token"

# Task action hooks: task_add / task_publish / task_start / task_complete / task_cancel / task_select
# Contract action hooks: contract_add / contract_edit / contract_delete / contract_dod_check /
#                        contract_dod_uncheck / contract_note_add
#
# [cli.task_add.hooks.example]
# command = "echo task-added"
#
# [cli.task_complete.hooks.notify]
# command = "curl -X POST $WEBHOOK_URL"
# mode = "async"
# [[cli.task_complete.hooks.notify.env_vars]]
# name = "WEBHOOK_URL"
# required = true
#
# [cli.task_select.hooks.prompt_for_add]
# command = "echo 'no eligible task — consider adding one'"
# on_result = "none"
#
# [cli.contract_add.hooks.log]
# command = "echo 'contract created'"
#
# [cli.contract_dod_check.hooks.audit]
# command = "jq -r '.event.contract.id' | xargs -I{} logger -t senko 'contract {} dod check'"
# mode = "async"

# --- Server: Relay mode (serve --proxy) ---
[server]
# host = "127.0.0.1"
# port = 3142

[server.auth.api_key]
# master_key = "secret"

[server.auth.oidc]
# issuer_url = "https://accounts.example.com"
# client_id = "senko-cli"
# scopes = ["openid", "profile"]
# callback_ports = ["8400"]

[server.auth.oidc.session]
# ttl = "24h"
# inactive_ttl = "7d"
# max_per_user = 10

[server.relay]
# url = "http://upstream:3142"
# token = "relay-api-token"

# [server.relay.task_complete.hooks.audit]
# command = "logger -t senko 'task complete'"

# --- Server: Remote/Direct mode (serve) ---
[server.remote]

# [server.remote.task_publish.hooks.metrics]
# command = "emit-metric task_publish"
# mode = "async"
#
# [server.remote.contract_dod_check.hooks.audit]
# command = "emit-metric contract_dod_check"
# mode = "async"

# --- Workflow stages ---
# Built-in stages consumed by the Claude Code skill:
#   task_add / task_publish / task_start / task_complete / task_cancel / task_select
#   branch_set / branch_cleanup / branch_merge / pr_create / pr_update
#   plan / implement
# User-defined stages are allowed — unknown stage names are passed through
# to scripts that query `senko config` output.

[workflow]
# merge_via = "direct"        # or "pr"
# auto_merge = true
# branch_mode = "worktree"    # or "branch"
# merge_strategy = "rebase"   # or "squash"
# branch_template = "senko/{{id}}-{{slug}}"

# [workflow.task_add]
# default_dod = ["Write unit tests", "Update documentation"]
# default_tags = ["backend"]
# default_priority = "p2"
# instructions = ["Include acceptance criteria in the description"]
#
# [workflow.task_add.hooks.example]
# command = "echo 'adding task'"
# when = "pre"
# mode = "sync"
#
# [[workflow.task_add.metadata_fields]]
# key = "team"
# source = "value"
# value = "backend"

# [workflow.task_start]
# instructions = ["Check for blockers before starting"]
#
# [workflow.task_start.hooks.pre_check]
# command = "cargo check"
# when = "pre"
# mode = "sync"
# on_failure = "abort"

# [workflow.branch_set]
# instructions = ["Use feature/ prefix for new features"]

# [workflow.branch_merge]
# instructions = ["Ensure CI is green before merging"]
#
# [workflow.branch_merge.hooks.mise_check]
# command = "mise check"
# when = "pre"
# mode = "sync"
# on_failure = "abort"

# [workflow.pr_create]
# instructions = ["Include screenshots for UI changes"]

# [workflow.plan]
# required_sections = ["Overview", "Acceptance Criteria"]
# instructions = ["Include time estimates in the plan"]

# [workflow.implement]
# instructions = ["Follow project coding standards"]

# [workflow.task_complete]
# instructions = ["Update changelog"]
#
# [workflow.task_complete.hooks.notify]
# command = "echo 'task completed'"
# mode = "async"

# [workflow.contract_note_add]
# instructions = ["Re-read recent notes on this contract before adding a new one"]
#
# [workflow.contract_note_add.hooks.review_before_note]
# command = "true"
# prompt = "Skip the note if the same observation already exists in earlier notes."
# when = "pre"

# [workflow.branch_cleanup]
# instructions = ["Verify branch is fully merged"]

[log]
# dir = "/custom/path/to/logs"

[web]
# host = "127.0.0.1"
# port = 8080
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
        Command::Task { ref action } => match action {
            TaskAction::Add {
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
                metadata,
                from_json,
                from_json_file,
                assignee_user_id,
            } => {
                handlers::cmd_add(
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
                    *from_json,
                    from_json_file.clone(),
                    assignee_user_id.clone(),
                )
                .await
            }
            TaskAction::List {
                status,
                tag,
                depends_on,
                ready,
                include_unassigned,
                metadata,
                contract,
                id_min,
                id_max,
                limit,
                offset,
            } => {
                handlers::cmd_list(
                    &cli,
                    status.clone(),
                    tag.clone(),
                    *depends_on,
                    *ready,
                    *include_unassigned,
                    metadata.clone(),
                    *contract,
                    *id_min,
                    *id_max,
                    *limit,
                    *offset,
                )
                .await
            }
            TaskAction::Get { task_id } => handlers::cmd_get(&cli, *task_id).await,
            TaskAction::Next {
                session_id,
                metadata,
                include_unassigned,
            } => {
                handlers::cmd_next(
                    &cli,
                    session_id.clone(),
                    metadata.clone(),
                    *include_unassigned,
                )
                .await
            }
            TaskAction::Publish { id } => handlers::cmd_publish(&cli, *id).await,
            TaskAction::Start {
                id,
                session_id,
                metadata,
            } => handlers::cmd_start(&cli, *id, session_id.clone(), metadata.clone()).await,
            TaskAction::Edit {
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
                contract,
                clear_contract,
                metadata,
                replace_metadata,
                clear_metadata,
                assignee_user_id,
                clear_assignee_user_id,
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
                let priority_domain = priority.map(|p| p.into());
                handlers::cmd_edit(
                    &cli,
                    *id,
                    title,
                    background,
                    *clear_background,
                    description,
                    *clear_description,
                    plan,
                    plan_file,
                    *clear_plan,
                    &priority_domain,
                    branch,
                    *clear_branch,
                    pr_url,
                    *clear_pr_url,
                    contract,
                    *clear_contract,
                    metadata,
                    replace_metadata,
                    *clear_metadata,
                    assignee_user_id,
                    *clear_assignee_user_id,
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
                )
                .await
            }
            TaskAction::Complete { id, skip_pr_check } => {
                handlers::cmd_complete(&cli, *id, *skip_pr_check).await
            }
            TaskAction::Cancel { id, reason } => {
                handlers::cmd_cancel(&cli, *id, reason.clone()).await
            }
            TaskAction::Dod { command } => handlers::cmd_dod(&cli, command).await,
            TaskAction::Deps { command } => handlers::cmd_deps(&cli, command).await,
        },
        Command::Web { port, host } => {
            let root = resolve_project_root(cli.project_root.as_deref())?;
            let xdg = crate::infra::xdg::XdgDirs::from_env();
            let mut config = crate::bootstrap::load_config(&root, cli.config.as_deref(), &xdg)?;
            config.apply_cli(&CliOverrides {
                log_dir: cli
                    .log_dir
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned()),
                db_path: cli
                    .db_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned()),
                port,
                host,
                ..Default::default()
            });
            #[cfg(feature = "aws-secrets")]
            config.resolve_secrets().await?;
            let (task_ops, project_ops) = crate::bootstrap::create_task_operations(&root, &config)?;
            let project_id = crate::bootstrap::resolve_project_id(&*project_ops, &config).await?;
            let port_is_explicit = config.web_port_is_explicit();
            let effective_port = config.web_port_or(3141);
            crate::presentation::web::serve(
                root,
                effective_port,
                port_is_explicit,
                &config,
                task_ops,
                project_id,
            )
            .await?;
            Ok(())
        }
        Command::Serve { port, host } => {
            let root = resolve_project_root(cli.project_root.as_deref())?;
            let xdg = crate::infra::xdg::XdgDirs::from_env();
            let mut config = crate::bootstrap::load_config(&root, cli.config.as_deref(), &xdg)?;
            config.apply_cli(&CliOverrides {
                log_dir: cli
                    .log_dir
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned()),
                db_path: cli
                    .db_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned()),
                postgres_url: cli.postgres_url.clone(),
                server_port: port,
                server_host: host,
                ..Default::default()
            });
            #[cfg(feature = "aws-secrets")]
            config.resolve_secrets().await?;
            let is_proxy = config.server.relay.url.is_some();
            if !is_proxy {
                crate::bootstrap::validate_serve_auth(&config)?;
            }
            let port_is_explicit = config.server_port_is_explicit();
            let effective_port = config.server_port_or(3142);
            if is_proxy {
                let hook_data = crate::bootstrap::create_hook_data_from(
                    config.server.relay.url.as_ref().unwrap(),
                    config.server.relay.token.clone(),
                );
                crate::presentation::api::serve_proxy(
                    root,
                    effective_port,
                    port_is_explicit,
                    &config,
                    cli.config.clone(),
                    hook_data,
                )
                .await?;
            } else {
                let backend = crate::bootstrap::create_backend(&root, &config)?;
                let auth_mode = crate::bootstrap::create_auth_mode(&config, backend.clone())?;
                crate::presentation::api::serve(
                    root,
                    effective_port,
                    port_is_explicit,
                    &config,
                    cli.config.clone(),
                    backend,
                    auth_mode,
                )
                .await?;
            }
            Ok(())
        }
        Command::SkillInstall {
            ref output_dir,
            yes,
            force,
        } => skill::skill_install(&cli, output_dir.clone(), yes, force),
        Command::Hooks { ref command } => handlers::cmd_hooks(&cli, command).await,
        Command::Doctor => handlers::cmd_doctor(&cli),
        Command::Config { init } => handlers::cmd_config(&cli, init),
        Command::Project { ref action } => handlers::cmd_project(&cli, action).await,
        Command::User { ref action } => handlers::cmd_user(&cli, action).await,
        Command::Auth { ref command } => match command {
            AuthCommand::Login { device_name } => {
                handlers::cmd_auth_login(&cli, device_name.clone()).await
            }
            AuthCommand::Token => handlers::cmd_auth_token(&cli).await,
            AuthCommand::Status => handlers::cmd_auth_status(&cli).await,
            AuthCommand::Logout => handlers::cmd_auth_logout(&cli).await,
            AuthCommand::Sessions => handlers::cmd_auth_sessions(&cli).await,
            AuthCommand::Revoke { id, all } => handlers::cmd_auth_revoke(&cli, *id, *all).await,
        },
        Command::Contract { ref action } => handlers::cmd_contract(&cli, action).await,
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn parse_add_subcommand() {
        let cli = Cli::parse_from(["senko", "task", "add"]);
        assert!(matches!(
            cli.command,
            Command::Task {
                action: TaskAction::Add { .. }
            }
        ));
    }

    #[test]
    fn parse_add_with_title() {
        let cli = Cli::parse_from(["senko", "task", "add", "--title", "my task"]);
        match cli.command {
            Command::Task {
                action: TaskAction::Add { title, .. },
            } => assert_eq!(title, Some("my task".to_string())),
            _ => panic!("expected Add"),
        }
    }

    #[test]
    fn parse_add_with_all_flags() {
        let cli = Cli::parse_from([
            "senko",
            "task",
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
            Command::Task {
                action:
                    TaskAction::Add {
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
                        assignee_user_id,
                    },
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
                assert!(assignee_user_id.is_none());
            }
            _ => panic!("expected Add"),
        }
    }

    #[test]
    fn parse_add_with_from_json() {
        let cli = Cli::parse_from(["senko", "task", "add", "--from-json"]);
        match cli.command {
            Command::Task {
                action: TaskAction::Add {
                    from_json, title, ..
                },
            } => {
                assert!(from_json);
                assert!(title.is_none());
            }
            _ => panic!("expected Add"),
        }
    }

    #[test]
    fn parse_add_with_from_json_file() {
        let cli = Cli::parse_from(["senko", "task", "add", "--from-json-file", "/tmp/task.json"]);
        match cli.command {
            Command::Task {
                action:
                    TaskAction::Add {
                        from_json_file,
                        from_json,
                        title,
                        ..
                    },
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
        let cli = Cli::parse_from(["senko", "task", "list"]);
        assert!(matches!(
            cli.command,
            Command::Task {
                action: TaskAction::List { .. }
            }
        ));
    }

    #[test]
    fn parse_list_with_filters() {
        let cli = Cli::parse_from([
            "senko",
            "task",
            "list",
            "--status",
            "todo",
            "--status",
            "in_progress",
            "--tag",
            "rust",
            "--tag",
            "web",
            "--depends-on",
            "3",
            "--ready",
        ]);
        match cli.command {
            Command::Task {
                action:
                    TaskAction::List {
                        status,
                        tag,
                        depends_on,
                        ready,
                        include_unassigned,
                        metadata,
                        ..
                    },
            } => {
                assert_eq!(status, vec!["todo", "in_progress"]);
                assert_eq!(tag, vec!["rust", "web"]);
                assert_eq!(depends_on, Some(3));
                assert!(ready);
                assert!(!include_unassigned);
                assert!(metadata.is_empty());
            }
            _ => panic!("expected List"),
        }
    }

    #[test]
    fn parse_get_subcommand() {
        let cli = Cli::parse_from(["senko", "task", "get", "42"]);
        match cli.command {
            Command::Task {
                action: TaskAction::Get { task_id },
            } => assert_eq!(task_id, 42),
            _ => panic!("expected Get"),
        }
    }

    #[test]
    fn parse_next_subcommand() {
        let cli = Cli::parse_from(["senko", "task", "next"]);
        assert!(matches!(
            cli.command,
            Command::Task {
                action: TaskAction::Next { .. }
            }
        ));
    }

    #[test]
    fn parse_next_with_session_id() {
        let cli = Cli::parse_from(["senko", "task", "next", "--session-id", "abc-123"]);
        match cli.command {
            Command::Task {
                action: TaskAction::Next { session_id, .. },
            } => {
                assert_eq!(session_id, Some("abc-123".to_string()));
            }
            _ => panic!("expected Next"),
        }
    }

    #[test]
    fn parse_edit_subcommand() {
        let cli = Cli::parse_from(["senko", "task", "edit", "1"]);
        assert!(matches!(
            cli.command,
            Command::Task {
                action: TaskAction::Edit { id: 1, .. }
            }
        ));
    }

    #[test]
    fn parse_edit_with_scalar_args() {
        let cli = Cli::parse_from([
            "senko",
            "task",
            "edit",
            "5",
            "--title",
            "new title",
            "--priority",
            "p0",
        ]);
        match cli.command {
            Command::Task {
                action:
                    TaskAction::Edit {
                        id,
                        title,
                        priority,
                        ..
                    },
            } => {
                assert_eq!(id, 5);
                assert_eq!(title.as_deref(), Some("new title"));
                assert!(matches!(priority, Some(CliPriority::P0)));
            }
            _ => panic!("expected Edit"),
        }
    }

    #[test]
    fn parse_publish_command() {
        let cli = Cli::parse_from(["senko", "task", "publish", "3"]);
        match cli.command {
            Command::Task {
                action: TaskAction::Publish { id },
            } => assert_eq!(id, 3),
            _ => panic!("expected Publish"),
        }
    }

    #[test]
    fn parse_start_command() {
        let cli = Cli::parse_from(["senko", "task", "start", "5", "--session-id", "abc"]);
        match cli.command {
            Command::Task {
                action: TaskAction::Start { id, session_id, .. },
            } => {
                assert_eq!(id, 5);
                assert_eq!(session_id.as_deref(), Some("abc"));
            }
            _ => panic!("expected Start"),
        }
    }

    #[test]
    fn parse_start_with_metadata() {
        let cli = Cli::parse_from([
            "senko",
            "task",
            "start",
            "5",
            "--metadata",
            r#"{"key":"val"}"#,
        ]);
        match cli.command {
            Command::Task {
                action: TaskAction::Start { id, metadata, .. },
            } => {
                assert_eq!(id, 5);
                assert_eq!(metadata.as_deref(), Some(r#"{"key":"val"}"#));
            }
            _ => panic!("expected Start"),
        }
    }

    #[test]
    fn parse_next_with_metadata() {
        let cli = Cli::parse_from(["senko", "task", "next", "--metadata", r#"{"key":"val"}"#]);
        match cli.command {
            Command::Task {
                action: TaskAction::Next { metadata, .. },
            } => {
                assert_eq!(metadata.as_deref(), Some(r#"{"key":"val"}"#));
            }
            _ => panic!("expected Next"),
        }
    }

    #[test]
    fn parse_edit_with_array_args() {
        let cli = Cli::parse_from([
            "senko",
            "task",
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
            Command::Task {
                action:
                    TaskAction::Edit {
                        id,
                        add_tag,
                        remove_tag,
                        set_in_scope,
                        ..
                    },
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
        let cli = Cli::parse_from(["senko", "task", "edit", "1", "--plan-file", "/tmp/plan.md"]);
        match cli.command {
            Command::Task {
                action: TaskAction::Edit {
                    plan, plan_file, ..
                },
            } => {
                assert!(plan.is_none());
                assert_eq!(plan_file, Some(PathBuf::from("/tmp/plan.md")));
            }
            _ => panic!("expected Edit"),
        }
    }

    #[test]
    fn parse_edit_plan_file_conflicts_with_plan() {
        let result = Cli::try_parse_from([
            "senko",
            "task",
            "edit",
            "1",
            "--plan",
            "inline",
            "--plan-file",
            "/tmp/plan.md",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_edit_clear_background() {
        let cli = Cli::parse_from(["senko", "task", "edit", "1", "--clear-background"]);
        match cli.command {
            Command::Task {
                action:
                    TaskAction::Edit {
                        clear_background, ..
                    },
            } => {
                assert!(clear_background);
            }
            _ => panic!("expected Edit"),
        }
    }

    #[test]
    fn parse_complete_subcommand() {
        let cli = Cli::parse_from(["senko", "task", "complete", "1"]);
        assert!(matches!(
            cli.command,
            Command::Task {
                action: TaskAction::Complete { id: 1, .. }
            }
        ));
    }

    #[test]
    fn parse_cancel_subcommand() {
        let cli = Cli::parse_from(["senko", "task", "cancel", "2"]);
        assert!(matches!(
            cli.command,
            Command::Task {
                action: TaskAction::Cancel { id: 2, .. }
            }
        ));
    }

    #[test]
    fn parse_cancel_with_reason() {
        let cli = Cli::parse_from([
            "senko",
            "task",
            "cancel",
            "3",
            "--reason",
            "no longer needed",
        ]);
        match cli.command {
            Command::Task {
                action: TaskAction::Cancel { id, reason },
            } => {
                assert_eq!(id, 3);
                assert_eq!(reason.as_deref(), Some("no longer needed"));
            }
            _ => panic!("expected Cancel"),
        }
    }

    #[test]
    fn parse_cancel_without_reason() {
        let cli = Cli::parse_from(["senko", "task", "cancel", "4"]);
        match cli.command {
            Command::Task {
                action: TaskAction::Cancel { id, reason },
            } => {
                assert_eq!(id, 4);
                assert!(reason.is_none());
            }
            _ => panic!("expected Cancel"),
        }
    }

    #[test]
    fn parse_deps_add() {
        let cli = Cli::parse_from(["senko", "task", "deps", "add", "1", "--on", "2"]);
        match cli.command {
            Command::Task {
                action:
                    TaskAction::Deps {
                        command: TaskDepsCommand::Add { task_id, on },
                    },
            } => {
                assert_eq!(task_id, 1);
                assert_eq!(on, 2);
            }
            _ => panic!("expected Deps Add"),
        }
    }

    #[test]
    fn parse_deps_remove() {
        let cli = Cli::parse_from(["senko", "task", "deps", "remove", "3", "--on", "4"]);
        match cli.command {
            Command::Task {
                action:
                    TaskAction::Deps {
                        command: TaskDepsCommand::Remove { task_id, on },
                    },
            } => {
                assert_eq!(task_id, 3);
                assert_eq!(on, 4);
            }
            _ => panic!("expected Deps Remove"),
        }
    }

    #[test]
    fn parse_deps_set() {
        let cli = Cli::parse_from(["senko", "task", "deps", "set", "1", "--on", "2", "3", "4"]);
        match cli.command {
            Command::Task {
                action:
                    TaskAction::Deps {
                        command: TaskDepsCommand::Set { task_id, on },
                    },
            } => {
                assert_eq!(task_id, 1);
                assert_eq!(on, vec![2, 3, 4]);
            }
            _ => panic!("expected Deps Set"),
        }
    }

    #[test]
    fn parse_deps_list() {
        let cli = Cli::parse_from(["senko", "task", "deps", "list", "5"]);
        match cli.command {
            Command::Task {
                action:
                    TaskAction::Deps {
                        command: TaskDepsCommand::List { task_id },
                    },
            } => {
                assert_eq!(task_id, 5);
            }
            _ => panic!("expected Deps List"),
        }
    }

    #[test]
    fn parse_dod_check() {
        let cli = Cli::parse_from(["senko", "task", "dod", "check", "7", "2"]);
        match cli.command {
            Command::Task {
                action:
                    TaskAction::Dod {
                        command: TaskDodCommand::Check { task_id, index },
                    },
            } => {
                assert_eq!(task_id, 7);
                assert_eq!(index, 2);
            }
            _ => panic!("expected Dod Check"),
        }
    }

    #[test]
    fn parse_dod_uncheck() {
        let cli = Cli::parse_from(["senko", "task", "dod", "uncheck", "7", "2"]);
        match cli.command {
            Command::Task {
                action:
                    TaskAction::Dod {
                        command: TaskDodCommand::Uncheck { task_id, index },
                    },
            } => {
                assert_eq!(task_id, 7);
                assert_eq!(index, 2);
            }
            _ => panic!("expected Dod Uncheck"),
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
            Command::SkillInstall {
                output_dir,
                yes,
                force,
            } => {
                assert_eq!(output_dir, Some(PathBuf::from("/tmp/out")));
                assert!(!yes);
                assert!(!force);
            }
            _ => panic!("expected SkillInstall"),
        }
    }

    #[test]
    fn parse_skill_install_without_output_dir() {
        let cli = Cli::parse_from(["senko", "skill-install"]);
        match cli.command {
            Command::SkillInstall {
                output_dir,
                yes,
                force,
            } => {
                assert!(output_dir.is_none());
                assert!(!yes);
                assert!(!force);
            }
            _ => panic!("expected SkillInstall"),
        }
    }

    #[test]
    fn parse_skill_install_with_yes() {
        let cli = Cli::parse_from(["senko", "skill-install", "--yes"]);
        match cli.command {
            Command::SkillInstall {
                output_dir,
                yes,
                force,
            } => {
                assert!(output_dir.is_none());
                assert!(yes);
                assert!(!force);
            }
            _ => panic!("expected SkillInstall"),
        }
    }

    #[test]
    fn parse_skill_install_with_force() {
        let cli = Cli::parse_from(["senko", "skill-install", "--force"]);
        match cli.command {
            Command::SkillInstall {
                output_dir,
                yes,
                force,
            } => {
                assert!(output_dir.is_none());
                assert!(!yes);
                assert!(force);
            }
            _ => panic!("expected SkillInstall"),
        }
    }

    #[test]
    fn parse_output_json() {
        let cli = Cli::parse_from(["senko", "--output", "json", "task", "add"]);
        assert!(matches!(cli.output, OutputFormat::Json));
    }

    #[test]
    fn parse_output_json_default() {
        let cli = Cli::parse_from(["senko", "task", "add"]);
        assert!(matches!(cli.output, OutputFormat::Json));
    }

    #[test]
    fn parse_project_root() {
        let cli = Cli::parse_from(["senko", "--project-root", "/tmp/test", "task", "add"]);
        assert_eq!(cli.project_root, Some(PathBuf::from("/tmp/test")));
    }

    #[test]
    fn parse_no_project_root() {
        let cli = Cli::parse_from(["senko", "task", "add"]);
        assert!(cli.project_root.is_none());
    }

    #[test]
    fn parse_metadata_field_add() {
        let cli = Cli::parse_from([
            "senko",
            "project",
            "metadata-field",
            "add",
            "--name",
            "sprint",
            "--type",
            "string",
            "--required-on-complete",
            "--description",
            "Sprint name",
        ]);
        match cli.command {
            Command::Project {
                action:
                    ProjectAction::MetadataField {
                        action:
                            MetadataFieldAction::Add {
                                name,
                                field_type,
                                required_on_complete,
                                description,
                            },
                    },
            } => {
                assert_eq!(name, "sprint");
                assert_eq!(field_type, "string");
                assert!(required_on_complete);
                assert_eq!(description, Some("Sprint name".to_string()));
            }
            _ => panic!("expected Project MetadataField Add"),
        }
    }

    #[test]
    fn parse_metadata_field_add_minimal() {
        let cli = Cli::parse_from([
            "senko",
            "project",
            "metadata-field",
            "add",
            "--name",
            "points",
            "--type",
            "number",
        ]);
        match cli.command {
            Command::Project {
                action:
                    ProjectAction::MetadataField {
                        action:
                            MetadataFieldAction::Add {
                                name,
                                field_type,
                                required_on_complete,
                                description,
                            },
                    },
            } => {
                assert_eq!(name, "points");
                assert_eq!(field_type, "number");
                assert!(!required_on_complete);
                assert!(description.is_none());
            }
            _ => panic!("expected Project MetadataField Add"),
        }
    }

    #[test]
    fn parse_metadata_field_list() {
        let cli = Cli::parse_from(["senko", "project", "metadata-field", "list"]);
        assert!(matches!(
            cli.command,
            Command::Project {
                action: ProjectAction::MetadataField {
                    action: MetadataFieldAction::List
                }
            }
        ));
    }

    #[test]
    fn parse_metadata_field_remove() {
        let cli = Cli::parse_from([
            "senko",
            "project",
            "metadata-field",
            "remove",
            "--name",
            "sprint",
        ]);
        match cli.command {
            Command::Project {
                action:
                    ProjectAction::MetadataField {
                        action: MetadataFieldAction::Remove { name },
                    },
            } => {
                assert_eq!(name, "sprint");
            }
            _ => panic!("expected Project MetadataField Remove"),
        }
    }

    #[test]
    fn parse_add_with_assignee_user_id() {
        let cli = Cli::parse_from([
            "senko",
            "task",
            "add",
            "--title",
            "test",
            "--assignee-user-id",
            "self",
        ]);
        match cli.command {
            Command::Task {
                action: TaskAction::Add {
                    assignee_user_id, ..
                },
            } => {
                assert_eq!(assignee_user_id, Some("self".to_string()));
            }
            _ => panic!("expected Add"),
        }
    }

    #[test]
    fn parse_edit_with_assignee_user_id() {
        let cli = Cli::parse_from(["senko", "task", "edit", "1", "--assignee-user-id", "42"]);
        match cli.command {
            Command::Task {
                action:
                    TaskAction::Edit {
                        id,
                        assignee_user_id,
                        clear_assignee_user_id,
                        ..
                    },
            } => {
                assert_eq!(id, 1);
                assert_eq!(assignee_user_id, Some("42".to_string()));
                assert!(!clear_assignee_user_id);
            }
            _ => panic!("expected Edit"),
        }
    }

    #[test]
    fn parse_edit_clear_assignee() {
        let cli = Cli::parse_from(["senko", "task", "edit", "1", "--clear-assignee-user-id"]);
        match cli.command {
            Command::Task {
                action:
                    TaskAction::Edit {
                        id,
                        assignee_user_id,
                        clear_assignee_user_id,
                        ..
                    },
            } => {
                assert_eq!(id, 1);
                assert!(assignee_user_id.is_none());
                assert!(clear_assignee_user_id);
            }
            _ => panic!("expected Edit"),
        }
    }

    #[test]
    fn parse_next_with_include_unassigned() {
        let cli = Cli::parse_from(["senko", "task", "next", "--include-unassigned"]);
        match cli.command {
            Command::Task {
                action:
                    TaskAction::Next {
                        include_unassigned, ..
                    },
            } => {
                assert!(include_unassigned);
            }
            _ => panic!("expected Next"),
        }
    }

    #[test]
    fn parse_list_with_include_unassigned() {
        let cli = Cli::parse_from(["senko", "task", "list", "--ready", "--include-unassigned"]);
        match cli.command {
            Command::Task {
                action:
                    TaskAction::List {
                        ready,
                        include_unassigned,
                        ..
                    },
            } => {
                assert!(ready);
                assert!(include_unassigned);
            }
            _ => panic!("expected List"),
        }
    }

    #[test]
    fn parse_project_members_list() {
        let cli = Cli::parse_from(["senko", "project", "members", "list"]);
        assert!(matches!(
            cli.command,
            Command::Project {
                action: ProjectAction::Members {
                    action: MemberAction::List
                }
            }
        ));
    }

    #[test]
    fn parse_project_members_add() {
        let cli = Cli::parse_from([
            "senko",
            "project",
            "members",
            "add",
            "--user-id",
            "3",
            "--role",
            "member",
        ]);
        match cli.command {
            Command::Project {
                action:
                    ProjectAction::Members {
                        action: MemberAction::Add { user_id, role },
                    },
            } => {
                assert_eq!(user_id, 3);
                assert!(matches!(role, Some(CliRole::Member)));
            }
            _ => panic!("expected Project Members Add"),
        }
    }

    #[test]
    fn parse_project_members_remove() {
        let cli = Cli::parse_from(["senko", "project", "members", "remove", "--user-id", "4"]);
        match cli.command {
            Command::Project {
                action:
                    ProjectAction::Members {
                        action: MemberAction::Remove { user_id },
                    },
            } => {
                assert_eq!(user_id, 4);
            }
            _ => panic!("expected Project Members Remove"),
        }
    }

    #[test]
    fn parse_project_members_set_role() {
        let cli = Cli::parse_from([
            "senko",
            "project",
            "members",
            "set-role",
            "--user-id",
            "5",
            "--role",
            "viewer",
        ]);
        match cli.command {
            Command::Project {
                action:
                    ProjectAction::Members {
                        action: MemberAction::SetRole { user_id, role },
                    },
            } => {
                assert_eq!(user_id, 5);
                assert!(matches!(role, CliRole::Viewer));
            }
            _ => panic!("expected Project Members SetRole"),
        }
    }

    #[test]
    fn parse_no_top_level_add() {
        // After restructuring, `senko add` must not be accepted as an alias.
        let result = Cli::try_parse_from(["senko", "add"]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_no_top_level_members() {
        // After restructuring, `senko members` must not be accepted as an alias.
        let result = Cli::try_parse_from(["senko", "members", "list"]);
        assert!(result.is_err());
    }
}
