//! zoxide integration: register worktrees so `z <branch>` jumps into them, and
//! use the zoxide database to find other workspaces on the machine.

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::config::{Register, ZoxideConfig, CONFIG_FILE};
use crate::exec::{launcher_from_env, program_available, Cmd, Runner};

pub struct Zoxide {
    command: Vec<String>,
    enabled: bool,
    register: Register,
}

impl Zoxide {
    pub fn new(config: &ZoxideConfig) -> Zoxide {
        let command = launcher_from_env("WS_ZOXIDE_CMD").unwrap_or_else(|| config.command.clone());
        let available = command
            .first()
            .map(|p| program_available(p))
            .unwrap_or(false);
        Zoxide {
            enabled: config.enabled.resolve(available),
            register: config.register,
            command,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn base(&self) -> Option<Cmd> {
        if !self.enabled {
            return None;
        }
        Cmd::launcher(&self.command).ok()
    }

    /// Which of the branch directory and the per-repo worktrees to register.
    pub fn paths_to_register<'a>(
        &self,
        branch_dir: &'a Path,
        worktrees: &'a [PathBuf],
    ) -> Vec<&'a Path> {
        let mut paths: Vec<&Path> = Vec::new();
        let include_dir = matches!(self.register, Register::BranchDirs | Register::Both);
        let include_wt = matches!(self.register, Register::Worktrees | Register::Both);
        if include_dir {
            paths.push(branch_dir);
        }
        if include_wt {
            for wt in worktrees {
                if wt != branch_dir {
                    paths.push(wt.as_path());
                }
            }
        }
        paths
    }

    /// Best effort: a zoxide failure must never fail a switch.
    pub fn add(&self, runner: &Runner, paths: &[&Path]) {
        if paths.is_empty() {
            return;
        }
        let Some(base) = self.base() else { return };
        let mut cmd = base.arg("add");
        for p in paths {
            cmd = cmd.path_arg(p);
        }
        runner.run_best_effort(&cmd);
    }

    pub fn remove(&self, runner: &Runner, paths: &[&Path]) {
        if paths.is_empty() {
            return;
        }
        let Some(base) = self.base() else { return };
        let mut cmd = base.arg("remove");
        for p in paths {
            cmd = cmd.path_arg(p);
        }
        runner.run_best_effort(&cmd);
    }

    /// Directories from the zoxide database that look like workspace roots.
    pub fn known_workspaces(&self, runner: &Runner, query: &[String]) -> Result<Vec<PathBuf>> {
        let Some(base) = self.base() else {
            return Ok(Vec::new());
        };
        let cmd = base.args(["query", "--list"]).args(query.iter().cloned());
        let Some(out) = runner.try_capture(&cmd)? else {
            return Ok(Vec::new());
        };
        Ok(filter_workspace_roots(&out))
    }
}

/// Keep only the listed directories that hold a `ws.yaml`.
fn filter_workspace_roots(list: &str) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    list.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.join(CONFIG_FILE).is_file())
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{NamedToggle, Toggle};

    fn zoxide_with(register: Register) -> Zoxide {
        Zoxide {
            command: vec!["zoxide".into()],
            enabled: true,
            register,
        }
    }

    #[test]
    fn register_modes_select_paths() {
        let dir = PathBuf::from("/w/feature-x");
        let wts = vec![
            PathBuf::from("/w/feature-x/api"),
            PathBuf::from("/w/feature-x/web"),
        ];

        let both = zoxide_with(Register::Both);
        assert_eq!(both.paths_to_register(&dir, &wts).len(), 3);

        let only_dirs = zoxide_with(Register::BranchDirs);
        assert_eq!(only_dirs.paths_to_register(&dir, &wts), [dir.as_path()]);

        let only_wts = zoxide_with(Register::Worktrees);
        assert_eq!(only_wts.paths_to_register(&dir, &wts).len(), 2);

        let none = zoxide_with(Register::None);
        assert!(none.paths_to_register(&dir, &wts).is_empty());
    }

    #[test]
    fn single_repo_layout_does_not_duplicate_the_branch_dir() {
        let dir = PathBuf::from("/w/feature-x");
        let wts = vec![dir.clone()];
        let both = zoxide_with(Register::Both);
        assert_eq!(both.paths_to_register(&dir, &wts), [dir.as_path()]);
    }

    #[test]
    fn disabled_zoxide_issues_no_commands() {
        let cfg = ZoxideConfig {
            enabled: Toggle::Bool(false),
            ..ZoxideConfig::default()
        };
        let z = Zoxide::new(&cfg);
        assert!(!z.is_enabled());
        assert!(z.base().is_none());
    }

    #[test]
    fn auto_follows_availability() {
        assert!(Toggle::Named(NamedToggle::Auto).resolve(true));
        assert!(!Toggle::Named(NamedToggle::Auto).resolve(false));
    }
}
