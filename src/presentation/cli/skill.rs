use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::{Cli, DryRunOperation, print_dry_run};
use crate::bootstrap::resolve_project_root;

pub const SKILL_MD_CONTENT: &str = include_str!("../../skill_md.txt");
pub const DOD_VERIFIER_AGENT_CONTENT: &str = include_str!("../../dod_verifier_agent.md");

const CLI_REFERENCE_CONTENT: &str =
    include_str!("../../../.claude/skills/senko/cli-reference.md");

const WF_ADD_TASK: &str =
    include_str!("../../../.claude/skills/senko/workflows/add-task.md");
const WF_AUTO_SELECT: &str =
    include_str!("../../../.claude/skills/senko/workflows/auto-select.md");
const WF_CANCEL_TASK: &str =
    include_str!("../../../.claude/skills/senko/workflows/cancel-task.md");
const WF_COMPLETE_TASK: &str =
    include_str!("../../../.claude/skills/senko/workflows/complete-task.md");
const WF_CONFIG_EXPLAIN: &str =
    include_str!("../../../.claude/skills/senko/workflows/config-explain.md");
const WF_CONFIG_SETUP: &str =
    include_str!("../../../.claude/skills/senko/workflows/config-setup.md");
const WF_DEPENDENCY_GRAPH: &str =
    include_str!("../../../.claude/skills/senko/workflows/dependency-graph.md");
const WF_DOD_CHECK: &str =
    include_str!("../../../.claude/skills/senko/workflows/dod-check.md");
const WF_EXECUTE_TASK: &str =
    include_str!("../../../.claude/skills/senko/workflows/execute-task.md");
const WF_LIST_TASKS: &str =
    include_str!("../../../.claude/skills/senko/workflows/list-tasks.md");
const WF_MANAGE_DEPS: &str =
    include_str!("../../../.claude/skills/senko/workflows/manage-dependencies.md");

const SCRIPT_CHECK_WORKFLOW_CONFIG: &str =
    include_str!("../../../.claude/skills/senko/scripts/check-workflow-config.sh");
const SCRIPT_GENERATE_PLAN_SECTIONS: &str =
    include_str!("../../../.claude/skills/senko/scripts/generate-plan-sections.sh");
const SCRIPT_REBASE_MERGE: &str =
    include_str!("../../../.claude/skills/senko/scripts/rebase-merge.sh");
const SCRIPT_SQUASH_MERGE: &str =
    include_str!("../../../.claude/skills/senko/scripts/squash-merge.sh");
const SCRIPT_BUILD_START_METADATA: &str =
    include_str!("../../../.claude/skills/senko/scripts/build-start-metadata.sh");

/// File to install with its relative path under `.claude/` and content.
pub struct InstallableFile {
    /// Path segments under `.claude/` (e.g. `["skills", "senko", "SKILL.md"]`)
    pub segments: &'static [&'static str],
    pub content: &'static str,
}

pub const INSTALLABLE_FILES: &[InstallableFile] = &[
    // Main skill definition
    InstallableFile {
        segments: &["skills", "senko", "SKILL.md"],
        content: SKILL_MD_CONTENT,
    },
    // CLI reference
    InstallableFile {
        segments: &["skills", "senko", "cli-reference.md"],
        content: CLI_REFERENCE_CONTENT,
    },
    // Workflows
    InstallableFile {
        segments: &["skills", "senko", "workflows", "add-task.md"],
        content: WF_ADD_TASK,
    },
    InstallableFile {
        segments: &["skills", "senko", "workflows", "auto-select.md"],
        content: WF_AUTO_SELECT,
    },
    InstallableFile {
        segments: &["skills", "senko", "workflows", "cancel-task.md"],
        content: WF_CANCEL_TASK,
    },
    InstallableFile {
        segments: &["skills", "senko", "workflows", "complete-task.md"],
        content: WF_COMPLETE_TASK,
    },
    InstallableFile {
        segments: &["skills", "senko", "workflows", "config-explain.md"],
        content: WF_CONFIG_EXPLAIN,
    },
    InstallableFile {
        segments: &["skills", "senko", "workflows", "config-setup.md"],
        content: WF_CONFIG_SETUP,
    },
    InstallableFile {
        segments: &["skills", "senko", "workflows", "dependency-graph.md"],
        content: WF_DEPENDENCY_GRAPH,
    },
    InstallableFile {
        segments: &["skills", "senko", "workflows", "dod-check.md"],
        content: WF_DOD_CHECK,
    },
    InstallableFile {
        segments: &["skills", "senko", "workflows", "execute-task.md"],
        content: WF_EXECUTE_TASK,
    },
    InstallableFile {
        segments: &["skills", "senko", "workflows", "list-tasks.md"],
        content: WF_LIST_TASKS,
    },
    InstallableFile {
        segments: &["skills", "senko", "workflows", "manage-dependencies.md"],
        content: WF_MANAGE_DEPS,
    },
    // Scripts
    InstallableFile {
        segments: &["skills", "senko", "scripts", "check-workflow-config.sh"],
        content: SCRIPT_CHECK_WORKFLOW_CONFIG,
    },
    InstallableFile {
        segments: &["skills", "senko", "scripts", "generate-plan-sections.sh"],
        content: SCRIPT_GENERATE_PLAN_SECTIONS,
    },
    InstallableFile {
        segments: &["skills", "senko", "scripts", "rebase-merge.sh"],
        content: SCRIPT_REBASE_MERGE,
    },
    InstallableFile {
        segments: &["skills", "senko", "scripts", "squash-merge.sh"],
        content: SCRIPT_SQUASH_MERGE,
    },
    InstallableFile {
        segments: &["skills", "senko", "scripts", "build-start-metadata.sh"],
        content: SCRIPT_BUILD_START_METADATA,
    },
    // Agent
    InstallableFile {
        segments: &["agents", "dod-verifier.md"],
        content: DOD_VERIFIER_AGENT_CONTENT,
    },
];

/// Directories under `.claude/` owned by senko. Deleted entirely during clean install.
const CLEAN_INSTALL_DIRS: &[&[&str]] = &[&["skills", "senko"]];

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

/// Returns true if any installable file already exists under `base_dir`.
fn any_install_target_exists(base_dir: &Path) -> bool {
    INSTALLABLE_FILES.iter().any(|f| {
        let path = f.segments.iter().fold(base_dir.to_path_buf(), |p, s| p.join(s));
        path.exists()
    })
}

/// Returns true if any installable file already exists (flat layout) under `dir`.
fn any_install_target_exists_flat(dir: &Path) -> bool {
    INSTALLABLE_FILES.iter().any(|f| {
        let filename = f.segments.last().unwrap();
        dir.join(filename).exists()
    })
}

/// Remove senko-owned directories and individual files under `base_dir` for clean install.
fn clean_install_targets(base_dir: &Path) -> Result<()> {
    // Remove senko-owned directories
    for segments in CLEAN_INSTALL_DIRS {
        let dir = segments.iter().fold(base_dir.to_path_buf(), |p, s| p.join(s));
        if dir.exists() {
            fs::remove_dir_all(&dir)
                .with_context(|| format!("failed to remove directory: {}", dir.display()))?;
            println!("Removed {}", dir.display());
        }
    }
    // Remove individual files not covered by CLEAN_INSTALL_DIRS
    for file in INSTALLABLE_FILES {
        let path = file.segments.iter().fold(base_dir.to_path_buf(), |p, s| p.join(s));
        if !path.exists() {
            continue;
        }
        let covered = CLEAN_INSTALL_DIRS.iter().any(|dir_segs| {
            file.segments.len() > dir_segs.len()
                && file.segments[..dir_segs.len()] == **dir_segs
        });
        if !covered {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove file: {}", path.display()))?;
            println!("Removed {}", path.display());
        }
    }
    Ok(())
}

/// Remove individual installable files (flat layout) from `dir` for clean install.
fn clean_install_targets_flat(dir: &Path) -> Result<()> {
    for file in INSTALLABLE_FILES {
        let filename = file.segments.last().unwrap();
        let path = dir.join(filename);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove file: {}", path.display()))?;
            println!("Removed {}", path.display());
        }
    }
    Ok(())
}

/// Prompt user for confirmation and return true if they accept.
fn confirm(prompt: &str) -> Result<bool> {
    eprint!("{prompt}");
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .context("failed to read from stdin")?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}

pub fn skill_install(cli: &Cli, output_dir: Option<PathBuf>, yes: bool, force: bool) -> Result<()> {
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
        // Clean install for --output-dir mode
        if force && any_install_target_exists_flat(&dir) {
            clean_install_targets_flat(&dir)?;
        } else if !force && any_install_target_exists_flat(&dir) {
            if confirm("Existing files found. Clean install? [y/N] ")? {
                clean_install_targets_flat(&dir)?;
            }
        }
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

    if created_claude_dir && !yes && !force {
        if !confirm(&format!(
            ".claude/ directory does not exist. Create it at {}? [y/N] ",
            claude_dir.display()
        ))? {
            bail!("aborted");
        }
    }

    // Clean install when targets already exist
    if !created_claude_dir && any_install_target_exists(&claude_dir) {
        if force {
            clean_install_targets(&claude_dir)?;
        } else if confirm("Existing files found. Clean install? [y/N] ")? {
            clean_install_targets(&claude_dir)?;
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

    fn make_cli(tmp: &tempfile::TempDir) -> Cli {
        Cli {
            output: OutputFormat::Text,
            project_root: Some(tmp.path().to_path_buf()),
            config: None,
            dry_run: false,
            log_dir: None,
            db_path: None,
            postgres_url: None,
            project: None,
            user: None,
            command: Command::SkillInstall {
                output_dir: None,
                yes: true,
                force: false,
            },
        }
    }

    #[test]
    fn skill_install_with_output_dir_creates_files() {
        let dir = tempfile::tempdir().unwrap();
        let cli = Cli {
            command: Command::SkillInstall {
                output_dir: Some(dir.path().to_path_buf()),
                yes: false,
                force: false,
            },
            ..make_cli(&dir)
        };
        skill_install(&cli, Some(dir.path().to_path_buf()), false, false).unwrap();

        let content = std::fs::read_to_string(dir.path().join("SKILL.md")).unwrap();
        assert_eq!(content, SKILL_MD_CONTENT);
        let agent_content =
            std::fs::read_to_string(dir.path().join("dod-verifier.md")).unwrap();
        assert_eq!(agent_content, DOD_VERIFIER_AGENT_CONTENT);
        // Verify workflow and other files are present (flat mode uses last segment as filename)
        assert!(dir.path().join("cli-reference.md").exists());
        assert!(dir.path().join("add-task.md").exists());
        assert!(dir.path().join("execute-task.md").exists());
        assert!(dir.path().join("rebase-merge.sh").exists());
    }

    #[test]
    fn skill_install_default_places_in_claude_skills() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = make_cli(&tmp);
        skill_install(&cli, None, true, false).unwrap();

        let senko_dir = tmp.path().join(".claude/skills/senko");

        let skill_path = senko_dir.join("SKILL.md");
        assert!(skill_path.exists());
        let content = std::fs::read_to_string(&skill_path).unwrap();
        assert_eq!(content, SKILL_MD_CONTENT);

        // CLI reference
        assert!(senko_dir.join("cli-reference.md").exists());

        // Workflows
        let wf_dir = senko_dir.join("workflows");
        for name in [
            "add-task.md",
            "auto-select.md",
            "cancel-task.md",
            "complete-task.md",
            "config-explain.md",
            "config-setup.md",
            "dependency-graph.md",
            "dod-check.md",
            "execute-task.md",
            "list-tasks.md",
            "manage-dependencies.md",
        ] {
            assert!(wf_dir.join(name).exists(), "missing workflow: {name}");
        }

        // Scripts
        let scripts_dir = senko_dir.join("scripts");
        for name in [
            "check-workflow-config.sh",
            "generate-plan-sections.sh",
            "rebase-merge.sh",
            "squash-merge.sh",
            "build-start-metadata.sh",
        ] {
            assert!(scripts_dir.join(name).exists(), "missing script: {name}");
        }

        // Agent
        let agent_path = tmp.path().join(".claude/agents/dod-verifier.md");
        assert!(agent_path.exists());
        let agent_content = std::fs::read_to_string(&agent_path).unwrap();
        assert_eq!(agent_content, DOD_VERIFIER_AGENT_CONTENT);
    }

    #[test]
    fn skill_install_existing_claude_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".claude")).unwrap();

        let cli = make_cli(&tmp);
        // Should not prompt since .claude/ already exists (no existing install targets)
        skill_install(&cli, None, false, false).unwrap();

        assert!(tmp.path().join(".claude/skills/senko/SKILL.md").exists());
        assert!(tmp.path().join(".claude/agents/dod-verifier.md").exists());
    }

    #[test]
    fn skill_install_no_claude_dir_with_yes() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!tmp.path().join(".claude").exists());

        let cli = make_cli(&tmp);
        skill_install(&cli, None, true, false).unwrap();

        assert!(tmp.path().join(".claude").exists());
        assert!(tmp.path().join(".claude/skills/senko/SKILL.md").exists());
        assert!(tmp.path().join(".claude/agents/dod-verifier.md").exists());
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
        let cli = make_cli(&tmp);
        skill_install(&cli, None, true, false).unwrap();
        // Second install should succeed (files are up to date)
        skill_install(&cli, None, true, false).unwrap();

        let skill_path = tmp.path().join(".claude/skills/senko/SKILL.md");
        let content = std::fs::read_to_string(&skill_path).unwrap();
        assert_eq!(content, SKILL_MD_CONTENT);
    }

    #[test]
    fn skill_install_overwrites_with_yes() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = make_cli(&tmp);
        skill_install(&cli, None, true, false).unwrap();
        // Tamper with the file
        let skill_path = tmp.path().join(".claude/skills/senko/SKILL.md");
        std::fs::write(&skill_path, "modified content").unwrap();
        // Reinstall with --yes should overwrite
        skill_install(&cli, None, true, false).unwrap();
        let content = std::fs::read_to_string(&skill_path).unwrap();
        assert_eq!(content, SKILL_MD_CONTENT);
    }

    #[test]
    fn skill_install_force_removes_old_files_and_reinstalls() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = make_cli(&tmp);
        // First install
        skill_install(&cli, None, true, false).unwrap();
        // Add a stale file in the senko skill directory
        let stale_file = tmp.path().join(".claude/skills/senko/old-file.md");
        std::fs::write(&stale_file, "stale").unwrap();
        assert!(stale_file.exists());

        // Force reinstall should remove the stale file
        skill_install(&cli, None, true, true).unwrap();
        assert!(!stale_file.exists());
        // Fresh files should be present
        assert!(tmp.path().join(".claude/skills/senko/SKILL.md").exists());
        assert!(tmp.path().join(".claude/agents/dod-verifier.md").exists());
    }

    #[test]
    fn skill_install_force_removes_individual_files_not_in_clean_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = make_cli(&tmp);
        skill_install(&cli, None, true, false).unwrap();

        // Verify agents/dod-verifier.md exists
        let agent_path = tmp.path().join(".claude/agents/dod-verifier.md");
        assert!(agent_path.exists());

        // Add another file in agents/ (should NOT be removed)
        let user_agent = tmp.path().join(".claude/agents/my-agent.md");
        std::fs::write(&user_agent, "user agent").unwrap();

        // Force reinstall
        skill_install(&cli, None, true, true).unwrap();
        // dod-verifier.md should be reinstalled
        assert!(agent_path.exists());
        // User's agent should be untouched
        assert!(user_agent.exists());
        let content = std::fs::read_to_string(&user_agent).unwrap();
        assert_eq!(content, "user agent");
    }

    #[test]
    fn skill_install_force_no_existing_files_installs_normally() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = make_cli(&tmp);
        // Force on fresh install should work fine
        skill_install(&cli, None, true, true).unwrap();
        assert!(tmp.path().join(".claude/skills/senko/SKILL.md").exists());
        assert!(tmp.path().join(".claude/agents/dod-verifier.md").exists());
    }

    #[test]
    fn skill_install_force_with_output_dir() {
        let dir = tempfile::tempdir().unwrap();
        // Pre-create files
        std::fs::write(dir.path().join("SKILL.md"), "old").unwrap();
        std::fs::write(dir.path().join("dod-verifier.md"), "old").unwrap();
        std::fs::write(dir.path().join("unrelated.txt"), "keep").unwrap();

        let cli = Cli {
            command: Command::SkillInstall {
                output_dir: Some(dir.path().to_path_buf()),
                yes: false,
                force: true,
            },
            ..make_cli(&dir)
        };
        skill_install(&cli, Some(dir.path().to_path_buf()), false, true).unwrap();

        // Installable files should be refreshed
        let content = std::fs::read_to_string(dir.path().join("SKILL.md")).unwrap();
        assert_eq!(content, SKILL_MD_CONTENT);
        // Unrelated file should remain
        let unrelated = std::fs::read_to_string(dir.path().join("unrelated.txt")).unwrap();
        assert_eq!(unrelated, "keep");
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
}
