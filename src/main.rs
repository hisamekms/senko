use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use localflow::db;
use localflow::models::{ListTasksFilter, TaskStatus};
use localflow::project;

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
    Add,
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
    Next,
    /// Edit a task
    Edit,
    /// Mark a task as complete
    Complete,
    /// Cancel a task
    Cancel,
    /// Manage task dependencies
    Deps,
    /// Install a skill
    SkillInstall {
        /// Output directory for SKILL.md
        #[arg(long)]
        output_dir: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Add => todo!("add"),
        Command::List {
            status,
            tag,
            depends_on,
            ready,
        } => cmd_list(&cli.output, cli.project_root.as_deref(), status, tag, depends_on, ready),
        Command::Get { task_id } => cmd_get(&cli.output, cli.project_root.as_deref(), task_id),
        Command::Next => todo!("next"),
        Command::Edit => todo!("edit"),
        Command::Complete => todo!("complete"),
        Command::Cancel => todo!("cancel"),
        Command::Deps => todo!("deps"),
        Command::SkillInstall { output_dir } => skill_install(output_dir),
    }
}

fn cmd_list(
    output: &OutputFormat,
    project_root: Option<&std::path::Path>,
    status: Option<String>,
    tag: Option<String>,
    depends_on: Option<i64>,
    ready: bool,
) -> Result<()> {
    let root = project::resolve_project_root(project_root)?;
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
                println!("[{}] #{} {} ({})", task.status, task.id, task.title, task.priority);
            }
        }
    }
    Ok(())
}

fn cmd_get(output: &OutputFormat, project_root: Option<&std::path::Path>, task_id: i64) -> Result<()> {
    let root = project::resolve_project_root(project_root)?;
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

const SKILL_MD_CONTENT: &str = include_str!("skill_md.txt");

fn skill_install(output_dir: Option<PathBuf>) -> Result<()> {
    let dir = output_dir.unwrap_or_else(|| PathBuf::from("."));
    let path = dir.join("SKILL.md");
    fs::write(&path, SKILL_MD_CONTENT)?;
    println!("SKILL.md written to {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn parse_add_subcommand() {
        let cli = Cli::parse_from(["localflow", "add"]);
        assert!(matches!(cli.command, Command::Add));
    }

    #[test]
    fn parse_list_subcommand() {
        let cli = Cli::parse_from(["localflow", "list"]);
        assert!(matches!(cli.command, Command::List { .. }));
    }

    #[test]
    fn parse_list_with_filters() {
        let cli = Cli::parse_from([
            "localflow", "list", "--status", "todo", "--tag", "rust", "--depends-on", "3", "--ready",
        ]);
        match cli.command {
            Command::List { status, tag, depends_on, ready } => {
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
        assert!(matches!(cli.command, Command::Next));
    }

    #[test]
    fn parse_edit_subcommand() {
        let cli = Cli::parse_from(["localflow", "edit"]);
        assert!(matches!(cli.command, Command::Edit));
    }

    #[test]
    fn parse_complete_subcommand() {
        let cli = Cli::parse_from(["localflow", "complete"]);
        assert!(matches!(cli.command, Command::Complete));
    }

    #[test]
    fn parse_cancel_subcommand() {
        let cli = Cli::parse_from(["localflow", "cancel"]);
        assert!(matches!(cli.command, Command::Cancel));
    }

    #[test]
    fn parse_deps_subcommand() {
        let cli = Cli::parse_from(["localflow", "deps"]);
        assert!(matches!(cli.command, Command::Deps));
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
            Command::SkillInstall { output_dir } => {
                assert_eq!(output_dir, Some(PathBuf::from("/tmp/out")));
            }
            _ => panic!("expected SkillInstall"),
        }
    }

    #[test]
    fn parse_skill_install_without_output_dir() {
        let cli = Cli::parse_from(["localflow", "skill-install"]);
        match cli.command {
            Command::SkillInstall { output_dir } => {
                assert!(output_dir.is_none());
            }
            _ => panic!("expected SkillInstall"),
        }
    }

    #[test]
    fn skill_install_creates_file() {
        let dir = std::env::temp_dir().join("localflow_test_skill_install");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        skill_install(Some(dir.clone())).unwrap();

        let content = std::fs::read_to_string(dir.join("SKILL.md")).unwrap();
        assert_eq!(content, SKILL_MD_CONTENT);

        std::fs::remove_dir_all(&dir).unwrap();
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
            "localflow deps",
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
