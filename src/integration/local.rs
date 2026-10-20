//! `local`: untracked files kept beside the workspace and placed into every
//! worktree — the `.env` with your credentials, editor settings, a personal
//! `Makefile.local` — so a new branch starts with them instead of without.
//!
//! The files live in one directory, `integrations.local.dir`. With a single
//! repository everything in it belongs to that repository; with several, it
//! holds one subdirectory per repository, named after it, and
//! `local/api/.env` goes to `<branch>/api/.env`.
//!
//! Each entry's path relative to that directory is matched against the
//! `paths` rules in order, and the first rule that matches decides how it is
//! placed: a symlink, a copy or a hard link. A directory a rule matches is
//! placed whole when the worktree has nothing there; when the worktree already
//! has that directory — it is tracked — the walk goes inside it instead, so
//! `local/config/dev.yaml` can sit next to the repository's own `config/`.
//!
//! What the worktree already has is never replaced. The repository's files
//! are the repository's, and an existing target on a later switch is simply
//! what an earlier one placed — which is also what makes it safe to run on
//! every switch, picking up files added to the local directory since.

use anyhow::{Context, Result};
use regex::Regex;
use std::path::{Path, PathBuf};

use crate::config::{LocalMode, RepoConfig, Workspace};
use crate::exec::Runner;

use super::link;

pub struct Local {
    enabled: bool,
    dir: PathBuf,
    grouped: bool,
    rules: Vec<(Regex, LocalMode)>,
}

/// What one repository got, for the line that reports it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Placed {
    linked: usize,
    copied: usize,
    hardlinked: usize,
}

impl Placed {
    fn add(&mut self, mode: LocalMode) {
        match mode {
            LocalMode::Link => self.linked += 1,
            LocalMode::Copy => self.copied += 1,
            LocalMode::Hardlink => self.hardlinked += 1,
        }
    }

    fn summary(&self) -> Option<String> {
        let parts: Vec<String> = [
            (self.linked, "linked"),
            (self.copied, "copied"),
            (self.hardlinked, "hard-linked"),
        ]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} {what}"))
        .collect();
        (!parts.is_empty()).then(|| parts.join(", "))
    }
}

impl Local {
    pub fn new(ws: &Workspace) -> Local {
        let config = &ws.config.integrations.local;
        let dir = ws.local_dir();
        Local {
            enabled: config.enabled.resolve(dir.is_dir()),
            grouped: ws.is_grouped(),
            // Checked when the configuration was loaded, so none is dropped
            // here that the user would not already have been told about.
            rules: config
                .paths
                .iter()
                .filter_map(|rule| Some((rule.regex().ok()?, rule.mode)))
                .collect(),
            dir,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// The directory a repository's local files are kept in.
    pub fn source(&self, repo: &RepoConfig) -> PathBuf {
        if self.grouped {
            self.dir.join(repo.name())
        } else {
            self.dir.clone()
        }
    }

    /// Place the repository's local files into `worktree`.
    ///
    /// `fresh` says the worktree was created by this switch, so anything
    /// already in the way is the repository's own and worth a warning; on a
    /// later switch it is just what the previous one put there.
    pub fn place(&self, runner: &Runner, repo: &RepoConfig, worktree: &Path, fresh: bool) {
        if !self.enabled {
            return;
        }
        let source = self.source(repo);
        if !source.is_dir() {
            return;
        }
        let name = repo.name();
        let mut placed = Placed::default();
        self.walk(
            runner,
            &name,
            &source,
            worktree,
            Path::new(""),
            fresh,
            &mut placed,
        );
        if let Some(summary) = placed.summary() {
            eprintln!("  [{name}] local: {summary}");
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        &self,
        runner: &Runner,
        repo: &str,
        source: &Path,
        worktree: &Path,
        relative: &Path,
        fresh: bool,
        placed: &mut Placed,
    ) {
        let Ok(entries) = std::fs::read_dir(source.join(relative)) else {
            return;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let relative = relative.join(entry.file_name());
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let from = entry.path();
            let to = worktree.join(&relative);
            let mode = self.mode_for(&relative);
            // `symlink_metadata`, so a dangling link still counts as in the way.
            let occupied = std::fs::symlink_metadata(&to).ok();

            if kind.is_dir() {
                match (&occupied, mode) {
                    (None, Some(mode)) => self.put(runner, repo, mode, &from, &to, placed),
                    // Nothing claims the directory itself, but a rule may
                    // still claim something inside it; and a directory the
                    // worktree has already is gone into rather than replaced.
                    (None, None) => {
                        self.walk(runner, repo, source, worktree, &relative, fresh, placed)
                    }
                    (Some(meta), _) if meta.is_dir() => {
                        self.walk(runner, repo, source, worktree, &relative, fresh, placed)
                    }
                    (Some(_), _) => in_the_way(repo, &to, fresh),
                }
                continue;
            }

            let Some(mode) = mode else {
                continue;
            };
            if occupied.is_some() {
                in_the_way(repo, &to, fresh);
                continue;
            }
            self.put(runner, repo, mode, &from, &to, placed);
        }
    }

    /// The mode of the first rule that matches `relative`, written with `/`
    /// whatever the platform, so a pattern means the same everywhere.
    fn mode_for(&self, relative: &Path) -> Option<LocalMode> {
        let key = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        self.rules
            .iter()
            .find(|(regex, _)| regex.is_match(&key))
            .map(|(_, mode)| *mode)
    }

    fn put(
        &self,
        runner: &Runner,
        repo: &str,
        mode: LocalMode,
        from: &Path,
        to: &Path,
        placed: &mut Placed,
    ) {
        if runner.dry_run {
            let verb = match mode {
                LocalMode::Link => "symlink",
                LocalMode::Copy => "copy",
                LocalMode::Hardlink => "hard-link",
            };
            eprintln!("would {verb} {} -> {}", from.display(), to.display());
            placed.add(mode);
            return;
        }
        match put(mode, from, to) {
            Ok(()) => placed.add(mode),
            Err(e) => {
                // Half a directory is worse than none: it would pass for the
                // whole thing on the next switch and never be completed.
                if std::fs::symlink_metadata(to).is_ok_and(|m| m.is_dir()) {
                    let _ = std::fs::remove_dir_all(to);
                }
                eprintln!("warning: [{repo}] cannot place {}: {e:#}", to.display());
            }
        }
    }
}

fn put(mode: LocalMode, from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let kind = std::fs::symlink_metadata(from)
        .with_context(|| format!("cannot stat {}", from.display()))?
        .file_type();
    match mode {
        LocalMode::Link => link::symlink(from, to),
        // A symlink kept in the local directory is reproduced as one, under
        // both of the other modes: copying it would copy what it points at.
        _ if kind.is_symlink() => {
            let pointee = std::fs::read_link(from)
                .with_context(|| format!("cannot read {}", from.display()))?;
            link::symlink(&pointee, to)
        }
        LocalMode::Copy if kind.is_dir() => link::copy_tree(from, to).map(drop),
        LocalMode::Hardlink if kind.is_dir() => link::tree(from, to).map(drop),
        LocalMode::Copy => link::file(from, to, false).map(drop),
        LocalMode::Hardlink => link::file(from, to, true).map(drop),
    }
}

fn in_the_way(repo: &str, to: &Path, fresh: bool) {
    if fresh {
        eprintln!(
            "warning: [{repo}] {} is in the repository already, keeping it",
            to.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LocalPath;
    use tempfile::TempDir;

    fn local(dir: &Path, rules: &[(&str, LocalMode)]) -> Local {
        Local {
            enabled: true,
            dir: dir.to_path_buf(),
            grouped: false,
            rules: rules
                .iter()
                .map(|(pattern, mode)| {
                    let rule = LocalPath {
                        pattern: pattern.to_string(),
                        mode: *mode,
                    };
                    (rule.regex().unwrap(), *mode)
                })
                .collect(),
        }
    }

    fn repo() -> RepoConfig {
        crate::config::parse("repos:\n  - url: x/app.git\n")
            .unwrap()
            .repos
            .remove(0)
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn the_first_matching_rule_decides_and_patterns_match_whole_paths() {
        let l = local(
            Path::new("/l"),
            &[
                (r"\.env.*", LocalMode::Copy),
                ("secrets/.*", LocalMode::Hardlink),
                (".*", LocalMode::Link),
            ],
        );
        assert_eq!(l.mode_for(Path::new(".env")), Some(LocalMode::Copy));
        assert_eq!(l.mode_for(Path::new(".env.local")), Some(LocalMode::Copy));
        // Anchored: `.env` somewhere inside a path is not a match.
        assert_eq!(l.mode_for(Path::new("web/.env")), Some(LocalMode::Link));
        assert_eq!(
            l.mode_for(Path::new("secrets/key")),
            Some(LocalMode::Hardlink)
        );

        let only_env = local(Path::new("/l"), &[(r"\.env", LocalMode::Link)]);
        assert_eq!(only_env.mode_for(Path::new("README")), None);
    }

    #[test]
    fn places_each_mode_and_goes_inside_directories_the_worktree_has() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("local");
        write(&dir.join(".env"), "SECRET=1");
        write(&dir.join("notes.md"), "mine");
        write(&dir.join(".vscode/settings.json"), "{}");
        write(&dir.join("config/dev.yaml"), "dev");

        let worktree = tmp.path().join("main");
        // Tracked by the repository: gone into, not replaced.
        write(&worktree.join("config/base.yaml"), "base");

        let l = local(
            &dir,
            &[
                (r"\.env", LocalMode::Copy),
                (r"notes\.md", LocalMode::Hardlink),
                (".*", LocalMode::Link),
            ],
        );
        l.place(&Runner::default(), &repo(), &worktree, true);

        let env = worktree.join(".env");
        assert!(!std::fs::symlink_metadata(&env).unwrap().is_symlink());
        assert_eq!(std::fs::read_to_string(&env).unwrap(), "SECRET=1");

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let a = std::fs::metadata(dir.join("notes.md")).unwrap();
            let b = std::fs::metadata(worktree.join("notes.md")).unwrap();
            assert_eq!(a.ino(), b.ino());
        }

        // A directory the worktree did not have is linked whole...
        assert_eq!(
            std::fs::read_link(worktree.join(".vscode")).unwrap(),
            dir.join(".vscode")
        );
        // ...and one it had gets the file placed inside it.
        assert!(!std::fs::symlink_metadata(worktree.join("config"))
            .unwrap()
            .is_symlink());
        assert_eq!(
            std::fs::read_link(worktree.join("config/dev.yaml")).unwrap(),
            dir.join("config/dev.yaml")
        );
        assert!(worktree.join("config/base.yaml").is_file());
    }

    #[test]
    fn what_the_worktree_has_is_never_replaced() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("local");
        write(&dir.join("README.md"), "local");
        let worktree = tmp.path().join("main");
        write(&worktree.join("README.md"), "tracked");

        local(&dir, &[(".*", LocalMode::Copy)]).place(&Runner::default(), &repo(), &worktree, true);
        assert_eq!(
            std::fs::read_to_string(worktree.join("README.md")).unwrap(),
            "tracked"
        );
    }

    #[test]
    fn a_second_run_only_adds_what_is_new() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("local");
        write(&dir.join(".env"), "1");
        let worktree = tmp.path().join("main");
        std::fs::create_dir_all(&worktree).unwrap();
        let l = local(&dir, &[(".*", LocalMode::Copy)]);

        l.place(&Runner::default(), &repo(), &worktree, true);
        std::fs::write(worktree.join(".env"), "edited here").unwrap();
        write(&dir.join(".npmrc"), "token");
        l.place(&Runner::default(), &repo(), &worktree, false);

        assert_eq!(
            std::fs::read_to_string(worktree.join(".env")).unwrap(),
            "edited here"
        );
        assert!(worktree.join(".npmrc").is_file());
    }

    #[test]
    fn an_unmatched_directory_is_still_searched_for_matching_files() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("local");
        write(&dir.join("apps/web/.env"), "web");
        write(&dir.join("apps/web/other"), "no");
        let worktree = tmp.path().join("main");
        std::fs::create_dir_all(&worktree).unwrap();

        local(&dir, &[(r".*/\.env", LocalMode::Link)]).place(
            &Runner::default(),
            &repo(),
            &worktree,
            true,
        );
        assert!(worktree.join("apps/web/.env").is_file());
        assert!(!worktree.join("apps/web/other").exists());
    }

    #[test]
    fn dry_run_touches_nothing() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("local");
        write(&dir.join(".env"), "1");
        let worktree = tmp.path().join("main");
        std::fs::create_dir_all(&worktree).unwrap();

        local(&dir, &[(".*", LocalMode::Link)]).place(
            &Runner::new(true, false),
            &repo(),
            &worktree,
            true,
        );
        assert!(!worktree.join(".env").exists());
    }
}
