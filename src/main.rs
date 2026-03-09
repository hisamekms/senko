use std::fs;
use std::io::Read;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand, ValueEnum};
use localflow::db;
use localflow::models::{
    CreateTaskParams, ListTasksFilter, Priority, TaskStatus, UpdateTaskArrayParams,
    UpdateTaskParams,
};
use localflow::project::resolve_project_root;

#[derive(Debug, Clone, ValueEnum)]
enum OutputFormat {
    Json,
    Text,
}

#[derive(Debug, Parser)]
#[command(name = "localflow", about = "Local task management CLI")]
struct Cli {
    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    output: OutputFormat,

    /// Project root directory
    #[arg(long)]
    project_root: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
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
        details: Option<String>,
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
        /// Read JSON from stdin
        #[arg(long, conflicts_with_all = ["title", "background", "details", "priority", "definition_of_done", "in_scope", "out_of_scope", "tag", "depends_on"])]
        from_json: bool,
        /// Read JSON from file
        #[arg(long, conflicts_with_all = ["title", "background", "details", "priority", "definition_of_done", "in_scope", "out_of_scope", "tag", "depends_on", "from_json"])]
        from_json_file: Option<PathBuf>,
    },
    /// List tasks
    List {
        /// Filter by status (draft, todo, in_progress, completed, canceled)
        #[arg(long)]
        status: Option<String>,
        /// Filter by tag
        #[arg(long)]
        tag: Option<String>,
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
        details: Option<String>,
        #[arg(long)]
        clear_details: bool,
        #[arg(long, value_enum)]
        priority: Option<Priority>,
        #[arg(long, value_enum)]
        status: Option<TaskStatus>,
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
    },
    /// Cancel a task
    Cancel {
        /// Task ID
        id: i64,
        /// Cancellation reason
        #[arg(long)]
        reason: Option<String>,
    },
    /// Manage task dependencies
    Deps {
        #[command(subcommand)]
        command: DepsCommand,
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

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Add {
            ref title,
            ref background,
            ref details,
            ref priority,
            ref definition_of_done,
            ref in_scope,
            ref out_of_scope,
            ref tag,
            ref depends_on,
            from_json,
            ref from_json_file,
        } => cmd_add(
            &cli,
            title.clone(),
            background.clone(),
            details.clone(),
            priority.clone(),
            definition_of_done.clone(),
            in_scope.clone(),
            out_of_scope.clone(),
            tag.clone(),
            depends_on.clone(),
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
        Command::Edit {
            id,
            title,
            background,
            clear_background,
            details,
            clear_details,
            priority,
            status,
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
            let conn = db::open_db(&project_root)?;

            let scalar_params = UpdateTaskParams {
                title,
                background: if clear_background {
                    Some(None)
                } else {
                    background.map(Some)
                },
                details: if clear_details {
                    Some(None)
                } else {
                    details.map(Some)
                },
                priority,
                status,
                assignee_session_id: None,
                started_at: None,
                completed_at: None,
                canceled_at: None,
                cancel_reason: None,
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

            db::update_task(&conn, id, &scalar_params)?;
            db::update_task_arrays(&conn, id, &array_params)?;
            let task = db::get_task(&conn, id)?;

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
                    if let Some(ref det) = task.details {
                        println!("  details: {det}");
                    }
                    if !task.tags.is_empty() {
                        println!("  tags: {}", task.tags.join(", "));
                    }
                }
            }
            Ok(())
        }
        Command::Complete { id } => cmd_complete(&cli, id),
        Command::Cancel { id, ref reason } => cmd_cancel(&cli, id, reason.clone()),
        Command::Deps { ref command } => cmd_deps(&cli, command),
        Command::SkillInstall { ref output_dir, yes } => {
            skill_install(&cli, output_dir.clone(), yes)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_add(
    cli: &Cli,
    title: Option<String>,
    background: Option<String>,
    details: Option<String>,
    priority: Option<String>,
    definition_of_done: Vec<String>,
    in_scope: Vec<String>,
    out_of_scope: Vec<String>,
    tag: Vec<String>,
    depends_on: Vec<i64>,
    from_json: bool,
    from_json_file: Option<PathBuf>,
) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let conn = db::open_db(&root)?;

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
        CreateTaskParams {
            title,
            background,
            details,
            priority,
            definition_of_done,
            in_scope,
            out_of_scope,
            tags: tag,
            dependencies: depends_on,
        }
    };

    let task = db::create_task(&conn, &params)?;

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

fn cmd_list(
    output: &OutputFormat,
    project_root: Option<&std::path::Path>,
    status: Option<String>,
    tag: Option<String>,
    depends_on: Option<i64>,
    ready: bool,
) -> Result<()> {
    let root = resolve_project_root(project_root)?;
    let conn = db::open_db(&root)?;

    let status = status
        .map(|s| s.parse::<TaskStatus>())
        .transpose()
        .context("invalid status value")?;

    let filter = ListTasksFilter {
        status,
        tag,
        depends_on,
        ready,
    };

    let tasks = db::list_tasks(&conn, &filter)?;

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
    let conn = db::open_db(&root)?;
    let task = db::get_task(&conn, task_id)?;

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
            if let Some(ref det) = task.details {
                println!("Details:  {det}");
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
            if !task.definition_of_done.is_empty() {
                println!("DoD:");
                for item in &task.definition_of_done {
                    println!("  - {item}");
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

fn cmd_next(cli: &Cli, session_id: Option<String>) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let conn = db::open_db(&root)?;

    let task = db::next_task(&conn)?.ok_or_else(|| anyhow::anyhow!("no eligible task found"))?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let updated = db::update_task(
        &conn,
        task.id,
        &UpdateTaskParams {
            title: None,
            background: None,
            details: None,
            priority: None,
            status: Some(TaskStatus::InProgress),
            assignee_session_id: Some(session_id),
            started_at: Some(Some(now)),
            completed_at: None,
            canceled_at: None,
            cancel_reason: None,
        },
    )?;

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

fn cmd_complete(cli: &Cli, id: i64) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let conn = db::open_db(&root)?;

    let task = db::get_task(&conn, id)?;
    task.status.transition_to(TaskStatus::Completed)?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let updated = db::update_task(
        &conn,
        id,
        &UpdateTaskParams {
            title: None,
            background: None,
            details: None,
            priority: None,
            status: Some(TaskStatus::Completed),
            assignee_session_id: None,
            started_at: None,
            completed_at: Some(Some(now)),
            canceled_at: None,
            cancel_reason: None,
        },
    )?;

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

fn cmd_cancel(cli: &Cli, id: i64, reason: Option<String>) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let conn = db::open_db(&root)?;

    let task = db::get_task(&conn, id)?;
    task.status.transition_to(TaskStatus::Canceled)?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let updated = db::update_task(
        &conn,
        id,
        &UpdateTaskParams {
            title: None,
            background: None,
            details: None,
            priority: None,
            status: Some(TaskStatus::Canceled),
            assignee_session_id: None,
            started_at: None,
            completed_at: None,
            canceled_at: Some(Some(now)),
            cancel_reason: reason.map(Some),
        },
    )?;

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

fn cmd_deps(cli: &Cli, command: &DepsCommand) -> Result<()> {
    let root = resolve_project_root(cli.project_root.as_deref())?;
    let conn = db::open_db(&root)?;

    match command {
        DepsCommand::Add { task_id, on } => {
            let (task_id, on) = (*task_id, *on);
            let task = db::add_dependency(&conn, task_id, on)?;
            match cli.output {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&task)?),
                OutputFormat::Text => println!("Added dependency: task #{} depends on #{}", task_id, on),
            }
        }
        DepsCommand::Remove { task_id, on } => {
            let (task_id, on) = (*task_id, *on);
            let task = db::remove_dependency(&conn, task_id, on)?;
            match cli.output {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&task)?),
                OutputFormat::Text => println!("Removed dependency: task #{} no longer depends on #{}", task_id, on),
            }
        }
        DepsCommand::Set { task_id, on } => {
            let task_id = *task_id;
            let task = db::set_dependencies(&conn, task_id, on)?;
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
            let deps = db::list_dependencies(&conn, *task_id)?;
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

fn skill_install(cli: &Cli, output_dir: Option<PathBuf>, yes: bool) -> Result<()> {
    if let Some(dir) = output_dir {
        let path = dir.join("SKILL.md");
        fs::write(&path, SKILL_MD_CONTENT)?;
        println!("SKILL.md written to {}", path.display());
        return Ok(());
    }

    let project_root = resolve_project_root(cli.project_root.as_deref())?;
    let claude_dir = project_root.join(".claude");
    let target_dir = claude_dir.join("skills").join("localflow");
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

    fs::create_dir_all(&target_dir)
        .with_context(|| format!("failed to create directory: {}", target_dir.display()))?;

    let path = target_dir.join("SKILL.md");
    fs::write(&path, SKILL_MD_CONTENT)?;
    println!("SKILL.md written to {}", path.display());

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
            "--details",
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
                details,
                priority,
                definition_of_done,
                in_scope,
                out_of_scope,
                tag,
                depends_on,
                from_json,
                from_json_file,
            } => {
                assert_eq!(title, Some("task".to_string()));
                assert_eq!(background, Some("bg".to_string()));
                assert_eq!(details, Some("det".to_string()));
                assert_eq!(priority, Some("p1".to_string()));
                assert_eq!(definition_of_done, vec!["done1", "done2"]);
                assert_eq!(in_scope, vec!["s1"]);
                assert_eq!(out_of_scope, vec!["o1"]);
                assert_eq!(tag, vec!["rust", "cli"]);
                assert_eq!(depends_on, vec![1, 2]);
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
            command: Command::Add {
                title: None,
                background: None,
                details: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                tag: vec![],
                depends_on: vec![],
                from_json: false,
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
            false,
            None,
        )
        .unwrap();

        let conn = db::open_db(tmp.path()).unwrap();
        let task = db::get_task(&conn, 1).unwrap();
        assert_eq!(task.title, "test task");
        assert_eq!(task.background.as_deref(), Some("bg"));
        assert_eq!(task.priority, localflow::models::Priority::P1);
        assert_eq!(task.definition_of_done, vec!["done"]);
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
            command: Command::Add {
                title: None,
                background: None,
                details: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                tag: vec![],
                depends_on: vec![],
                from_json: false,
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
            false,
            Some(json_path),
        )
        .unwrap();

        let conn = db::open_db(tmp.path()).unwrap();
        let task = db::get_task(&conn, 1).unwrap();
        assert_eq!(task.title, "file task");
        assert_eq!(task.priority, localflow::models::Priority::P0);
    }

    #[test]
    fn cmd_add_missing_title_error() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli {
            output: OutputFormat::Text,
            project_root: Some(tmp.path().to_path_buf()),
            command: Command::Add {
                title: None,
                background: None,
                details: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                tag: vec![],
                depends_on: vec![],
                from_json: false,
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
            command: Command::Add {
                title: None,
                background: None,
                details: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                tag: vec![],
                depends_on: vec![],
                from_json: false,
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
            false,
            None,
        )
        .unwrap();
        let conn = db::open_db(tmp.path()).unwrap();
        let task = db::get_task(&conn, 1).unwrap();
        assert_eq!(task.title, "my task");
    }

    #[test]
    fn cmd_add_json_output() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli {
            output: OutputFormat::Json,
            project_root: Some(tmp.path().to_path_buf()),
            command: Command::Add {
                title: None,
                background: None,
                details: None,
                priority: None,
                definition_of_done: vec![],
                in_scope: vec![],
                out_of_scope: vec![],
                tag: vec![],
                depends_on: vec![],
                from_json: false,
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
            false,
            None,
        )
        .unwrap();
        let conn = db::open_db(tmp.path()).unwrap();
        let task = db::get_task(&conn, 1).unwrap();
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
            "localflow", "list", "--status", "todo", "--tag", "rust", "--depends-on", "3",
            "--ready",
        ]);
        match cli.command {
            Command::List {
                status,
                tag,
                depends_on,
                ready,
            } => {
                assert_eq!(status.as_deref(), Some("todo"));
                assert_eq!(tag.as_deref(), Some("rust"));
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
            "localflow", "edit", "5", "--title", "new title", "--priority", "p0", "--status",
            "todo",
        ]);
        match cli.command {
            Command::Edit {
                id,
                title,
                priority,
                status,
                ..
            } => {
                assert_eq!(id, 5);
                assert_eq!(title.as_deref(), Some("new title"));
                assert_eq!(priority, Some(Priority::P0));
                assert_eq!(status, Some(TaskStatus::Todo));
            }
            _ => panic!("expected Edit"),
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
        assert!(matches!(cli.command, Command::Complete { id: 1 }));
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
    fn skill_install_with_output_dir_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let cli = Cli {
            output: OutputFormat::Text,
            project_root: None,
            command: Command::SkillInstall {
                output_dir: Some(dir.path().to_path_buf()),
                yes: false,
            },
        };
        skill_install(&cli, Some(dir.path().to_path_buf()), false).unwrap();

        let content = std::fs::read_to_string(dir.path().join("SKILL.md")).unwrap();
        assert_eq!(content, SKILL_MD_CONTENT);
    }

    #[test]
    fn skill_install_default_places_in_claude_skills() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli {
            output: OutputFormat::Text,
            project_root: Some(tmp.path().to_path_buf()),
            command: Command::SkillInstall {
                output_dir: None,
                yes: true,
            },
        };
        skill_install(&cli, None, true).unwrap();

        let expected = tmp
            .path()
            .join(".claude")
            .join("skills")
            .join("localflow")
            .join("SKILL.md");
        assert!(expected.exists());
        let content = std::fs::read_to_string(&expected).unwrap();
        assert_eq!(content, SKILL_MD_CONTENT);
    }

    #[test]
    fn skill_install_existing_claude_dir() {
        let tmp = tempfile::tempdir().unwrap();
        // Pre-create .claude/
        std::fs::create_dir_all(tmp.path().join(".claude")).unwrap();

        let cli = Cli {
            output: OutputFormat::Text,
            project_root: Some(tmp.path().to_path_buf()),
            command: Command::SkillInstall {
                output_dir: None,
                yes: false,
            },
        };
        // Should not prompt since .claude/ already exists
        skill_install(&cli, None, false).unwrap();

        let expected = tmp
            .path()
            .join(".claude")
            .join("skills")
            .join("localflow")
            .join("SKILL.md");
        assert!(expected.exists());
    }

    #[test]
    fn skill_install_no_claude_dir_with_yes() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!tmp.path().join(".claude").exists());

        let cli = Cli {
            output: OutputFormat::Text,
            project_root: Some(tmp.path().to_path_buf()),
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
    }

    #[test]
    fn skill_md_covers_all_commands() {
        let commands = [
            "localflow add",
            "localflow list",
            "localflow get",
            "localflow next",
            "localflow edit",
            "localflow complete",
            "localflow cancel",
            "localflow deps add",
            "localflow deps remove",
            "localflow deps set",
            "localflow deps list",
            "localflow skill-install",
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
    fn parse_output_text_default() {
        let cli = Cli::parse_from(["localflow", "add"]);
        assert!(matches!(cli.output, OutputFormat::Text));
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
