use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

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
    SkillInstall,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Add => todo!("add"),
        Command::List => todo!("list"),
        Command::Get => todo!("get"),
        Command::Next => todo!("next"),
        Command::Edit => todo!("edit"),
        Command::Complete => todo!("complete"),
        Command::Cancel => todo!("cancel"),
        Command::Deps => todo!("deps"),
        Command::SkillInstall => todo!("skill-install"),
    }
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
        assert!(matches!(cli.command, Command::SkillInstall));
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
