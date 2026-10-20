//! Orchestration: fetch, switch, list, remove.

use anyhow::{anyhow, bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::{RepoConfig, Workspace};
use crate::exec::Runner;
use crate::git::{self, BranchSource};
use crate::integration::{Local, PackageManagers};
use crate::marker::{self, Marker};
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
    /// Base for a branch that has to be created, overriding `base_branch`.
    pub from: Option<Start>,
    /// Branch whose worktrees a package manager integration seeds the new
    /// ones from: the branch of the directory `ws switch` ran in. It falls
    /// back to the default branch, so this may be `None`.
    pub seed_from: Option<String>,
}

/// Where a branch that does not exist yet starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Start {
    pub reference: String,
    pub source: StartSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartSource {
    /// `--from`: the user named it, so every repository has to have it.
    Flag,
    /// The branch of the directory `ws switch` ran in. Taken as the local
    /// branch, which is what is actually in front of you — `origin/` may be
    /// behind it — and a repository without it falls back to the base.
    CurrentBranch,
}

impl Start {
    fn is_flag(&self) -> bool {
        self.source == StartSource::Flag
    }
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
    if let Some(start) = &options.from {
        // Before any worktree is created, so a ref that only some of the
        // repositories have cannot leave the branch half made.
        require_start(ws, runner, branch, start)?;
    }

    let grouped = ws.is_grouped();
    let mode = ws.config.branch_dirs;
    let base = ws.worktree_root();
    let branch_dir = paths::branch_dir(&base, branch, mode);
    let mut state = State::load(&ws.state_path())?;
    let mut worktrees = Vec::new();
    let mut created = Vec::new();
    let mut started_from: Option<String> = None;
    let managers = PackageManagers::new(&ws.config.integrations);
    let local = Local::new(ws);
    // With the branch directories in the workspace root, a branch can share
    // its name with the local directory — and would be checked out into it.
    if branch_dir == ws.local_dir()
        && branch_dir.exists()
        && !branch_dir.join(".git").exists()
        && Marker::load(&branch_dir).is_none()
    {
        bail!(
            "branch {branch} would be checked out into the local files directory {} — \
             set integrations.local.dir to somewhere else",
            branch_dir.display()
        );
    }

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
            let source = branch_source(
                runner,
                &bare,
                &name,
                branch,
                options.from.as_ref(),
                ws.config.base_branch.as_deref(),
            );
            if let Some(start) = &options.from {
                if start.is_flag() && !matches!(source, BranchSource::New(_)) {
                    eprintln!("warning: {name}: {branch} exists already, --from is not used");
                }
            }
            if started_from.is_none() {
                started_from = match &source {
                    BranchSource::New(start) => Some(start.clone()),
                    // The branch came from the remote, which is where it
                    // started as far as this workspace is concerned.
                    BranchSource::Track => Some(format!("origin/{branch}")),
                    // It was already here: `ws` did not start it anywhere.
                    BranchSource::Local => None,
                };
            }
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

        // The local files first — an `.npmrc` may be what lets the install
        // reach its registry — then the dependencies, then whatever else the
        // repository needs: a `setup` command is written against a worktree
        // it can build in.
        if !options.skip_setup {
            local.place(runner, repo, &path, created.contains(&name));
            let seeds = seed_worktrees(ws, options.seed_from.as_deref(), repo, &path);
            managers.prepare(runner, &name, &path, &seeds, options.rerun_setup);
        }

        // Setup runs once per worktree; a missing marker also means "not yet run",
        // so an interrupted first switch recovers on the next one.
        let commands = repo.setup_commands();
        if !options.skip_setup
            && !commands.is_empty()
            && (options.rerun_setup || !state.setup_done(branch, &name))
        {
            let context = setup::Context {
                root: &ws.root,
                branch,
                branch_dir: &branch_dir,
                worktree: &path,
                repo: &name,
            };
            setup::run(runner, &context, &commands)?;
            state.mark_setup_done(branch, &name);
        }

        worktrees.push((name, path));
    }

    if !runner.dry_run {
        mark_branch_dir(ws, branch, &branch_dir, started_from)?;
        state.save(&ws.state_path())?;
    }

    Ok(Switched {
        branch: branch.to_string(),
        branch_dir,
        worktrees,
        created,
    })
}

/// Worktrees of `repo` a new one's dependencies can be hard-linked from, in
/// the order they are tried: the branch `ws switch` ran in, then the default
/// branch.
///
/// The branch you came from first, because it is the one whose dependencies
/// are most likely to be the ones the new branch wants — a branch forked from
/// it resolves the same lockfile. The default branch is the fallback for a
/// switch from anywhere else.
fn seed_worktrees(
    ws: &Workspace,
    seed_from: Option<&str>,
    repo: &RepoConfig,
    target: &Path,
) -> Vec<PathBuf> {
    let base = ws.worktree_root();
    let mode = ws.config.branch_dirs;
    let grouped = ws.is_grouped();
    let mut seeds = Vec::new();

    for branch in seed_from
        .into_iter()
        .chain([ws.config.default_branch.as_str()])
    {
        if paths::validate_branch(branch).is_err() {
            continue;
        }
        let path = paths::worktree_path(&base, branch, mode, grouped, &repo.dir());
        if path != target && !seeds.contains(&path) && path.is_dir() {
            seeds.push(path);
        }
    }
    seeds
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
/// Leave the marker that makes this a branch directory — written once, by
/// the call that created it, so what the branch started from is not rewritten
/// by a later switch that re-creates one repository's worktree.
///
/// Only where the branch directory holds the repositories rather than *being*
/// one. With a single repository it is that repository's working tree, and a
/// file of `ws`'s there is a file in the user's project: `git add .` commits
/// it, `git status` reports it, and keeping it quiet would mean writing into
/// git's own directory or into the project's `.gitignore` — neither of which
/// is `ws`'s to touch. Nothing is lost by leaving it out: a worktree already
/// says which branch it is on, and [`current_branch_name`] asks it.
fn mark_branch_dir(
    ws: &Workspace,
    branch: &str,
    branch_dir: &Path,
    from: Option<String>,
) -> Result<()> {
    if !ws.is_grouped() || Marker::load(branch_dir).is_some() {
        return Ok(());
    }
    Marker {
        branch: branch.to_string(),
        from,
    }
    .save(branch_dir)
}

/// Every repository has to be able to reach an explicit `--from`, or the
/// branch would mean something different in each of them.
fn require_start(ws: &Workspace, runner: &Runner, branch: &str, start: &Start) -> Result<()> {
    if !start.is_flag() {
        return Ok(());
    }
    let grouped = ws.is_grouped();
    let mode = ws.config.branch_dirs;
    let base = ws.worktree_root();
    let mut missing = Vec::new();

    for repo in &ws.config.repos {
        let bare = ensure_bare(ws, runner, repo)?;
        let path = paths::worktree_path(&base, branch, mode, grouped, &repo.dir());
        // Nothing is created where the worktree is already there, or where
        // the branch exists and is checked out or tracked instead.
        if path.join(".git").exists()
            || git::local_branch_exists(runner, &bare, branch)
            || git::remote_branch_exists(runner, &bare, branch)
        {
            continue;
        }
        if resolve_ref(runner, &bare, start).is_none() {
            missing.push(repo.name());
        }
    }

    if !missing.is_empty() {
        bail!(
            "--from {} is not in {} — fetch it, or name a ref every repository has",
            start.reference,
            missing.join(", ")
        );
    }
    Ok(())
}

fn branch_source(
    runner: &Runner,
    bare: &Path,
    repo: &str,
    branch: &str,
    from: Option<&Start>,
    base: Option<&str>,
) -> BranchSource {
    if git::local_branch_exists(runner, bare, branch) {
        return BranchSource::Local;
    }
    if git::remote_branch_exists(runner, bare, branch) {
        return BranchSource::Track;
    }
    BranchSource::New(resolve_base(runner, bare, repo, from, base))
}

/// What a new branch starts from: `--from` when it names something this
/// repository has, then `base_branch`, then the repository's own default.
fn resolve_base(
    runner: &Runner,
    bare: &Path,
    repo: &str,
    from: Option<&Start>,
    base: Option<&str>,
) -> String {
    if let Some(start) = from {
        if let Some(resolved) = resolve_ref(runner, bare, start) {
            return resolved;
        }
        // An explicit `--from` never gets here: it is checked against every
        // repository first. The branch of the current directory does, and a
        // repository that has never seen it starts from the base instead.
        eprintln!(
            "warning: {repo}: {} is not here, starting from the base branch",
            start.reference
        );
    }
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

/// `--from` as this repository can reach it: `origin/x` as written, a branch
/// name as the remote one when there is one, and anything else — a tag, a
/// commit, `HEAD~2` — as given.
fn resolve_ref(runner: &Runner, bare: &Path, start: &Start) -> Option<String> {
    let reference = start.reference.as_str();
    if let Some(branch) = reference.strip_prefix("origin/") {
        return git::remote_branch_exists(runner, bare, branch).then(|| reference.to_string());
    }
    let local = git::local_branch_exists(runner, bare, reference);
    if local && !start.is_flag() {
        return Some(reference.to_string());
    }
    if git::remote_branch_exists(runner, bare, reference) {
        return Some(format!("origin/{reference}"));
    }
    if local {
        return Some(reference.to_string());
    }
    git::rev_exists(runner, bare, reference).then(|| reference.to_string())
}

/// The branch directory the path is inside, read from the marker it carries:
/// walking up from the path, no git, no listing of worktrees.
///
/// The marker has to name the directory it is in — a branch directory of
/// *this* workspace, at the place that branch belongs — so a stray `.ws.yaml`
/// somewhere in a repository's own files cannot pass for one.
pub fn current_branch_dir(ws: &Workspace, path: &Path) -> Option<(PathBuf, Marker)> {
    let (dir, marker) = Marker::find_upward(path)?;
    let expected = paths::branch_dir(&ws.worktree_root(), &marker.branch, ws.config.branch_dirs);
    (expected == dir).then_some((dir, marker))
}

/// The branch of the directory `path` sits in, found by walking up from it.
///
/// Two markers do that, and git has the last word in both cases, since a
/// worktree can be moved to another branch after `ws` made it:
///
/// * the `.ws.yaml` of a branch directory that holds repositories,
/// * the `.git` of a worktree, which is what a branch directory is when there
///   is only one repository — and what the path passes through anyway when it
///   is inside one of several.
///
/// A branch directory from a `ws` that predates the marker still answers
/// through the second of those, or through the worktree listing.
pub fn current_branch_name(ws: &Workspace, runner: &Runner, path: &Path) -> Option<String> {
    if let Some((dir, marker)) = current_branch_dir(ws, path) {
        let base = ws.worktree_root();
        for repo in &ws.config.repos {
            let worktree = paths::worktree_path(
                &base,
                &marker.branch,
                ws.config.branch_dirs,
                ws.is_grouped(),
                &repo.dir(),
            );
            if worktree.join(".git").exists() {
                if let Some(branch) = git::current_branch(runner, &worktree) {
                    return Some(branch);
                }
            }
        }
        let _ = dir;
        return Some(marker.branch);
    }

    if let Some(worktree) = nearest_worktree(ws, path) {
        if let Some(branch) = git::current_branch(runner, &worktree) {
            return Some(branch);
        }
    }

    current_branch(ws, runner, path)?.branch
}

/// Nearest worktree at or above `path`, as long as it is one this workspace
/// keeps its branches in.
fn nearest_worktree(ws: &Workspace, path: &Path) -> Option<PathBuf> {
    let base = ws.worktree_root();
    let mut dir = Some(path);
    while let Some(current) = dir {
        if !current.starts_with(&base) {
            return None;
        }
        if current.join(".git").exists() {
            return Some(current.to_path_buf());
        }
        dir = current.parent();
    }
    None
}

/// The branch directory the path is inside, with the repositories in it —
/// read back from git, which is what `ws status` reports.
pub fn current_branch(ws: &Workspace, runner: &Runner, path: &Path) -> Option<BranchStatus> {
    list(ws, runner, false)
        .ok()?
        .into_iter()
        .find(|entry| path.starts_with(&entry.dir))
}

/// Every tag and branch of the workspace, deduplicated, as `--from` takes
/// them. The `origin/` forms are only offered once that prefix is typed:
/// they are the same branches again, and double the list for nothing.
pub fn ref_choices(ws: &Workspace, runner: &Runner, word: &str) -> Result<Vec<BranchChoice>> {
    if let Some(typed) = word.strip_prefix("origin/") {
        let _ = typed;
        let mut remote = Vec::new();
        for repo in &ws.config.repos {
            let bare = ws.bare_path(repo);
            if !bare.exists() {
                continue;
            }
            for branch in git::remote_branches(runner, &bare)? {
                let name = format!("origin/{branch}");
                match remote
                    .iter_mut()
                    .find(|c: &&mut BranchChoice| c.branch == name)
                {
                    Some(choice) => choice.repos.push(repo.name()),
                    None => remote.push(BranchChoice {
                        branch: name,
                        kind: ChoiceKind::Branch,
                        repos: vec![repo.name()],
                    }),
                }
            }
        }
        remote.sort_by(|a, b| a.branch.cmp(&b.branch));
        return Ok(remote);
    }

    let mut choices = branch_choices(ws, runner)?;
    let branches = choices.len();
    for repo in &ws.config.repos {
        let bare = ws.bare_path(repo);
        if !bare.exists() {
            continue;
        }
        for tag in git::tags(runner, &bare)? {
            match choices[branches..].iter_mut().find(|c| c.branch == tag) {
                Some(choice) => choice.repos.push(repo.name()),
                None => choices.push(BranchChoice {
                    branch: tag,
                    kind: ChoiceKind::Tag,
                    repos: vec![repo.name()],
                }),
            }
        }
    }
    choices[branches..].sort_by(|a, b| a.branch.cmp(&b.branch));
    Ok(choices)
}

/// A branch or tag `ws switch` could be given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchChoice {
    pub branch: String,
    pub kind: ChoiceKind,
    /// Repositories it is in, or the ones with a worktree for it.
    pub repos: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceKind {
    /// A branch with a worktree in this workspace.
    CheckedOut,
    Branch,
    Tag,
}

/// Every branch of the workspace, deduplicated across its repositories: the
/// ones with worktrees first, then the `origin/*` branches without — the same
/// two sources, in the same order, as the fzf picker.
pub fn branch_choices(ws: &Workspace, runner: &Runner) -> Result<Vec<BranchChoice>> {
    let mut choices: Vec<BranchChoice> = Vec::new();
    for entry in list(ws, runner, false)? {
        let Some(branch) = entry.branch else {
            continue;
        };
        if choices.iter().any(|c| c.branch == branch) {
            continue;
        }
        choices.push(BranchChoice {
            branch,
            kind: ChoiceKind::CheckedOut,
            repos: entry.repos.into_iter().map(|r| r.name).collect(),
        });
    }

    let checked_out = choices.len();
    for repo in &ws.config.repos {
        let bare = ws.bare_path(repo);
        if !bare.exists() {
            continue;
        }
        for branch in git::remote_branches(runner, &bare)? {
            if choices[..checked_out].iter().any(|c| c.branch == branch) {
                continue;
            }
            match choices[checked_out..]
                .iter_mut()
                .find(|c| c.branch == branch)
            {
                Some(choice) => choice.repos.push(repo.name()),
                None => choices.push(BranchChoice {
                    branch,
                    kind: ChoiceKind::Branch,
                    repos: vec![repo.name()],
                }),
            }
        }
    }

    choices[checked_out..].sort_by(|a, b| a.branch.cmp(&b.branch));
    Ok(choices)
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

    // Before git is asked to remove anything: with a single repository the
    // marker lies inside the worktree, where it would both count as an
    // untracked file and keep the directory from being removed.
    if !runner.dry_run {
        let _ = std::fs::remove_file(branch_dir.join(marker::BRANCH_FILE));
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
