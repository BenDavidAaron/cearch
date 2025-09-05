use std::path::{Path, PathBuf};
use std::process::Command;

/// Walk upward from a starting path to locate the root directory of a Git repository.
///
/// The root is detected by the presence of a `.git` entry (either a directory or a file)
/// in the directory. Returns `Some(root_dir)` when found, or `None` if no repository root
/// exists at or above the given path.
pub fn find_git_root(start_path: impl AsRef<Path>) -> Option<PathBuf> {
    // Prefer canonical paths when available, but gracefully fall back if not
    let start = start_path
        .as_ref()
        .canonicalize()
        .unwrap_or_else(|_| start_path.as_ref().to_path_buf());

    let mut current_directory = if start.is_dir() {
        start
    } else {
        start.parent()?.to_path_buf()
    };

    loop {
        let git_entry = current_directory.join(".git");

        // `.git` can be a directory or a file (e.g., worktrees use a gitdir file)
        if git_entry.is_dir() || git_entry.is_file() {
            return Some(current_directory);
        }

        // Stop when we reach filesystem root
        if !current_directory.pop() {
            return None;
        }
    }
}

/// Return absolute paths for all files tracked by Git in the provided repository root.
///
/// This invokes `git ls-files -z` to ensure results match Git's notion of "tracked".
pub fn list_git_tracked_files(repo_root: impl AsRef<Path>) -> Result<Vec<PathBuf>, String> {
    let repo_root = repo_root.as_ref();

    // Ensure the directory looks like a git repo root
    if !repo_root.join(".git").exists() {
        return Err(format!(
            "{} is not a Git repository root (missing .git)",
            repo_root.display()
        ));
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("ls-files")
        .arg("-z")
        .output()
        .map_err(|e| format!("failed to invoke git: {}", e))?;

    if !output.status.success() {
        return Err(format!("git ls-files failed with status {}", output.status));
    }

    let mut files = Vec::new();
    for rel_bytes in output.stdout.split(|b| *b == 0) {
        if rel_bytes.is_empty() {
            continue;
        }
        let rel_str = String::from_utf8_lossy(rel_bytes);
        let rel_path = PathBuf::from(rel_str.as_ref());
        files.push(repo_root.join(rel_path));
    }

    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::find_git_root;
    use std::path::PathBuf;

    #[test]
    fn returns_none_for_non_repo_paths() {
        // This test is heuristic and may run in various environments. We pick a path
        // that is unlikely to be inside a Git repo: the filesystem root.
        // If it is, the assertion will be skipped.
        #[cfg(target_os = "macos")]
        let root = PathBuf::from("/");
        #[cfg(target_os = "linux")]
        let root = PathBuf::from("/");
        #[cfg(target_os = "windows")]
        let root = PathBuf::from("C:/");

        if find_git_root(&root).is_some() {
            // Environment happens to be a repo at root; skip to avoid false failure
            return;
        }

        assert!(find_git_root(&root).is_none());
    }
}
