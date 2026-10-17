//! Orchestration: fetch, switch, list, remove.

use anyhow::{anyhow, bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::{RepoConfig, Workspace};
use crate::exec::Runner;
use crate::git::{self, BranchSource};
use crate::mux::{Entry, SessionPlan};
use crate::paths;
use crate::setup;
use crate::state::State;

/// Clone anything missing and fetch every repo.
pub fn fetch(ws: &Workspace, runner: &Runner) -> Result<()> {
    require_repos(ws)?;
    for repo in &ws.config.repos {
        let bare = ws.bare_path(repo);
        if bare.exists() {
            eprintln!("fetching {}", repo.name());
            git::fetch(runner, &bare)?;
        } else {
            eprintln!("cloning {} -> {}", repo.url, bare.display());
            git::clone_bare(runner, &repo.url, &bare)?;
        }
    }
    Ok(())
}

fn require_repos(ws: &Workspace) -> Result<()> {
    if ws.config.repos.is_empty() {
        bail!(
            "no repositories configured in {}",
            ws.root.join(crate::config::CONFIG_FILE).display()
        );
    }
    Ok(())
}

fn ensure_bare(ws: &Workspace, runner: &Runner, repo: &RepoConfig) -> Result<PathBuf> {
    let bare = ws.bare_path(repo);
    if !bare.exists() {
        eprintln!("cloning {} -> {}", repo.url, bare.display());
        git::clone_bare(runner, &repo.url, &bare)?;
    }
    Ok(bare)
}

#[derive(Debug, Clone, Default)]
pub struct SwitchOptions {
    /// Run `setup` even for worktrees that already had it.
    pub rerun_setup: bool,
    pub skip_setup: bool,
}

#[derive(Debug, Clone)]
pub struct Switched {
    pub branch: String,
    pub branch_dir: PathBuf,
    pub worktrees: Vec<(String, PathBuf)>,
    /// Repos whose worktree was created by this call.
    pub created: Vec<String>,
}

impl Switched {
    pub fn paths(&self) -> Vec<PathBuf> {
        self.worktrees.iter().map(|(_, p)| p.clone()).collect()
    }
}

/// Create or reuse the worktree of every repo for `branch`.
pub fn switch(
    ws: &Workspace,
    runner: &Runner,
    branch: &str,
    options: &SwitchOptions,
) -> Result<Switched> {
    paths::validate_branch(branch)?;
    require_repos(ws)?;

    let grouped = ws.is_grouped();
    let mode = ws.config.branch_dirs;
    let base = ws.worktree_root();
    let branch_dir = paths::branch_dir(&base, branch, mode);
    let mut state = State::load(&ws.state_path())?;
    let mut worktrees = Vec::new();
    let mut created = Vec::new();

    for repo in &ws.config.repos {
        let name = repo.name();
        let bare = ensure_bare(ws, runner, repo)?;
        let path = paths::worktree_path(&base, branch, mode, grouped, &repo.dir());

        if path.join(".git").exists() {
            if let Some(current) = git::current_branch(runner, &path) {
                if current != branch {
                    eprintln!(
                        "warning: {} is on {current}, not {branch} ({})",
                        name,
                        path.display()
                    );
                }
            }
        } else {
            if let Some(other) = checked_out_elsewhere(runner, &bare, branch, &path)? {
                bail!(
                    "{name}: branch {branch} is already checked out at {}",
                    other.display()
                );
            }
            let source = branch_source(runner, &bare, branch, ws.config.base_branch.as_deref());
            eprintln!("creating worktree {} ({name})", path.display());
            git::worktree_add(runner, &bare, &path, branch, &source)
                .with_context(|| format!("cannot create worktree for {name}"))?;
            if source == BranchSource::Local
                && git::remote_branch_exists(runner, &bare, branch)
                && git::upstream(runner, &path).is_none()
            {
                git::set_upstream(runner, &path, branch)?;
            }
            created.push(name.clone());
        }

        // Setup runs once per worktree; a missing marker also means "not yet run",
        // so an interrupted first switch recovers on the next one.
        let commands = repo.setup_commands();
        if !options.skip_setup
            && !commands.is_empty()
            && (options.rerun_setup || !state.setup_done(branch, &name))
        {
            setup::run(runner, &path, &name, &commands)?;
            state.mark_setup_done(branch, &name);
        }

        worktrees.push((name, path));
    }

    if !runner.dry_run {
        state.save(&ws.state_path())?;
    }

    Ok(Switched {
        branch: branch.to_string(),
        branch_dir,
        worktrees,
        created,
    })
}

/// Another worktree of the same bare repo already holds this branch.
fn checked_out_elsewhere(
    runner: &Runner,
    bare: &Path,
    branch: &str,
    target: &Path,
) -> Result<Option<PathBuf>> {
    if !bare.exists() {
        return Ok(None);
    }
    Ok(git::worktrees(runner, bare)?
        .into_iter()
        .find(|w| w.branch.as_deref() == Some(branch) && w.path != target)
        .map(|w| w.path))
}

/// Local branch, remote branch to track, or a new branch off the base ref.
fn branch_source(runner: &Runner, bare: &Path, branch: &str, base: Option<&str>) -> BranchSource {
    if git::local_branch_exists(runner, bare, branch) {
        return BranchSource::Local;
    }
    if git::remote_branch_exists(runner, bare, branch) {
        return BranchSource::Track;
    }
    BranchSource::New(resolve_base(runner, bare, base))
}

fn resolve_base(runner: &Runner, bare: &Path, base: Option<&str>) -> String {
    if let Some(base) = base {
        if git::remote_branch_exists(runner, bare, base) {
            return format!("origin/{base}");
        }
        if git::local_branch_exists(runner, bare, base) {
            return base.to_string();
        }
        eprintln!("warning: base branch {base} not found, using the repo default");
    }
    match git::default_branch(runner, bare) {
        Some(head) if git::remote_branch_exists(runner, bare, &head) => format!("origin/{head}"),
        Some(head) if git::local_branch_exists(runner, bare, &head) => head,
        _ => "HEAD".to_string(),
    }
}

/// One repository inside a branch directory.
#[derive(Debug, Clone)]
pub struct RepoStatus {
    pub name: String,
    pub path: PathBuf,
    pub branch: Option<String>,
    pub dirty: bool,
}

/// A branch directory with the repos checked out in it.
#[derive(Debug, Clone)]
pub struct BranchStatus {
    pub dir: PathBuf,
    /// Branch name as git reports it, taken from the first repo that has one.
    pub branch: Option<String>,
    pub repos: Vec<RepoStatus>,
}

impl BranchStatus {
    /// Name shown by `ws list`, relative to the worktree base directory.
    pub fn dir_name(&self, base: &Path) -> String {
        self.dir
            .strip_prefix(base)
            .unwrap_or(&self.dir)
            .to_string_lossy()
            .into_owned()
    }
}

/// Every branch directory of the workspace, read back from git.
pub fn list(ws: &Workspace, runner: &Runner, with_status: bool) -> Result<Vec<BranchStatus>> {
    let grouped = ws.is_grouped();
    let mut by_dir: BTreeMap<PathBuf, Vec<RepoStatus>> = BTreeMap::new();

    for repo in &ws.config.repos {
        let name = repo.name();
        let repo_dir = repo.dir();
        let bare = ws.bare_path(repo);
        if !bare.exists() {
            continue;
        }
        for wt in git::worktrees(runner, &bare)? {
            let Some(dir) = paths::branch_dir_of(&wt.path, &repo_dir, grouped) else {
                continue;
            };
            let dirty = with_status && git::is_dirty(runner, &wt.path);
            by_dir.entry(dir).or_default().push(RepoStatus {
                name: name.clone(),
                path: wt.path,
                branch: wt.branch,
                dirty,
            });
        }
    }

    Ok(by_dir
        .into_iter()
        .map(|(dir, repos)| BranchStatus {
            branch: repos.iter().find_map(|r| r.branch.clone()),
            dir,
            repos,
        })
        .collect())
}

/// Union of `origin/*` across all repos, minus branches already checked out.
pub fn available_branches(ws: &Workspace, runner: &Runner) -> Result<Vec<String>> {
    let mut all: Vec<String> = Vec::new();
    for repo in &ws.config.repos {
        let bare = ws.bare_path(repo);
        if !bare.exists() {
            continue;
        }
        for branch in git::remote_branches(runner, &bare)? {
            if !all.contains(&branch) {
                all.push(branch);
            }
        }
    }
    all.sort();
    Ok(all)
}

pub fn remove(ws: &Workspace, runner: &Runner, branch: &str, force: bool) -> Result<Switched> {
    paths::validate_branch(branch)?;
    let grouped = ws.is_grouped();
    let mode = ws.config.branch_dirs;
    let base = ws.worktree_root();
    let branch_dir = paths::branch_dir(&base, branch, mode);
    let mut worktrees = Vec::new();
    let mut dirty = Vec::new();

    for repo in &ws.config.repos {
        let name = repo.name();
        let path = paths::worktree_path(&base, branch, mode, grouped, &repo.dir());
        if path.exists() && git::is_dirty(runner, &path) {
            dirty.push(name.clone());
        }
        worktrees.push((name, path));
    }

    if !dirty.is_empty() && !force {
        bail!(
            "uncommitted changes in {} — commit them or pass --force",
            dirty.join(", ")
        );
    }

    for (repo, path) in &worktrees {
        let config = ws
            .config
            .repos
            .iter()
            .find(|r| &r.name() == repo)
            .ok_or_else(|| anyhow!("unknown repo {repo}"))?;
        let bare = ws.bare_path(config);
        if !bare.exists() {
            continue;
        }
        if path.exists() {
            git::worktree_remove(runner, &bare, path, force)?;
        }
        git::worktree_prune(runner, &bare)?;
    }

    if !runner.dry_run {
        // A nested repo path such as `services/api` leaves its parents behind.
        for (_, path) in &worktrees {
            if let Some(parent) = path.parent() {
                remove_empty_dirs(parent, &branch_dir);
            }
        }
        // Only when nothing else lives there.
        let _ = std::fs::remove_dir(&branch_dir);
    }

    let mut state = State::load(&ws.state_path())?;
    state.forget(branch);
    if !runner.dry_run {
        state.save(&ws.state_path())?;
    }

    Ok(Switched {
        branch: branch.to_string(),
        branch_dir,
        worktrees,
        created: Vec::new(),
    })
}

/// Remove `from` and its parents while they are empty, stopping at `until`.
fn remove_empty_dirs(from: &Path, until: &Path) {
    let mut current = from.to_path_buf();
    while current != until && current.starts_with(until) {
        if std::fs::remove_dir(&current).is_err() {
            return;
        }
        if !current.pop() {
            return;
        }
    }
}

/// Turn a completed switch into the multiplexer plan.
pub fn session_plan(ws: &Workspace, switched: &Switched) -> SessionPlan {
    let session = paths::session_name(
        &ws.config.mux.session,
        &ws.name(),
        &switched.branch,
        ws.config.branch_dirs,
    );
    let root_window = ws.config.mux.root_window.trim();
    // A shared window only earns its place when there are several repos.
    let root_window =
        (!root_window.is_empty() && switched.worktrees.len() > 1).then(|| root_window.to_string());

    SessionPlan {
        session,
        root_dir: switched.branch_dir.clone(),
        root_window,
        entries: switched
            .worktrees
            .iter()
            .map(|(name, path)| Entry {
                name: name.clone(),
                path: path.clone(),
            })
            .collect(),
    }
}

pub fn session_name_for(ws: &Workspace, branch: &str) -> String {
    paths::session_name(
        &ws.config.mux.session,
        &ws.name(),
        branch,
        ws.config.branch_dirs,
    )
}
