//! Per-repo initialisation commands, run once per new worktree.

use anyhow::{Context, Result};
use std::path::Path;

use crate::exec::{Cmd, Runner};

/// Run `commands` in `worktree` through `sh -c`, streaming their output.
pub fn run(runner: &Runner, worktree: &Path, repo: &str, commands: &[String]) -> Result<()> {
    for command in commands {
        if command.trim().is_empty() {
            continue;
        }
        eprintln!("  [{repo}] {command}");
        runner
            .run(&Cmd::new("sh").args(["-c", command]).cwd(worktree))
            .with_context(|| format!("setup command for {repo} failed: {command}"))?;
    }
    Ok(())
}
