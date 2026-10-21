//! Thin wrappers over the `git` CLI.
//!
//! Shelling out (rather than linking libgit2) means ssh-agent, credential
//! helpers, hooks, LFS and the user's own git config all keep working.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::exec::{Cmd, Runner};

/// One entry of `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// Short branch name, `None` when detached.
    pub branch: Option<String>,
    pub bare: bool,
    pub detached: bool,
}

fn git(dir: &Path) -> Cmd {
    Cmd::new("git").arg("-C").path_arg(dir)
}

/// Clone as a bare repository and wire up the `origin` fetch refspec.
///
/// `git clone --bare` deliberately omits `remote.origin.fetch`, so without this
/// a later `git fetch` never populates `refs/remotes/origin/*` and no remote
/// branch can be resolved — the classic bare+worktree trap.
pub fn clone_bare(runner: &Runner, url: &str, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        if !runner.dry_run {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
    }
    runner.run(
        &Cmd::new("git")
            .args(["clone", "--bare", url])
            .path_arg(dest),
    )?;
    runner.run(
        &git(dest)
            .args(["config", "remote.origin.fetch"])
            .arg("+refs/heads/*:refs/remotes/origin/*"),
    )?;
    fetch(runner, dest)
}

pub fn fetch(runner: &Runner, bare: &Path) -> Result<()> {
    runner.run(&git(bare).args(["fetch", "--prune", "origin"]))
}

pub fn parse_worktree_list(out: &str) -> Vec<Worktree> {
    let mut list = Vec::new();
    let mut current: Option<Worktree> = None;
    for line in out.lines() {
        if line.is_empty() {
            list.extend(current.take());
            continue;
        }
        let (key, value) = match line.split_once(' ') {
            Some((k, v)) => (k, v),
            None => (line, ""),
        };
        match key {
            "worktree" => {
                list.extend(current.take());
                current = Some(Worktree {
                    path: PathBuf::from(value),
                    branch: None,
                    bare: false,
                    detached: false,
                });
            }
            "branch" => {
                if let Some(w) = current.as_mut() {
                    w.branch = Some(value.trim_start_matches("refs/heads/").to_string());
                }
            }
            "bare" => {
                if let Some(w) = current.as_mut() {
                    w.bare = true;
                }
            }
            "detached" => {
                if let Some(w) = current.as_mut() {
                    w.detached = true;
                }
            }
            _ => {}
        }
    }
    list.extend(current.take());
    list
}

/// Worktrees of a bare repo, excluding the bare repo itself.
pub fn worktrees(runner: &Runner, bare: &Path) -> Result<Vec<Worktree>> {
    let out = runner.capture(&git(bare).args(["worktree", "list", "--porcelain"]))?;
    Ok(parse_worktree_list(&out)
        .into_iter()
        .filter(|w| !w.bare)
        .collect())
}

pub fn local_branch_exists(runner: &Runner, bare: &Path, branch: &str) -> bool {
    runner.ok(&git(bare).args([
        "show-ref",
        "--verify",
        "--quiet",
        &format!("refs/heads/{branch}"),
    ]))
}

pub fn remote_branch_exists(runner: &Runner, bare: &Path, branch: &str) -> bool {
    runner.ok(&git(bare).args([
        "show-ref",
        "--verify",
        "--quiet",
        &format!("refs/remotes/origin/{branch}"),
    ]))
}

/// Branch the bare clone's HEAD points at, i.e. the remote's default branch.
pub fn default_branch(runner: &Runner, bare: &Path) -> Option<String> {
    let out = runner
        .try_capture(&git(bare).args(["symbolic-ref", "--short", "HEAD"]))
        .ok()??;
    let name = out.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Every `origin/*` branch known to the bare clone.
pub fn remote_branches(runner: &Runner, bare: &Path) -> Result<Vec<String>> {
    let out = runner.capture(&git(bare).args([
        "for-each-ref",
        "--format=%(refname:strip=3)",
        "refs/remotes/origin",
    ]))?;
    Ok(out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != "HEAD")
        .map(str::to_string)
        .collect())
}

/// Tag names of the repository.
pub fn tags(runner: &Runner, bare: &Path) -> Result<Vec<String>> {
    let out = runner.capture(&git(bare).args([
        "for-each-ref",
        "--format=%(refname:strip=2)",
        "refs/tags",
    ]))?;
    Ok(out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

/// Whether `rev` names something this repository can start a branch from —
/// a tag, a commit, `HEAD~2`, a ref of another remote.
pub fn rev_exists(runner: &Runner, bare: &Path, rev: &str) -> bool {
    runner.ok(&git(bare).args([
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("{rev}^{{commit}}"),
    ]))
}

/// How the branch should be created when adding a worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchSource {
    /// Branch already exists locally.
    Local,
    /// Create a local branch tracking `origin/<branch>`.
    Track,
    /// Create a new branch from this ref.
    New(String),
}

/// The `git worktree add` invocation for a branch; kept pure so its shape can
/// be asserted in tests.
pub fn worktree_add_cmd(bare: &Path, path: &Path, branch: &str, source: &BranchSource) -> Cmd {
    let base = git(bare).args(["worktree", "add"]);
    match source {
        BranchSource::Local => base.path_arg(path).arg(branch),
        BranchSource::Track => base
            .arg("--track")
            .arg("-b")
            .arg(branch)
            .path_arg(path)
            .arg(format!("origin/{branch}")),
        BranchSource::New(start) => base.arg("-b").arg(branch).path_arg(path).arg(start),
    }
}

pub fn worktree_add(
    runner: &Runner,
    bare: &Path,
    path: &Path,
    branch: &str,
    source: &BranchSource,
) -> Result<()> {
    runner.run(&worktree_add_cmd(bare, path, branch, source))
}

pub fn worktree_remove(runner: &Runner, bare: &Path, path: &Path, force: bool) -> Result<()> {
    let mut cmd = git(bare).args(["worktree", "remove"]);
    if force {
        cmd = cmd.arg("--force");
    }
    runner.run(&cmd.path_arg(path))
}

pub fn worktree_prune(runner: &Runner, bare: &Path) -> Result<()> {
    runner.run(&git(bare).args(["worktree", "prune"]))
}

/// Upstream branch configured for the worktree's HEAD, if any.
pub fn upstream(runner: &Runner, worktree: &Path) -> Option<String> {
    let out = runner
        .try_capture(&git(worktree).args(["rev-parse", "--abbrev-ref", "@{u}"]))
        .ok()??;
    let name = out.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Point a branch at `origin/<branch>`.
///
/// A bare clone copies every remote branch into `refs/heads/*`, so worktrees
/// created from one start out with no upstream; this restores what a normal
/// clone would have configured.
pub fn set_upstream(runner: &Runner, worktree: &Path, branch: &str) -> Result<()> {
    runner.run(&git(worktree).args([
        "branch",
        &format!("--set-upstream-to=origin/{branch}"),
        branch,
    ]))
}

/// Whether the worktree has anything of the user's in it.
///
/// `ws`'s own branch marker does not count: with a single repository the
/// branch directory is the worktree, so the marker sits among the files
/// there, and it is not a reason to call the worktree dirty or to refuse to
/// remove it.
/// Whether a worktree has changes of its own — not counting what `ws` puts
/// there itself: the branch marker, and `own`, the logs directory of a branch
/// directory that is also the worktree.
pub fn is_dirty(runner: &Runner, worktree: &Path, own: Option<&Path>) -> bool {
    let marker = format!("?? {}", crate::marker::BRANCH_FILE);
    let own = own.map(|dir| format!("?? {}/", dir.to_string_lossy()));
    match runner.try_capture(&git(worktree).args(["status", "--porcelain"])) {
        Ok(Some(out)) => out.lines().any(|line| {
            let line = line.trim();
            !line.is_empty() && line != marker && Some(line) != own.as_deref()
        }),
        _ => false,
    }
}

pub fn current_branch(runner: &Runner, worktree: &Path) -> Option<String> {
    let out = runner
        .try_capture(&git(worktree).args(["rev-parse", "--abbrev-ref", "HEAD"]))
        .ok()??;
    let name = out.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// `(ahead, behind)` relative to the upstream branch, if one is configured.
pub fn ahead_behind(runner: &Runner, worktree: &Path) -> Option<(u32, u32)> {
    let out = runner
        .try_capture(&git(worktree).args(["rev-list", "--left-right", "--count", "@{u}...HEAD"]))
        .ok()??;
    let mut parts = out.split_whitespace();
    let behind = parts.next()?.parse().ok()?;
    let ahead = parts.next()?.parse().ok()?;
    Some((ahead, behind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_output() {
        let out = "\
worktree /w/.bare/api.git
bare

worktree /w/main/api
HEAD abc123
branch refs/heads/main

worktree /w/feature-x/api
HEAD def456
detached

";
        let list = parse_worktree_list(out);
        assert_eq!(list.len(), 3);
        assert!(list[0].bare);
        assert_eq!(list[1].path, PathBuf::from("/w/main/api"));
        assert_eq!(list[1].branch.as_deref(), Some("main"));
        assert!(list[2].detached);
        assert_eq!(list[2].branch, None);
    }

    #[test]
    fn parses_output_without_trailing_blank_line() {
        let out = "worktree /w/main\nHEAD abc\nbranch refs/heads/feature/x";
        let list = parse_worktree_list(out);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].branch.as_deref(), Some("feature/x"));
    }

    #[test]
    fn worktree_add_builds_expected_commands() {
        let bare = Path::new("/w/.bare/api.git");
        let path = Path::new("/w/feature-x/api");
        assert_eq!(
            worktree_add_cmd(bare, path, "feature/x", &BranchSource::Local).display(),
            "git -C /w/.bare/api.git worktree add /w/feature-x/api feature/x"
        );
        let track = worktree_add_cmd(bare, path, "feature/x", &BranchSource::Track);
        assert_eq!(
            track.args,
            [
                "-C",
                "/w/.bare/api.git",
                "worktree",
                "add",
                "--track",
                "-b",
                "feature/x",
                "/w/feature-x/api",
                "origin/feature/x",
            ]
        );
        assert_eq!(
            worktree_add_cmd(
                bare,
                path,
                "feature/x",
                &BranchSource::New("origin/main".into())
            )
            .display(),
            "git -C /w/.bare/api.git worktree add -b feature/x /w/feature-x/api origin/main"
        );
    }
}
