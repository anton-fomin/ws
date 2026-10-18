//! Mapping between branch names, directories on disk and session names.

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

use crate::config::BranchDirs;

/// Reject branch names that would escape the workspace root or confuse git.
pub fn validate_branch(branch: &str) -> Result<()> {
    if branch.is_empty() {
        bail!("branch name is empty");
    }
    if branch.starts_with('-') {
        bail!("branch name {branch:?} must not start with '-'");
    }
    if branch.starts_with('/') || branch.ends_with('/') {
        bail!("branch name {branch:?} must not start or end with '/'");
    }
    if branch
        .split('/')
        .any(|c| c.is_empty() || c == "." || c == "..")
    {
        bail!("branch name {branch:?} has an empty or relative path component");
    }
    if branch
        .chars()
        .any(|c| c.is_control() || c == '\\' || c == ':')
    {
        bail!("branch name {branch:?} contains an unsupported character");
    }
    Ok(())
}

/// Directory name for a branch, relative to the workspace root.
///
/// `flat` folds `feature/x` into `feature-x`; `nested` keeps the hierarchy.
pub fn branch_dir_name(branch: &str, mode: BranchDirs) -> PathBuf {
    match mode {
        BranchDirs::Flat => PathBuf::from(branch.replace('/', "-")),
        BranchDirs::Nested => PathBuf::from(branch),
    }
}

/// `base` is the workspace's worktree root, not necessarily the config's directory.
pub fn branch_dir(base: &Path, branch: &str, mode: BranchDirs) -> PathBuf {
    base.join(branch_dir_name(branch, mode))
}

/// Worktree path of one repo for a branch.
///
/// With `grouped = false` the branch directory *is* the worktree.
pub fn worktree_path(
    base: &Path,
    branch: &str,
    mode: BranchDirs,
    grouped: bool,
    repo_dir: &Path,
) -> PathBuf {
    let dir = branch_dir(base, branch, mode);
    if grouped {
        dir.join(repo_dir)
    } else {
        dir
    }
}

/// The branch directory a worktree belongs to: its path with the repo's own
/// relative directory peeled off the end.
pub fn branch_dir_of(worktree: &Path, repo_dir: &Path, grouped: bool) -> Option<PathBuf> {
    if !grouped {
        return Some(worktree.to_path_buf());
    }
    if !worktree.ends_with(repo_dir) {
        return None;
    }
    let mut dir = worktree.to_path_buf();
    for _ in repo_dir.components() {
        if !dir.pop() {
            return None;
        }
    }
    Some(dir)
}

/// Expand `{workspace}`, `{branch}` and `{branch_raw}` in a session template.
pub fn session_name(template: &str, workspace: &str, branch: &str, mode: BranchDirs) -> String {
    let flat = branch_dir_name(branch, mode).to_string_lossy().into_owned();
    template
        .replace("{workspace}", workspace)
        .replace("{branch_raw}", branch)
        .replace("{branch}", &flat)
}

/// Fold the characters tmux reads as target separators.
///
/// `:` separates session from window and `.` window from pane, so neither can
/// survive in a session name. A slash is fine — tmux keeps it verbatim — which
/// is what lets the default `{workspace}/{branch}` show up as written.
pub fn sanitize_tmux_session(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '.' | ':' | '\\' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    if cleaned.is_empty() {
        "ws".into()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_folds_slashes() {
        assert_eq!(
            branch_dir_name("feature/JIRA-123", BranchDirs::Flat),
            PathBuf::from("feature-JIRA-123")
        );
        assert_eq!(
            branch_dir_name("main", BranchDirs::Flat),
            PathBuf::from("main")
        );
    }

    #[test]
    fn nested_keeps_hierarchy() {
        assert_eq!(
            branch_dir_name("feature/JIRA-123", BranchDirs::Nested),
            PathBuf::from("feature/JIRA-123")
        );
    }

    #[test]
    fn worktree_path_depends_on_layout() {
        let root = Path::new("/w");
        let api = Path::new("api");
        assert_eq!(
            worktree_path(root, "feature/x", BranchDirs::Flat, true, api),
            PathBuf::from("/w/feature-x/api")
        );
        assert_eq!(
            worktree_path(root, "feature/x", BranchDirs::Flat, false, api),
            PathBuf::from("/w/feature-x")
        );
        assert_eq!(
            worktree_path(root, "feature/x", BranchDirs::Nested, true, api),
            PathBuf::from("/w/feature/x/api")
        );
    }

    #[test]
    fn a_nested_repo_directory_is_honoured() {
        assert_eq!(
            worktree_path(
                Path::new("/w/trees"),
                "feature/x",
                BranchDirs::Flat,
                true,
                Path::new("services/api")
            ),
            PathBuf::from("/w/trees/feature-x/services/api")
        );
    }

    #[test]
    fn branch_dir_of_peels_the_repo_directory_back_off() {
        let wt = Path::new("/w/trees/feature-x/services/api");
        assert_eq!(
            branch_dir_of(wt, Path::new("services/api"), true),
            Some(PathBuf::from("/w/trees/feature-x"))
        );
        assert_eq!(
            branch_dir_of(Path::new("/w/feature-x"), Path::new("api"), false),
            Some(PathBuf::from("/w/feature-x"))
        );
        // A worktree that does not end with the repo directory is not ours.
        assert_eq!(branch_dir_of(wt, Path::new("web"), true), None);
    }

    #[test]
    fn session_template_expands() {
        assert_eq!(
            session_name(
                "{workspace}/{branch}",
                "acme",
                "feature/x",
                BranchDirs::Flat
            ),
            // {branch} is the branch directory name, so the slash it folded
            // stays folded; the one in the template is the template's own.
            "acme/feature-x"
        );
        assert_eq!(
            session_name("{branch_raw}", "acme", "feature/x", BranchDirs::Flat),
            "feature/x"
        );
    }

    #[test]
    fn tmux_session_keeps_slashes_and_folds_separators() {
        // ':' and '.' are tmux target separators; '/' is not.
        assert_eq!(sanitize_tmux_session("acme.web:1/x"), "acme-web-1/x");
        assert_eq!(sanitize_tmux_session("acme/feature-x"), "acme/feature-x");
        assert_eq!(sanitize_tmux_session(""), "ws");
    }

    #[test]
    fn traversal_is_rejected() {
        for bad in [
            "", "-x", "/x", "x/", "a//b", "../etc", "a/../b", "a\\b", "a:b",
        ] {
            assert!(validate_branch(bad).is_err(), "{bad:?} should be rejected");
        }
        for good in ["main", "feature/JIRA-123", "release/1.2.x"] {
            validate_branch(good).unwrap();
        }
    }
}
