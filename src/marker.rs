//! `.ws.yaml` — the marker `ws` leaves in every branch directory.
//!
//! It is what makes a branch directory recognisable by walking up from any
//! path inside it, without asking git anything, and it records the one thing
//! git cannot be asked for afterwards: the ref the branch started at.
//!
//! Where the branch directory *is* a worktree — a single-repo workspace — the
//! file is added to the repository's `info/exclude`, which is shared by every
//! worktree of that bare repo, so it never shows up in `git status`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const BRANCH_FILE: &str = ".ws.yaml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub branch: String,
    /// The ref the branch was created from, when `ws` created it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

impl Marker {
    /// The marker of `dir`, if it has a readable one. A file that cannot be
    /// parsed is treated as absent: callers fall back to asking git, which is
    /// the truth anyway.
    pub fn load(dir: &Path) -> Option<Marker> {
        let text = std::fs::read_to_string(dir.join(BRANCH_FILE)).ok()?;
        serde_yaml_ng::from_str(&text).ok()
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        let path = dir.join(BRANCH_FILE);
        let text = serde_yaml_ng::to_string(self).context("cannot serialize the branch marker")?;
        std::fs::write(&path, text).with_context(|| format!("cannot write {}", path.display()))
    }

    /// Nearest directory at or above `start` that carries a marker.
    ///
    /// The walk goes all the way up: a workspace can keep its worktrees
    /// outside its own root. It is the caller that decides whether the
    /// directory found belongs to the workspace in hand.
    pub fn find_upward(start: &Path) -> Option<(PathBuf, Marker)> {
        let mut dir = Some(start);
        while let Some(current) = dir {
            if let Some(marker) = Marker::load(current) {
                return Some((current.to_path_buf(), marker));
            }
            dir = current.parent();
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn round_trips_and_is_found_from_below() {
        let tmp = TempDir::new().unwrap();
        let branch_dir = tmp.path().join("feature-x");
        let deep = branch_dir.join("api/src");
        std::fs::create_dir_all(&deep).unwrap();

        Marker {
            branch: "feature/x".into(),
            from: Some("origin/main".into()),
        }
        .save(&branch_dir)
        .unwrap();

        let (found, marker) = Marker::find_upward(&deep).unwrap();
        assert_eq!(found, branch_dir);
        assert_eq!(marker.branch, "feature/x");
        assert_eq!(marker.from.as_deref(), Some("origin/main"));
    }

    #[test]
    fn a_directory_without_one_finds_nothing_below_the_root() {
        let tmp = TempDir::new().unwrap();
        assert!(Marker::load(tmp.path()).is_none());
    }

    #[test]
    fn an_unreadable_marker_is_no_marker() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(BRANCH_FILE), "{ this is not yaml").unwrap();
        assert!(Marker::load(tmp.path()).is_none());
    }
}
