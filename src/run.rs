//! `ws run`: tasks side by side, their output interleaved line by line.
//!
//! Each task's stdout and stderr are read a line at a time and printed with a
//! prefix naming where they came from — the repository, or `repo:task` when
//! the tasks differ — coloured so the eye can follow one of them, and padded
//! so the output lines up. stdout stays stdout and stderr stays stderr.
//!
//! An interrupt from the terminal reaches every task by itself: they share
//! `ws`'s process group. `ws` only outlives it long enough to print what they
//! say on the way out, and passes a `SIGTERM` or `SIGHUP` of its own on, since
//! those arrive at `ws` alone.

use anyhow::{bail, Result};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::sync::Mutex;

use crate::config::ColorMode;
use crate::exec::{Cmd, Runner};
use crate::integration::tasks::Task;
use crate::logs::Sink;

/// What `ws run` was asked for, one argument at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// `task`: every repository that has it.
    All(String),
    /// `@repo:task`, or `@repo` alone to list what it has.
    Repo { repo: String, task: Option<String> },
}

impl Target {
    /// The repository is everything up to the first `:`, so a task may have
    /// colons of its own: `@api:db:migrate`, `@api:db::seed`.
    pub fn parse(text: &str) -> Result<Target> {
        let Some(rest) = text.strip_prefix('@') else {
            if text.is_empty() {
                bail!("empty task name");
            }
            return Ok(Target::All(text.to_string()));
        };
        let (repo, task) = match rest.split_once(':') {
            Some((repo, task)) => (repo, Some(task)),
            None => (rest, None),
        };
        if repo.is_empty() || task == Some("") {
            bail!("{text:?} — write `@repo:task`, or `@repo` to list its tasks");
        }
        Ok(Target::Repo {
            repo: repo.to_string(),
            task: task.map(str::to_string),
        })
    }
}

/// The tasks of each repository, as discovered, in the workspace's order.
pub type Found = [(String, Vec<Task>)];

/// The task each target names, per repository, in the order asked.
pub fn select<'a>(
    found: &'a Found,
    targets: &[Target],
    dir: &Path,
) -> Result<Vec<(&'a String, &'a Task)>> {
    let mut selected = Vec::new();
    for target in targets {
        match target {
            Target::All(name) => {
                let before = selected.len();
                for (repo, tasks) in found {
                    if let Some(task) = tasks.iter().find(|t| &t.name == name) {
                        selected.push((repo, task));
                    }
                }
                if selected.len() == before {
                    bail!(
                        "no repository in {} has a task named {name} — `ws run` lists them",
                        dir.display()
                    );
                }
            }
            Target::Repo { repo, task } => {
                let name = task.as_deref().unwrap_or_default();
                let Some((repo, tasks)) = found.iter().find(|(r, _)| r == repo) else {
                    let repos: Vec<&str> = found.iter().map(|(r, _)| r.as_str()).collect();
                    bail!(
                        "no repository named {repo} in {} — it has {}",
                        dir.display(),
                        repos.join(", ")
                    );
                };
                let Some(task) = tasks.iter().find(|t| t.name == name) else {
                    bail!("{repo} has no task named {name} — `ws run @{repo}` lists them");
                };
                selected.push((repo, task));
            }
        }
    }
    Ok(selected)
}

/// Every task name, with the repositories that have it.
pub fn by_name(found: &Found) -> Vec<(&str, Vec<&str>)> {
    let mut names: Vec<(&str, Vec<&str>)> = Vec::new();
    for (repo, tasks) in found {
        for task in tasks {
            match names.iter_mut().find(|(n, _)| *n == task.name) {
                Some((_, repos)) => repos.push(repo),
                None => names.push((&task.name, vec![repo])),
            }
        }
    }
    names
}

/// `ws run` with nothing to run: the tasks of every repository, or of the
/// ones named with `@repo`.
pub fn list(found: &Found, only: &[&str], dir: &Path) -> Result<()> {
    for repo in only {
        if !found.iter().any(|(r, _)| r == repo) {
            bail!("no repository named {repo} in {}", dir.display());
        }
    }
    let shown: Vec<&(String, Vec<Task>)> = found
        .iter()
        .filter(|(r, _)| only.is_empty() || only.contains(&r.as_str()))
        .collect();
    let width = shown
        .iter()
        .flat_map(|(_, tasks)| tasks.iter().map(|t| t.name.chars().count()))
        .max()
        .unwrap_or(0);
    let mut out = std::io::stdout().lock();
    for (repo, tasks) in shown {
        let _ = writeln!(out, "{repo}");
        if tasks.is_empty() {
            let _ = writeln!(out, "  (no tasks)");
        }
        for task in tasks {
            let about = task.description.as_deref().unwrap_or_default();
            let about: String = about
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .take(70)
                .collect();
            let line = format!(
                "  {:<width$}  {:<7}  {about}",
                task.name,
                task.source.name()
            );
            let _ = writeln!(out, "{}", line.trim_end());
        }
    }
    Ok(())
}

/// Whether the prefixes are coloured: `auto` asks for a terminal on stdout
/// and no `NO_COLOR`.
pub fn use_color(mode: ColorMode) -> bool {
    use std::io::IsTerminal;
    match mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => {
            std::io::stdout().is_terminal()
                && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
        }
    }
}

/// One task in one repository.
#[derive(Debug, Clone)]
pub struct Job {
    pub repo: String,
    pub task: String,
    pub cmd: Cmd,
}

#[derive(Debug, Default)]
pub struct Options {
    pub color: bool,
    /// One per job, in the same order, when logging is on.
    pub sinks: Option<Vec<Sink>>,
}

/// Prefix colours, picked in turn: the six plain ones, then their bright
/// variants.
const COLORS: [u8; 12] = [36, 35, 33, 32, 34, 31, 96, 95, 93, 92, 94, 91];

/// Every terminal line goes through this, so two tasks never interleave
/// within one line.
static OUTPUT: Mutex<()> = Mutex::new(());

/// Run every job at once and wait for all of them. A task that fails does
/// not stop the others; the run fails afterwards, naming it.
pub fn run(runner: &Runner, jobs: &[Job], options: Options) -> Result<()> {
    let labels = labels(jobs);
    let width = labels.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let prefixes: Vec<String> = labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let padded = format!("{label:<width$} |");
            if options.color {
                format!("\x1b[{}m{padded}\x1b[0m", COLORS[i % COLORS.len()])
            } else {
                padded
            }
        })
        .collect();

    if runner.dry_run {
        for (job, prefix) in jobs.iter().zip(&prefixes) {
            let dir = job.cmd.cwd.as_ref().map(|d| d.display().to_string());
            eprintln!(
                "{prefix} would run: {} (in {})",
                job.cmd.display(),
                dir.unwrap_or_default()
            );
        }
        return Ok(());
    }

    signals::install();
    let failures: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = jobs
            .iter()
            .enumerate()
            .map(|(i, job)| {
                let prefix = &prefixes[i];
                let sink = options.sinks.as_ref().map(|s| &s[i]);
                scope.spawn(move || run_one(runner, job, prefix, sink))
            })
            .collect();
        handles
            .into_iter()
            .zip(&labels)
            .filter_map(|(handle, label)| match handle.join() {
                Ok(Ok(true)) => None,
                // Said already, under the task's own prefix.
                Ok(Ok(false)) => Some(label.clone()),
                Ok(Err(e)) => {
                    let _guard = OUTPUT.lock();
                    eprintln!("error: {label}: {e:#}");
                    Some(label.clone())
                }
                Err(_) => Some(label.clone()),
            })
            .collect()
    });
    signals::restore();

    match failures.len() {
        0 => Ok(()),
        _ if jobs.len() == 1 => bail!("{} failed", failures[0]),
        n => bail!("{n} of {} failed: {}", jobs.len(), failures.join(", ")),
    }
}

/// `repo` when every job runs the same task, `repo:task` otherwise.
fn labels(jobs: &[Job]) -> Vec<String> {
    let same_task = jobs.windows(2).all(|w| w[0].task == w[1].task);
    let same_repo = jobs.windows(2).all(|w| w[0].repo == w[1].repo);
    jobs.iter()
        .map(|job| match (same_task, same_repo) {
            (true, _) => job.repo.clone(),
            (false, true) => job.task.clone(),
            (false, false) => format!("{}:{}", job.repo, job.task),
        })
        .collect()
}

/// Whether the task succeeded; an error is one that kept it from starting.
fn run_one(runner: &Runner, job: &Job, prefix: &str, sink: Option<&Sink>) -> Result<bool> {
    if runner.verbose {
        say(prefix, sink, &format!("+ {}", job.cmd.display()), true);
    }
    if let Some(sink) = sink {
        sink.line(&format!("--- {} ---", job.cmd.display()));
    }
    let mut child = job
        .cmd
        .build()
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| job.cmd.spawn_error(e))?;
    signals::track(child.id());
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");

    std::thread::scope(|scope| {
        scope.spawn(|| pump(stdout, prefix, sink, false));
        scope.spawn(|| pump(stderr, prefix, sink, true));
    });
    let status = child.wait()?;
    signals::untrack(child.id());

    let ended = describe(status);
    if let Some(sink) = sink {
        sink.line(&format!("--- {ended} ---"));
    }
    if !status.success() {
        say(prefix, None, &ended, true);
    }
    Ok(status.success())
}

fn describe(status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        return format!("exited with code {code}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return format!("killed by signal {signal}");
        }
    }
    format!("ended with {status}")
}

/// Copy one stream to the terminal, a prefixed line at a time.
fn pump(stream: impl Read, prefix: &str, sink: Option<&Sink>, stderr: bool) {
    let mut reader = BufReader::new(stream);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim_end_matches(['\n', '\r']);
        say(prefix, sink, line, stderr);
    }
}

fn say(prefix: &str, sink: Option<&Sink>, line: &str, stderr: bool) {
    {
        let _guard = OUTPUT.lock();
        // A closed pipe — `ws run dev | head` — must not take the task down
        // with it, so the write is allowed to fail.
        let _ = if stderr {
            writeln!(std::io::stderr().lock(), "{prefix} {line}")
        } else {
            writeln!(std::io::stdout().lock(), "{prefix} {line}")
        };
    }
    if let Some(sink) = sink {
        sink.line(line);
    }
}

#[cfg(unix)]
mod signals {
    //! While the tasks run, an interrupt from the terminal is theirs to act
    //! on; `ws` waits for them to finish. A termination sent to `ws` alone is
    //! passed on.
    //!
    //! A handler rather than `SIG_IGN`: an ignored signal stays ignored across
    //! `exec`, and the tasks would inherit that.

    use std::sync::atomic::{AtomicI32, Ordering};

    const SLOTS: usize = 256;
    static PIDS: [AtomicI32; SLOTS] = [const { AtomicI32::new(0) }; SLOTS];

    extern "C" fn interrupted(_: libc::c_int) {}

    extern "C" fn terminated(_: libc::c_int) {
        for slot in &PIDS {
            let pid = slot.load(Ordering::SeqCst);
            if pid > 0 {
                // SAFETY: kill is async-signal-safe.
                unsafe { libc::kill(pid, libc::SIGTERM) };
            }
        }
    }

    pub fn install() {
        // SAFETY: both handlers only touch atomics and call kill.
        unsafe {
            libc::signal(libc::SIGINT, interrupted as *const () as libc::sighandler_t);
            libc::signal(libc::SIGTERM, terminated as *const () as libc::sighandler_t);
            libc::signal(libc::SIGHUP, terminated as *const () as libc::sighandler_t);
        }
    }

    pub fn restore() {
        // SAFETY: putting back the defaults.
        unsafe {
            libc::signal(libc::SIGINT, libc::SIG_DFL);
            libc::signal(libc::SIGTERM, libc::SIG_DFL);
            libc::signal(libc::SIGHUP, libc::SIG_DFL);
        }
    }

    pub fn track(pid: u32) {
        for slot in &PIDS {
            if slot
                .compare_exchange(0, pid as i32, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return;
            }
        }
    }

    pub fn untrack(pid: u32) {
        for slot in &PIDS {
            let _ = slot.compare_exchange(pid as i32, 0, Ordering::SeqCst, Ordering::SeqCst);
        }
    }
}

#[cfg(not(unix))]
mod signals {
    pub fn install() {}
    pub fn restore() {}
    pub fn track(_: u32) {}
    pub fn untrack(_: u32) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(repo: &str, task: &str) -> Job {
        Job {
            repo: repo.into(),
            task: task.into(),
            cmd: Cmd::new("true"),
        }
    }

    #[test]
    fn targets_parse_with_colons_left_to_the_task() {
        assert_eq!(Target::parse("dev").unwrap(), Target::All("dev".into()));
        assert_eq!(
            Target::parse("@api:db:migrate").unwrap(),
            Target::Repo {
                repo: "api".into(),
                task: Some("db:migrate".into())
            }
        );
        assert_eq!(
            Target::parse("@api").unwrap(),
            Target::Repo {
                repo: "api".into(),
                task: None
            }
        );
        for bad in ["", "@", "@:dev", "@api:"] {
            assert!(Target::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_bare_task_runs_wherever_it_exists() {
        use crate::config::TaskSource;
        let t = |name: &str| Task {
            name: name.into(),
            source: TaskSource::Just,
            description: None,
        };
        let found = vec![
            ("api".to_string(), vec![t("dev"), t("test")]),
            ("web".to_string(), vec![t("dev")]),
            ("docs".to_string(), vec![]),
        ];
        let dir = Path::new("/w/main");
        let picked = |targets: &[&str]| -> Result<Vec<String>> {
            let targets: Vec<Target> = targets.iter().map(|t| Target::parse(t).unwrap()).collect();
            Ok(select(&found, &targets, dir)?
                .into_iter()
                .map(|(repo, task)| format!("{repo}:{}", task.name))
                .collect())
        };
        assert_eq!(picked(&["dev"]).unwrap(), ["api:dev", "web:dev"]);
        assert_eq!(picked(&["test"]).unwrap(), ["api:test"]);
        assert_eq!(
            picked(&["@web:dev", "test"]).unwrap(),
            ["web:dev", "api:test"]
        );
        assert!(picked(&["lint"]).is_err());
        assert!(picked(&["@web:test"]).is_err());
        assert!(picked(&["@nope:dev"]).is_err());

        let names = by_name(&found);
        assert_eq!(names[0], ("dev", vec!["api", "web"]));
        assert_eq!(names[1], ("test", vec!["api"]));
    }

    #[test]
    fn labels_say_only_what_tells_the_jobs_apart() {
        assert_eq!(
            labels(&[job("api", "dev"), job("web", "dev")]),
            ["api", "web"]
        );
        assert_eq!(
            labels(&[job("web", "dev"), job("web", "lint")]),
            ["dev", "lint"]
        );
        assert_eq!(
            labels(&[job("api", "dev"), job("web", "lint")]),
            ["api:dev", "web:lint"]
        );
        assert_eq!(labels(&[job("api", "dev")]), ["api"]);
    }
}
