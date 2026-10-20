//! Package managers: yarn, pnpm, npm and uv.
//!
//! A new worktree is a checkout without its dependencies, and installing them
//! is what stands between `ws switch` and being able to run anything. So each
//! manager, when it is on `PATH` and the repository is recognisably its, does
//! two things in every worktree `ws` creates:
//!
//! 1. **Seeds** the dependency directory by hard-linking the one from the
//!    branch you switched from, or from the default branch — see
//!    [`super::link`]. Most of a `node_modules` is identical between two
//!    branches of the same repository, and a hard link costs an inode.
//! 2. **Installs**, held to the lockfile, so what the seeding left behind is
//!    reconciled against what the branch actually asks for and the result is
//!    the same as installing from nothing.
//!
//! Neither step may fail a switch. The worktree is git's work and it is
//! already there; a manager that cannot finish says so and leaves the install
//! to you, in the session that is about to open.

use std::path::{Path, PathBuf};

use crate::config::{Integrations, Manager, PackageManagerConfig};
use crate::exec::{launcher_from_env, program_available, Cmd, Runner};

use super::link;

/// One package manager that is enabled and present, with the launcher it was
/// resolved to.
#[derive(Debug, Clone)]
struct Enabled {
    manager: Manager,
    command: Vec<String>,
    /// Configured arguments, when the workspace named its own.
    install: Option<Vec<String>>,
    link: bool,
}

/// The package managers `ws` will drive, resolved once per run.
#[derive(Debug, Clone, Default)]
pub struct PackageManagers {
    enabled: Vec<Enabled>,
}

impl PackageManagers {
    pub fn new(config: &Integrations) -> PackageManagers {
        let enabled = config
            .package_managers()
            .into_iter()
            .filter_map(|(manager, settings)| resolve(manager, settings))
            .collect();
        PackageManagers { enabled }
    }

    /// Names of the enabled managers, as `ws status` lists them.
    pub fn names(&self) -> Vec<&'static str> {
        self.enabled.iter().map(|e| e.manager.name()).collect()
    }

    /// Seed and install in `worktree`, for whichever managers recognise it.
    ///
    /// Skipped where the dependency directory is already there, which is what
    /// makes this safe to call on every switch; `force` — `--rerun-setup` —
    /// installs again anyway.
    pub fn prepare(
        &self,
        runner: &Runner,
        repo: &str,
        worktree: &Path,
        seeds: &[PathBuf],
        force: bool,
    ) {
        for enabled in self.detect(worktree) {
            let dependencies = worktree.join(enabled.manager.dependency_dir());
            if dependencies.exists() && !force {
                continue;
            }
            if enabled.link {
                seed(runner, repo, enabled.manager, worktree, seeds);
            }
            enabled.install(runner, repo, worktree);
        }
    }

    /// The managers this worktree belongs to.
    ///
    /// At most one of the three that share a `node_modules` — they are
    /// alternatives to one another — plus uv, which is independent of them:
    /// a repository with a Python service and a web front end has both.
    fn detect(&self, worktree: &Path) -> Vec<&Enabled> {
        let mut detected = Vec::new();
        if let Some(node) = self.detect_node(worktree) {
            detected.push(node);
        }
        if let Some(uv) = self.find(Manager::Uv) {
            if marked_by(worktree, Manager::Uv) {
                detected.push(uv);
            }
        }
        detected
    }

    /// Which of yarn, pnpm and npm a repository is run with.
    ///
    /// The `packageManager` field of its `package.json` is the project's own
    /// statement — it is what corepack reads — so it settles the question
    /// outright. Without one it is the lockfile that is on disk.
    fn detect_node(&self, worktree: &Path) -> Option<&Enabled> {
        if !worktree.join("package.json").is_file() {
            return None;
        }
        if let Some(declared) = declared_manager(worktree) {
            if self.find(declared).is_none() {
                eprintln!(
                    "warning: {} asks for {}, which is not enabled or not installed",
                    worktree.display(),
                    declared.name()
                );
            }
            return self.find(declared);
        }

        let marked: Vec<Manager> = Manager::ALL
            .into_iter()
            .filter(|m| m.is_node() && marked_by(worktree, *m))
            .collect();
        if marked.len() > 1 {
            let names: Vec<&str> = marked.iter().map(|m| m.name()).collect();
            eprintln!(
                "warning: {} has lockfiles for {} — using {}; \
                 set packageManager in package.json to say which",
                worktree.display(),
                names.join(" and "),
                names[0],
            );
        }
        marked.into_iter().find_map(|m| self.find(m))
    }

    fn find(&self, manager: Manager) -> Option<&Enabled> {
        self.enabled.iter().find(|e| e.manager == manager)
    }
}

/// A manager as this machine can actually run it, or nothing.
fn resolve(manager: Manager, settings: &PackageManagerConfig) -> Option<Enabled> {
    let command =
        launcher_from_env(manager.command_env()).unwrap_or_else(|| settings.command(manager));
    let available = command
        .first()
        .map(|program| program_available(program))
        .unwrap_or(false);
    if !settings.enabled.resolve(available) {
        return None;
    }
    Some(Enabled {
        manager,
        command,
        install: settings.install.clone(),
        link: settings.link(manager),
    })
}

impl Enabled {
    fn install(&self, runner: &Runner, repo: &str, worktree: &Path) {
        let args = match &self.install {
            Some(args) => args.clone(),
            None => default_install_args(runner, self.manager, &self.command, worktree),
        };
        let Ok(base) = Cmd::launcher(&self.command) else {
            return;
        };
        let cmd = base.args(args).cwd(worktree);
        eprintln!("  [{repo}] {}", cmd.display());
        if let Err(e) = runner.run(&cmd) {
            eprintln!("warning: {e:#} — install it yourself in the new worktree");
        }
    }
}

/// Arguments that install exactly what the lockfile says, for a manager the
/// workspace has not given its own.
///
/// Every one of these fails rather than writing a new lockfile, so two
/// worktrees of the same commit get the same dependencies.
pub fn default_install_args(
    runner: &Runner,
    manager: Manager,
    command: &[String],
    worktree: &Path,
) -> Vec<String> {
    let args: &[&str] = match manager {
        Manager::Yarn => &["install", yarn_immutable_flag(runner, command, worktree)],
        Manager::Pnpm => &["install", "--frozen-lockfile"],
        Manager::Npm => &["ci"],
        Manager::Uv => &["sync"],
    };
    args.iter().map(|a| a.to_string()).collect()
}

/// Yarn 2 and later take `--immutable`; 1.x only knows `--frozen-lockfile`,
/// and refuses to run when handed a flag it does not.
///
/// Which one a project gets is the project's own choice — corepack installs
/// the version its `packageManager` field names — so yarn is asked inside the
/// worktree rather than once for the machine. A version that cannot be read
/// falls back to the older spelling, which yarn 2+ still accepts.
fn yarn_immutable_flag(runner: &Runner, command: &[String], worktree: &Path) -> &'static str {
    match yarn_major(runner, command, worktree) {
        Some(major) if major >= 2 => "--immutable",
        _ => "--frozen-lockfile",
    }
}

fn yarn_major(runner: &Runner, command: &[String], worktree: &Path) -> Option<u32> {
    let cmd = Cmd::launcher(command).ok()?.arg("--version").cwd(worktree);
    let reported = runner.try_capture(&cmd).ok()??;
    reported.trim().split('.').next()?.parse().ok()
}

/// Whether one of the manager's lockfiles is in the worktree.
fn marked_by(worktree: &Path, manager: Manager) -> bool {
    manager
        .lockfiles()
        .iter()
        .any(|name| worktree.join(name).is_file())
}

/// The manager named by `package.json`'s `packageManager` field, as corepack
/// writes it: `"pnpm@9.1.0+sha512..."`.
fn declared_manager(worktree: &Path) -> Option<Manager> {
    let text = std::fs::read_to_string(worktree.join("package.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let declared = json.get("packageManager")?.as_str()?;
    let name = declared.split('@').next()?.trim();
    Manager::ALL
        .into_iter()
        .find(|m| m.is_node() && m.name() == name)
}

/// Hard-link the dependency directories of the first seed that has them.
///
/// One worktree is the source for all of them: taking the root `node_modules`
/// from one branch and a package's from another would mix two resolutions of
/// the same lockfile.
fn seed(runner: &Runner, repo: &str, manager: Manager, worktree: &Path, seeds: &[PathBuf]) {
    let name = manager.dependency_dir();
    let Some(source) = seeds.iter().find(|seed| seed.join(name).is_dir()) else {
        return;
    };

    let mut total = link::Stats::default();
    let mut seeded = 0;
    for relative in link::find(source, name) {
        let target = worktree.join(&relative);
        if target.exists() {
            continue;
        }
        if runner.dry_run {
            eprintln!(
                "would hard-link {} -> {}",
                source.join(&relative).display(),
                target.display()
            );
            seeded += 1;
            continue;
        }
        match link::tree(&source.join(&relative), &target) {
            Ok(stats) => {
                total.dirs += stats.dirs;
                total.linked += stats.linked;
                total.copied += stats.copied;
                total.symlinks += stats.symlinks;
                seeded += 1;
            }
            Err(e) => {
                // A tree that is only half there is worse than none: the
                // install would take it for a complete one.
                let _ = std::fs::remove_dir_all(&target);
                eprintln!("warning: cannot seed {}: {e:#}", target.display());
            }
        }
    }

    if seeded == 0 || runner.dry_run {
        return;
    }
    let what = if seeded == 1 {
        name.to_string()
    } else {
        format!("{seeded} {name} directories")
    };
    eprintln!(
        "  [{repo}] seeded {what} from {} ({} files)",
        source.display(),
        total.files()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Toggle;
    use tempfile::TempDir;

    fn managers(yaml: &str) -> PackageManagers {
        let config = crate::config::parse(yaml).unwrap();
        PackageManagers::new(&config.integrations)
    }

    /// Every manager enabled regardless of what is installed on the machine
    /// running the tests, so detection is what is under test.
    fn all() -> PackageManagers {
        managers(
            "integrations:\n\
             \x20 yarn: { enabled: true }\n\
             \x20 pnpm: { enabled: true }\n\
             \x20 npm: { enabled: true }\n\
             \x20 uv: { enabled: true }\n",
        )
    }

    /// What `managers` detects in `dir`, by name rather than by reference.
    fn detected(managers: &PackageManagers, dir: &Path) -> Vec<Manager> {
        managers.detect(dir).iter().map(|e| e.manager).collect()
    }

    fn project(files: &[(&str, &str)]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        for (name, body) in files {
            std::fs::write(tmp.path().join(name), body).unwrap();
        }
        tmp
    }

    #[test]
    fn a_lockfile_picks_the_manager() {
        for (lockfile, want) in [
            ("yarn.lock", Manager::Yarn),
            ("pnpm-lock.yaml", Manager::Pnpm),
            ("package-lock.json", Manager::Npm),
            ("npm-shrinkwrap.json", Manager::Npm),
        ] {
            let tmp = project(&[("package.json", "{}"), (lockfile, "")]);
            assert_eq!(detected(&all(), tmp.path()), [want], "{lockfile}");
        }
    }

    #[test]
    fn the_package_manager_field_beats_the_lockfile() {
        // A repository that switched managers and still carries the old file.
        let tmp = project(&[
            (
                "package.json",
                r#"{"packageManager":"pnpm@9.1.0+sha512.x"}"#,
            ),
            ("yarn.lock", ""),
        ]);
        assert_eq!(detected(&all(), tmp.path()), [Manager::Pnpm]);
    }

    #[test]
    fn two_lockfiles_fall_back_to_the_preferred_order() {
        let tmp = project(&[
            ("package.json", "{}"),
            ("pnpm-lock.yaml", ""),
            ("package-lock.json", ""),
        ]);
        assert_eq!(detected(&all(), tmp.path()), [Manager::Pnpm]);
    }

    #[test]
    fn a_disabled_manager_hands_over_to_the_next_lockfile() {
        let tmp = project(&[
            ("package.json", "{}"),
            ("pnpm-lock.yaml", ""),
            ("package-lock.json", ""),
        ]);
        let only_npm =
            managers("integrations:\n  pnpm: { enabled: false }\n  npm: { enabled: true }\n");
        assert_eq!(detected(&only_npm, tmp.path()), [Manager::Npm]);
    }

    #[test]
    fn node_and_uv_are_detected_side_by_side() {
        let tmp = project(&[
            ("package.json", "{}"),
            ("pnpm-lock.yaml", ""),
            ("uv.lock", ""),
        ]);
        assert_eq!(detected(&all(), tmp.path()), [Manager::Pnpm, Manager::Uv]);
    }

    #[test]
    fn a_repository_of_neither_is_left_alone() {
        let tmp = project(&[("README.md", "")]);
        assert!(all().detect(tmp.path()).is_empty());
        // A lockfile without a package.json is not a node project.
        let tmp = project(&[("yarn.lock", "")]);
        assert!(all().detect(tmp.path()).is_empty());
    }

    #[test]
    fn pyproject_alone_is_enough_for_uv() {
        let tmp = project(&[("pyproject.toml", "[project]\nname = \"x\"\n")]);
        assert_eq!(detected(&all(), tmp.path()), [Manager::Uv]);
    }

    #[test]
    fn the_defaults_hold_every_manager_to_its_lockfile() {
        let runner = Runner::default();
        let tmp = TempDir::new().unwrap();
        let args = |manager, command: &str| {
            default_install_args(&runner, manager, &[command.to_string()], tmp.path())
        };
        // No yarn to ask, so the spelling both majors accept.
        assert_eq!(
            args(Manager::Yarn, "yarn"),
            ["install", "--frozen-lockfile"]
        );
        assert_eq!(
            args(Manager::Pnpm, "pnpm"),
            ["install", "--frozen-lockfile"]
        );
        assert_eq!(args(Manager::Npm, "npm"), ["ci"]);
        assert_eq!(args(Manager::Uv, "uv"), ["sync"]);
    }

    #[test]
    fn a_configured_install_replaces_the_default() {
        let configured = managers(
            "integrations:\n  pnpm:\n    enabled: true\n    install: [install, --offline]\n",
        );
        let pnpm = configured.find(Manager::Pnpm).unwrap();
        assert_eq!(pnpm.install.as_deref().unwrap(), ["install", "--offline"]);
    }

    #[test]
    fn enablement_follows_the_configuration() {
        let only_yarn = managers(
            "integrations:\n  yarn: { enabled: true }\n  \
             pnpm: { enabled: false }\n  npm: { enabled: false }\n  uv: { enabled: false }\n",
        );
        assert_eq!(only_yarn.names(), ["yarn"]);
        assert!(only_yarn.find(Manager::Pnpm).is_none());

        // `auto` against a program that is certainly not installed.
        let missing = managers(
            "integrations:\n  yarn:\n    command: [ws-no-such-yarn]\n  \
             pnpm: { enabled: false }\n  npm: { enabled: false }\n  uv: { enabled: false }\n",
        );
        assert!(missing.names().is_empty());
        assert_eq!(
            crate::config::parse("").unwrap().integrations.yarn.enabled,
            Toggle::default()
        );
    }

    #[test]
    fn seeding_defaults_follow_what_the_install_would_do() {
        let every = all();
        assert!(every.find(Manager::Yarn).unwrap().link);
        assert!(every.find(Manager::Pnpm).unwrap().link);
        // `npm ci` would delete whatever was seeded.
        assert!(!every.find(Manager::Npm).unwrap().link);
        assert!(!every.find(Manager::Uv).unwrap().link);
    }
}
