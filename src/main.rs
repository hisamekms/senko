use std::fs;
use std::io::Read;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use localflow::db;
use localflow::models::{CreateTaskParams, Priority};

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
    SkillInstall {
        /// Output directory for SKILL.md
        #[arg(long)]
        output_dir: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
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
            *from_json,
            from_json_file.clone(),
        ),
        Command::List => todo!("list"),
        Command::Get => todo!("get"),
        Command::Next => todo!("next"),
        Command::Edit => todo!("edit"),
        Command::Complete => todo!("complete"),
        Command::Cancel => todo!("cancel"),
        Command::Deps => todo!("deps"),
        Command::SkillInstall { output_dir } => skill_install(output_dir.clone()),
    }
}

fn resolve_project_root(project_root: Option<PathBuf>) -> Result<PathBuf> {
    match project_root {
        Some(p) => Ok(p),
        None => std::env::current_dir().context("failed to get current directory"),
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
    let root = resolve_project_root(cli.project_root.clone())?;
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
        // Verify task was created
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
        // Verify task was created
        let conn = db::open_db(tmp.path()).unwrap();
        let task = db::get_task(&conn, 1).unwrap();
        assert_eq!(task.title, "json out");
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
