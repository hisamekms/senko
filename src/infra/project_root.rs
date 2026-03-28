use std::path::{Path, PathBuf};

use anyhow::Result;

pub fn resolve_project_root(explicit: Option<&Path>) -> Result<PathBuf> {
    // 1. Explicit CLI flag takes top priority
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }

    // 2. SENKO_PROJECT_ROOT env var
    if let Ok(val) = std::env::var("SENKO_PROJECT_ROOT") {
        if !val.is_empty() {
            return Ok(PathBuf::from(val));
        }
    }

    // 3. Search upward from current directory
    let start_dir = std::env::current_dir()?;
    resolve_from(None, &start_dir)
}

fn resolve_from(explicit: Option<&Path>, start_dir: &Path) -> Result<PathBuf> {
    // 1. Explicit path takes priority
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }

    // 2. Search upward for .senko/
    if let Some(root) = search_upward(start_dir, ".senko") {
        return Ok(root);
    }

    // 3. Search upward for .git
    if let Some(root) = search_upward(start_dir, ".git") {
        return Ok(root);
    }

    // 4. Fallback to current directory
    Ok(start_dir.to_path_buf())
}

fn search_upward(start: &Path, marker: &str) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join(marker);
        if candidate.exists() {
            // Reject symlinks to prevent symlink attacks
            if let Ok(meta) = std::fs::symlink_metadata(&candidate) {
                if meta.file_type().is_symlink() {
                    if !dir.pop() {
                        return None;
                    }
                    continue;
                }
            }
            // Canonicalize to resolve any path traversal
            return dir.canonicalize().ok();
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_path_takes_priority() {
        let explicit = PathBuf::from("/tmp/explicit");
        let result = resolve_from(Some(&explicit), Path::new("/tmp/other")).unwrap();
        assert_eq!(result, explicit);
    }

    #[test]
    fn detects_senko_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("a/b/c");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(tmp.path().join("a/.senko")).unwrap();

        let result = resolve_from(None, &sub).unwrap();
        assert_eq!(result, tmp.path().join("a").canonicalize().unwrap());
    }

    #[test]
    fn detects_git_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("x/y");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();

        let result = resolve_from(None, &sub).unwrap();
        assert_eq!(result, tmp.path().canonicalize().unwrap());
    }

    #[test]
    fn senko_takes_priority_over_git() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("proj/src");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::create_dir_all(tmp.path().join("proj/.senko")).unwrap();

        let result = resolve_from(None, &sub).unwrap();
        assert_eq!(result, tmp.path().join("proj").canonicalize().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_marker_is_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().join("evil");
        std::fs::create_dir_all(&target_dir).unwrap();
        // Create a symlink .senko -> evil (symlink attack)
        std::os::unix::fs::symlink(&target_dir, tmp.path().join(".senko")).unwrap();

        let result = search_upward(tmp.path(), ".senko");
        assert!(result.is_none(), "symlink marker should be skipped");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_marker_skipped_finds_real_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let child = tmp.path().join("child");
        std::fs::create_dir_all(&child).unwrap();
        // Symlink .senko in child dir
        let target_dir = tmp.path().join("evil");
        std::fs::create_dir_all(&target_dir).unwrap();
        std::os::unix::fs::symlink(&target_dir, child.join(".senko")).unwrap();
        // Real .senko in parent
        std::fs::create_dir_all(tmp.path().join(".senko")).unwrap();

        let result = search_upward(&child, ".senko");
        assert_eq!(result, Some(tmp.path().canonicalize().unwrap()));
    }

    #[test]
    fn fallback_to_start_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let result = resolve_from(None, tmp.path()).unwrap();
        assert_eq!(result, tmp.path().to_path_buf());
    }
}
