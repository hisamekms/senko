use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use super::{Cli, DryRunOperation, print_dry_run};
use crate::infra::project_root::resolve_project_root;

pub const SKILL_MD_CONTENT: &str = include_str!("../../skill_md.txt");
pub const DOD_VERIFIER_AGENT_CONTENT: &str = include_str!("../../dod_verifier_agent.md");

/// File to install with its relative path under `.claude/` and content.
pub struct InstallableFile {
    /// Path segments under `.claude/` (e.g. `["skills", "localflow", "SKILL.md"]`)
    pub segments: &'static [&'static str],
    pub content: &'static str,
}

pub const INSTALLABLE_FILES: &[InstallableFile] = &[
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
pub fn should_write_file(path: &std::path::Path, content: &str, yes: bool) -> Result<bool> {
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

pub fn skill_install(cli: &Cli, output_dir: Option<PathBuf>, yes: bool) -> Result<()> {
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
    use super::*;
    use super::super::{Command, OutputFormat};

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
            user: None,
            command: super::super::Command::SkillInstall {
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
            db_path: None,
            postgres_url: None,
            project: None,
            user: None,
            command: super::super::Command::SkillInstall {
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
            config: None,
            dry_run: false,
            log_dir: None,
            db_path: None,
            postgres_url: None,
            project: None,
            user: None,
            command: super::super::Command::SkillInstall {
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
            config: None,
            dry_run: false,
            log_dir: None,
            db_path: None,
            postgres_url: None,
            project: None,
            user: None,
            command: super::super::Command::SkillInstall {
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
            config: None,
            dry_run: false,
            log_dir: None,
            db_path: None,
            postgres_url: None,
            project: None,
            user: None,
            command: super::super::Command::SkillInstall {
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
            config: None,
            dry_run: false,
            log_dir: None,
            db_path: None,
            postgres_url: None,
            project: None,
            user: None,
            command: super::super::Command::SkillInstall {
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
}
