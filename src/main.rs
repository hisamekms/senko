use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use chrono::Utc;
use clap::{Parser, Subcommand, ValueEnum};

use localflow::db;
use localflow::models::{TaskStatus, UpdateTaskParams};
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
    Add,
    /// List tasks
    List,
    /// Get task details
    Get,
    /// Show the next task to work on
    Next {
        #[arg(long)]
        session_id: Option<String>,
    },
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
        Command::List => todo!("list"),
        Command::Get => todo!("get"),
        Command::Next { ref session_id } => cmd_next(&cli, session_id.clone()),
        Command::Edit => todo!("edit"),
        Command::Complete => todo!("complete"),
        Command::Cancel => todo!("cancel"),
        Command::Deps => todo!("deps"),
        Command::SkillInstall { output_dir } => skill_install(output_dir),
    }
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
        assert!(matches!(cli.command, Command::List));
    }

    #[test]
    fn parse_get_subcommand() {
        let cli = Cli::parse_from(["localflow", "get"]);
        assert!(matches!(cli.command, Command::Get));
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
