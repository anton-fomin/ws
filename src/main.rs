//! `ws` — multi-repo git worktree workspaces.

mod cli;
mod config;
mod exec;
mod git;
mod integration;
mod logs;
mod marker;
mod mux;
mod paths;
mod pick;
mod run;
mod setup;
mod state;
mod workspace;

use anyhow::{bail, Context, Result};
use clap::Parser;
use std::path::Path;

use cli::{Cli, Command};
use config::{Backend, Config, PickerMode, Workspace, CONFIG_FILE};
use exec::{Cmd, Runner};
use integration::{Local, PackageManagers, Tasks, Zoxide};
use pick::{Candidate, Picker};
use workspace::SwitchOptions;

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let runner = Runner::new(cli.dry_run, cli.verbose);
    let force = cli.mux.map(Backend::from);
    let cwd = std::env::current_dir().context("cannot read the current directory")?;

    match cli.command.unwrap_or(Command::Pick {
        global: false,
        inline: false,
        query: Vec::new(),
    }) {
        Command::Init {
            name,
            repos,
            global,
            force,
        } => {
            if global {
                cmd_init_global(&runner, force)
            } else {
                cmd_init(&runner, &cwd, name, &repos, force)
            }
        }
        Command::Fetch => {
            let ws = Workspace::discover(&cwd)?;
            workspace::fetch(&ws, &runner)
        }
        Command::Switch {
            branch: Some(branch),
            rerun_setup,
            no_setup,
            cd,
            from,
        } => {
            let ws = Workspace::discover(&cwd)?;
            let here = workspace::current_branch_name(&ws, &runner, &cwd);
            let options = SwitchOptions {
                rerun_setup,
                skip_setup: no_setup,
                from: start_from(here.clone(), from),
                seed_from: here,
            };
            switch_and_open(&ws, &runner, &branch, &options, force, cd)
        }
        Command::Switch { branch: None, .. } => cmd_pick(&runner, force, &cwd, false, false, &[]),
        Command::Pick {
            global,
            inline,
            query,
        } => cmd_pick(&runner, force, &cwd, global, inline, &query),
        Command::List => cmd_list(&runner, &cwd),
        Command::Path { branch, repo } => cmd_path(&cwd, &branch, repo.as_deref()),
        Command::Rm { branch, force: f } => cmd_rm(&runner, force, &cwd, &branch, f),
        Command::Run {
            targets,
            branch,
            logs,
            no_logs,
            args,
        } => {
            let logs = match (logs, no_logs) {
                (true, _) => Some(true),
                (_, true) => Some(false),
                _ => None,
            };
            cmd_run(&runner, &cwd, &targets, branch.as_deref(), logs, &args)
        }
        Command::Status => cmd_status(&runner, &cwd),
        Command::ShellInit { shell } => {
            print!("{}", cli::shell_init(shell));
            Ok(())
        }
        Command::Completions { shell } => {
            cli::completions(shell, &mut std::io::stdout());
            Ok(())
        }
        Command::Skill => {
            print!("{SKILL}");
            Ok(())
        }
        Command::Complete { what, word } => {
            cmd_complete(&runner, &cwd, what, word.as_deref().unwrap_or_default())
        }
        Command::Preview { payload } => {
            let ws = Workspace::discover(&cwd).ok();
            print!("{}", pick::preview(ws.as_ref(), &runner, &payload));
            Ok(())
        }
    }
}

/// Candidates for the shell completions. A shell asks this on every <Tab>, in
/// whatever directory the prompt happens to sit in, so anything that is not a
/// workspace — or a workspace git cannot read right now — is an empty list
/// rather than an error.
fn cmd_complete(runner: &Runner, cwd: &Path, what: cli::CompleteArg, word: &str) -> Result<()> {
    let Ok(ws) = Workspace::discover(cwd) else {
        return Ok(());
    };
    match what {
        cli::CompleteArg::Worktrees => {
            let base = ws.worktree_root();
            for entry in workspace::list(&ws, runner, false).unwrap_or_default() {
                let dir = entry.dir_name(&base);
                let Some(branch) = entry.branch else {
                    continue;
                };
                let repos: Vec<&str> = entry.repos.iter().map(|r| r.name.as_str()).collect();
                let repos = repos.join(", ");
                // The directory is what `ws rm` deletes, and naming it also
                // keeps the descriptions distinct, which is what stops zsh
                // from folding the branches into one line.
                if dir == branch {
                    println!("{branch}\t{repos}");
                } else {
                    println!("{branch}\t{dir} — {repos}");
                }
            }
        }
        cli::CompleteArg::Branches => {
            describe(workspace::branch_choices(&ws, runner).unwrap_or_default())
        }
        cli::CompleteArg::Refs => {
            describe(workspace::ref_choices(&ws, runner, word).unwrap_or_default())
        }
        cli::CompleteArg::Tasks => {
            let Ok(entry) = run_branch(&ws, runner, cwd, None) else {
                return Ok(());
            };
            let found = discover_tasks(&ws, runner, &entry);
            if word.starts_with('@') {
                for (repo, tasks) in &found {
                    println!("@{repo}\tlist the tasks of {repo}");
                    for task in tasks {
                        let about = task.description.as_deref().unwrap_or_default();
                        let about: String = about
                            .lines()
                            .next()
                            .unwrap_or_default()
                            .chars()
                            .take(60)
                            .collect();
                        let about = format!("{} {about}", task.source.name());
                        println!("@{repo}:{}\t{}", task.name, about.trim_end());
                    }
                }
            } else {
                for (name, repos) in run::by_name(&found) {
                    println!("{name}\t{}", repos.join(", "));
                }
            }
        }
    }
    Ok(())
}

/// `ws run`: list the tasks, or run the ones named, side by side.
fn cmd_run(
    runner: &Runner,
    cwd: &Path,
    targets: &[String],
    branch: Option<&str>,
    logs: Option<bool>,
    args: &[String],
) -> Result<()> {
    let ws = Workspace::discover(cwd)?;
    let entry = run_branch(&ws, runner, cwd, branch)?;
    let found = discover_tasks(&ws, runner, &entry);

    let targets: Vec<run::Target> = targets
        .iter()
        .map(|t| run::Target::parse(t))
        .collect::<Result<_>>()?;
    let listing: Vec<&str> = targets
        .iter()
        .filter_map(|t| match t {
            run::Target::Repo { repo, task: None } => Some(repo.as_str()),
            _ => None,
        })
        .collect();
    if targets.is_empty() || listing.len() == targets.len() {
        return run::list(&found, &listing, &entry.dir);
    }
    if !listing.is_empty() {
        bail!("`@repo` lists a repository's tasks — name a task as `@repo:task` to run it");
    }

    for target in &targets {
        if let run::Target::Repo { repo, .. } = target {
            unlinked_worktree(&ws, &entry, repo)?;
        }
    }

    let base = ws.worktree_root();
    let branch_name = entry
        .branch
        .clone()
        .unwrap_or_else(|| entry.dir_name(&base));
    let tasks = Tasks::new(&ws.config.tasks, &ws.config.integrations);
    let color = run::use_color(ws.config.tasks.color);
    let mut jobs: Vec<run::Job> = Vec::new();
    for (repo, task) in run::select(&found, &targets, &entry.dir)? {
        if jobs.iter().any(|j| &j.repo == repo && j.task == task.name) {
            continue;
        }
        let worktree = entry
            .repos
            .iter()
            .find(|r| &r.name == repo)
            .map(|r| r.path.clone())
            .context("the repository has a worktree")?;
        let mut cmd = tasks
            .command(task, &worktree, args)
            .env("WS_ROOT", ws.root.to_string_lossy())
            .env("WS_BRANCH", branch_name.as_str())
            .env("WS_BRANCH_DIR", entry.dir.to_string_lossy())
            .env("WS_WORKTREE", worktree.to_string_lossy())
            .env("WS_REPO", repo.as_str());
        if color {
            // Most tools stop colouring once their output is a pipe, which
            // is what `ws` hands them; these ask them not to.
            cmd = cmd.env("FORCE_COLOR", "1").env("CLICOLOR_FORCE", "1");
        }
        jobs.push(run::Job {
            repo: repo.clone(),
            task: task.name.clone(),
            cmd,
        });
    }

    let sinks = if logs.unwrap_or(ws.config.logs.enabled) && !runner.dry_run {
        let logs = logs::Logs::new(&ws.config.logs, ws.logs_dir(&entry.dir), &branch_name);
        let names: Vec<(&str, &str)> = jobs
            .iter()
            .map(|j| (j.repo.as_str(), j.task.as_str()))
            .collect();
        Some(logs.sinks(&names)?)
    } else {
        None
    };
    run::run(runner, &jobs, run::Options { color, sinks })
}

/// A repository of the workspace whose checkout is in the branch directory
/// but that git does not list as a worktree — what moving or renaming the
/// workspace leaves behind, since a worktree and its bare repository point at
/// each other by absolute path.
fn unlinked_worktree(ws: &Workspace, entry: &workspace::BranchStatus, repo: &str) -> Result<()> {
    if entry.repos.iter().any(|r| r.name == repo) {
        return Ok(());
    }
    let Some(config) = ws.config.repos.iter().find(|r| r.name() == repo) else {
        return Ok(());
    };
    let path = if ws.is_grouped() {
        entry.dir.join(config.dir())
    } else {
        entry.dir.clone()
    };
    if !path.join(".git").exists() {
        return Ok(());
    }
    bail!(
        "{} is there, but git does not list it as a worktree of {repo} — the \
         workspace was probably moved; relink it with\n  git --git-dir={} worktree repair {}",
        path.display(),
        ws.bare_path(config).display(),
        path.display()
    )
}

/// The branch directory `ws run` works in: the one named, the one the
/// command was run from, or the default branch's.
fn run_branch(
    ws: &Workspace,
    runner: &Runner,
    cwd: &Path,
    branch: Option<&str>,
) -> Result<workspace::BranchStatus> {
    // A worktree git still records at a path that is gone — a moved
    // workspace, a deleted directory — has nothing to run in.
    let entries: Vec<_> = workspace::list(ws, runner, false)?
        .into_iter()
        .filter(|e| e.dir.is_dir())
        .collect();
    let base = ws.worktree_root();
    if let Some(branch) = branch {
        return entries
            .into_iter()
            .find(|e| e.branch.as_deref() == Some(branch) || e.dir_name(&base) == branch)
            .with_context(|| format!("{branch} has no worktrees — `ws switch {branch}` first"));
    }
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    if let Some(here) = entries.iter().find(|e| cwd.starts_with(&e.dir)) {
        return Ok(here.clone());
    }
    let default = &ws.config.default_branch;
    entries
        .into_iter()
        .find(|e| e.branch.as_deref() == Some(default))
        .with_context(|| {
            format!(
                "not inside a branch directory and {default} has no worktrees — \
                 pass --branch, or `ws switch {default}`"
            )
        })
}

/// The tasks of every repository in the branch directory, in the
/// workspace's order — asked of each repository at once, since `just` and
/// `task` are processes of their own.
fn discover_tasks(
    ws: &Workspace,
    runner: &Runner,
    entry: &workspace::BranchStatus,
) -> Vec<(String, Vec<integration::tasks::Task>)> {
    let tasks = Tasks::new(&ws.config.tasks, &ws.config.integrations);
    std::thread::scope(|scope| {
        let handles: Vec<_> = entry
            .repos
            .iter()
            .map(|repo| {
                let tasks = &tasks;
                scope.spawn(move || (repo.name.clone(), tasks.discover(runner, &repo.path)))
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    })
}

/// `value<tab>description`, the line both the shells and `ws` agree on.
fn describe(choices: Vec<workspace::BranchChoice>) {
    for choice in choices {
        let repos = choice.repos.join(", ");
        match choice.kind {
            workspace::ChoiceKind::CheckedOut => {
                println!("{}\tchecked out — {repos}", choice.branch)
            }
            workspace::ChoiceKind::Branch => println!("{}\t{repos}", choice.branch),
            workspace::ChoiceKind::Tag => println!("{}\ttag — {repos}", choice.branch),
        }
    }
}

fn cmd_init(
    runner: &Runner,
    cwd: &Path,
    name: Option<String>,
    repos: &[String],
    force: bool,
) -> Result<()> {
    let path = cwd.join(CONFIG_FILE);
    if path.exists() && !force {
        bail!(
            "{} already exists — pass --force to overwrite",
            path.display()
        );
    }
    let name = name.unwrap_or_else(|| {
        cwd.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "ws".into())
    });
    let text = config::local_template(&name, repos);
    if runner.dry_run {
        eprintln!("would write {}", path.display());
        print!("{text}");
        return Ok(());
    }
    std::fs::write(&path, text).with_context(|| format!("cannot write {}", path.display()))?;
    eprintln!("wrote {}", path.display());
    if repos.is_empty() {
        eprintln!("add one entry under `repos` per repository, then run `ws fetch`");
    } else {
        eprintln!("run `ws fetch` to clone them");
    }
    match config::global_config_path() {
        Some(global) if global.is_file() => {
            eprintln!("settings come from {}", global.display())
        }
        Some(global) => eprintln!(
            "settings use the built-in defaults; `ws init --global` writes {}",
            global.display()
        ),
        None => {}
    }
    suggest_skill();
    Ok(())
}

/// Write the documented settings file every workspace layers on top of.
fn cmd_init_global(runner: &Runner, force: bool) -> Result<()> {
    let path = config::global_config_path()
        .context("cannot locate a config directory — set WS_CONFIG or HOME")?;
    if path.exists() && !force {
        bail!(
            "{} already exists — pass --force to overwrite",
            path.display()
        );
    }
    let text = config::global_template();
    if runner.dry_run {
        eprintln!("would write {}", path.display());
        print!("{text}");
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    std::fs::write(&path, text).with_context(|| format!("cannot write {}", path.display()))?;
    eprintln!("wrote {}", path.display());
    suggest_skill();
    Ok(())
}

/// The agent skill, embedded so that `ws skill` works wherever the binary does.
const SKILL: &str = include_str!("../skills/ws/SKILL.md");

/// Point at `ws skill` while Claude Code has no copy of it. Only a hint:
/// `~/.claude` is the user's, not something `ws` writes into on its own.
fn suggest_skill() {
    let base = match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) if !dir.is_empty() => std::path::PathBuf::from(dir),
        _ => match std::env::var_os("HOME") {
            Some(home) => Path::new(&home).join(".claude"),
            None => return,
        },
    };
    let dir = base.join("skills").join("ws");
    if dir.join("SKILL.md").exists() {
        return;
    }
    eprintln!(
        "coding agents can learn ws from its skill; for Claude Code:\n  mkdir -p '{0}' && ws skill > '{0}/SKILL.md'",
        dir.display()
    );
}

/// Shared by `ws switch` and a branch chosen in the picker.
/// Where a new branch starts: `--from` when it was given, otherwise the
/// branch of the directory the command ran in — forking from what you are
/// looking at rather than from the base branch.
fn start_from(here: Option<String>, flag: Option<String>) -> Option<workspace::Start> {
    if let Some(reference) = flag {
        return Some(workspace::Start {
            reference,
            source: workspace::StartSource::Flag,
        });
    }
    here.map(|reference| workspace::Start {
        reference,
        source: workspace::StartSource::CurrentBranch,
    })
}

fn switch_and_open(
    ws: &Workspace,
    runner: &Runner,
    branch: &str,
    options: &SwitchOptions,
    force: Option<Backend>,
    cd: bool,
) -> Result<()> {
    let switched = workspace::switch(ws, runner, branch, options)?;

    if !switched.created.is_empty() {
        eprintln!("created worktrees: {}", switched.created.join(", "));
    }

    let zox = Zoxide::new(&ws.config.integrations.zoxide);
    let worktrees = switched.paths();
    let register = zox.paths_to_register(&switched.branch_dir, &worktrees);
    zox.add(runner, &register);

    if cd {
        // Machine-readable channel for the shell wrapper.
        println!("{}", switched.branch_dir.display());
        return Ok(());
    }

    let plan = workspace::session_plan(ws, &switched);
    let backend = mux::select(&ws.config.mux, force);
    backend.open(runner, &plan)
}

fn cmd_list(runner: &Runner, cwd: &Path) -> Result<()> {
    let ws = Workspace::discover(cwd)?;
    let entries = workspace::list(&ws, runner, true)?;
    if entries.is_empty() {
        eprintln!(
            "no worktrees yet — try `ws switch {}`",
            ws.config.default_branch
        );
        return Ok(());
    }
    let base = ws.worktree_root();
    let width = entries
        .iter()
        .map(|e| e.dir_name(&base).chars().count())
        .max()
        .unwrap_or(4)
        .max(9);
    for entry in entries {
        let repos: Vec<String> = entry
            .repos
            .iter()
            .map(|r| {
                let mark = if r.dirty { "*" } else { "" };
                format!("{}{mark}", r.name)
            })
            .collect();
        println!(
            "{:<width$}  {:<24}  {}",
            entry.dir_name(&base),
            entry.branch.as_deref().unwrap_or("(detached)"),
            repos.join(", "),
            width = width
        );
    }
    Ok(())
}

fn cmd_path(cwd: &Path, branch: &str, repo: Option<&str>) -> Result<()> {
    let ws = Workspace::discover(cwd)?;
    paths::validate_branch(branch)?;
    let mode = ws.config.branch_dirs;
    let base = ws.worktree_root();
    let path = match repo {
        None => paths::branch_dir(&base, branch, mode),
        Some(repo) => {
            let config = ws
                .config
                .repos
                .iter()
                .find(|r| r.name() == repo)
                .with_context(|| format!("no repository named {repo} in this workspace"))?;
            paths::worktree_path(&base, branch, mode, ws.is_grouped(), &config.dir())
        }
    };
    println!("{}", path.display());
    Ok(())
}

fn cmd_rm(
    runner: &Runner,
    force_backend: Option<Backend>,
    cwd: &Path,
    branch: &str,
    force: bool,
) -> Result<()> {
    let ws = Workspace::discover(cwd)?;
    let removed = workspace::remove(&ws, runner, branch, force)?;

    let zox = Zoxide::new(&ws.config.integrations.zoxide);
    let worktrees = removed.paths();
    let register = zox.paths_to_register(&removed.branch_dir, &worktrees);
    zox.remove(runner, &register);

    let session = workspace::session_name_for(&ws, branch);
    let backend = mux::select(&ws.config.mux, force_backend);
    if let Err(e) = backend.close(runner, &session) {
        eprintln!(
            "warning: cannot close the {} session: {e:#}",
            backend.name()
        );
    }
    eprintln!("removed {}", removed.branch_dir.display());
    Ok(())
}

fn cmd_status(runner: &Runner, cwd: &Path) -> Result<()> {
    let ws = Workspace::discover(cwd)?;
    println!("workspace  {}", ws.name());
    println!("root       {}", ws.root.display());
    for (i, source) in ws.sources.iter().enumerate() {
        let label = if i == 0 { "config" } else { "" };
        println!("{label:<10} {}", source.display());
    }
    println!(
        "layout     {} / {}",
        if ws.is_grouped() { "grouped" } else { "single" },
        match ws.config.branch_dirs {
            config::BranchDirs::Flat => "flat",
            config::BranchDirs::Nested => "nested",
        }
    );

    // What `auto` resolved to on this machine, which is otherwise only
    // visible from what a switch does.
    let mut active: Vec<&str> = Vec::new();
    if Zoxide::new(&ws.config.integrations.zoxide).is_enabled() {
        active.push("zoxide");
    }
    if Local::new(&ws).is_enabled() {
        active.push("local");
    }
    active.extend(PackageManagers::new(&ws.config.integrations).names());
    let tasks = Tasks::new(&ws.config.tasks, &ws.config.integrations).names();
    println!(
        "tools      {}",
        if active.is_empty() {
            "(none)".to_string()
        } else {
            active.join(", ")
        }
    );

    println!(
        "tasks      {}",
        if tasks.is_empty() {
            "(none)".to_string()
        } else {
            tasks.join(", ")
        }
    );
    let logs = &ws.config.logs;
    if logs.enabled {
        println!(
            "logs       {} (rotated at {}, {} kept)",
            Path::new(&logs.dir).join(&logs.file).display(),
            logs.max_size,
            logs.keep
        );
    } else {
        println!("logs       off");
    }

    match workspace::current_branch(&ws, runner, cwd) {
        Some(entry) => {
            let branch = entry.branch.clone().unwrap_or_else(|| "(detached)".into());
            println!("branch     {branch}");
            if let Some(from) = marker::Marker::load(&entry.dir).and_then(|m| m.from) {
                println!("started    {from}");
            }
            println!("session    {}", workspace::session_name_for(&ws, &branch));
            for repo in entry.repos {
                println!(
                    "  {:<12} {:<20} {}",
                    repo.name,
                    repo.branch.as_deref().unwrap_or("(detached)"),
                    repo.path.display()
                );
            }
        }
        None => println!("branch     (not inside a branch directory)"),
    }
    Ok(())
}

fn cmd_pick(
    runner: &Runner,
    force: Option<Backend>,
    cwd: &Path,
    global: bool,
    inline: bool,
    query: &[String],
) -> Result<()> {
    let self_exe = std::env::current_exe().context("cannot find the ws binary")?;
    let mut current = Workspace::discover(cwd).ok();

    loop {
        let config = current
            .as_ref()
            .map(|w| w.config.clone())
            .unwrap_or_default();
        let backend = mux::select(&config.mux, force);
        let picker = Picker::new(
            exec::launcher_from_env("WS_PICKER_CMD")
                .unwrap_or_else(|| config.ui.picker_cmd.clone()),
            config.ui.picker,
            config.ui.popup.clone(),
        );
        let zox = Zoxide::new(&config.integrations.zoxide);
        let root = current
            .as_ref()
            .map(|w| w.root.clone())
            .unwrap_or_else(|| cwd.to_path_buf());

        let probe = Cmd::new("true");
        let has_popup = backend
            .popup(picker.popup(), &root, " ws ", &probe)
            .is_some();

        if !inline && picker.resolve(has_popup) == PickerMode::Popup {
            // The popup runs ws itself: it owns a tty for fzf and finishes the
            // switch from inside the session, which a returned string could not.
            let mut inner = Cmd::new(self_exe.to_string_lossy()).args(["pick", "--inline"]);
            if global {
                inner = inner.arg("--global");
            }
            inner = inner.args(query.iter().cloned());
            let popup = backend
                .popup(picker.popup(), &root, " ws ", &inner)
                .context("backend lost popup support")?;
            return runner.run(&popup);
        }

        let mut candidates = Vec::new();
        if let Some(ws) = &current {
            candidates.extend(pick::workspace_candidates(ws, runner)?);
        }
        if global || current.is_none() {
            candidates.extend(pick::global_candidates(
                &zox,
                runner,
                query,
                current.as_ref().map(|w| w.root.as_path()),
            )?);
        }
        if candidates.is_empty() {
            bail!(missing_candidates_hint(current.is_some(), zox.is_enabled()));
        }

        if picker.resolve(has_popup) == PickerMode::None {
            for candidate in &candidates {
                println!("{}", candidate.payload());
            }
            return Ok(());
        }

        match picker.choose(&self_exe, true, &candidates)? {
            None => return Ok(()),
            Some(Candidate::Branch { branch, .. }) => {
                let ws = current
                    .as_ref()
                    .context("a branch was chosen outside a workspace")?;
                let here = workspace::current_branch_name(ws, runner, cwd);
                let options = SwitchOptions {
                    from: start_from(here.clone(), None),
                    seed_from: here,
                    ..SwitchOptions::default()
                };
                return switch_and_open(ws, runner, &branch, &options, force, false);
            }
            Some(Candidate::Workspace { root }) => {
                current = Some(Workspace::load(&root)?);
            }
        }
    }
}

fn missing_candidates_hint(in_workspace: bool, zoxide_enabled: bool) -> String {
    if in_workspace {
        "nothing to pick — run `ws fetch` to get the branches".to_string()
    } else if zoxide_enabled {
        format!("no {CONFIG_FILE} here and no known workspace in the zoxide database")
    } else {
        format!("no {CONFIG_FILE} here — run `ws init` or enable zoxide to find other workspaces")
    }
}

impl Default for Config {
    fn default() -> Config {
        config::parse("").expect("the empty config is valid")
    }
}
