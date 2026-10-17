//! Command construction and execution.
//!
//! Every external call is built as a [`Cmd`] first and executed by a [`Runner`].
//! That split is what makes `--dry-run` and the command-shape unit tests possible.
//!
//! Mutating calls go through [`Runner::run`] / [`Runner::capture_mut`] and are
//! suppressed under `--dry-run`; read-only queries ([`Runner::capture`],
//! [`Runner::try_capture`], [`Runner::ok`]) always execute, so a dry run still
//! reports what it would do against real repository state.

use anyhow::{bail, Context, Result};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cmd {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}

impl Cmd {
    /// Build from a configured launcher: `["nix", "run", "nixpkgs#herdr", "--"]`
    /// becomes the program plus its fixed leading arguments.
    pub fn launcher(parts: &[String]) -> Result<Self> {
        let (program, fixed) = parts
            .split_first()
            .context("command must have at least one element")?;
        Ok(Cmd {
            program: program.clone(),
            args: fixed.to_vec(),
            cwd: None,
        })
    }

    pub fn new(program: impl Into<String>) -> Self {
        Cmd {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
        }
    }

    pub fn arg(mut self, a: impl Into<String>) -> Self {
        self.args.push(a.into());
        self
    }

    pub fn args<I, S>(mut self, it: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(it.into_iter().map(Into::into));
        self
    }

    pub fn path_arg(self, p: impl AsRef<Path>) -> Self {
        let p = p.as_ref().to_string_lossy().into_owned();
        self.arg(p)
    }

    pub fn cwd(mut self, p: impl Into<PathBuf>) -> Self {
        self.cwd = Some(p.into());
        self
    }

    /// Shell-quoted rendering, used by `--dry-run` and error messages.
    pub fn display(&self) -> String {
        let mut parts = Vec::with_capacity(self.args.len() + 1);
        parts.push(self.program.clone());
        parts.extend(self.args.iter().cloned());
        shell_words::join(parts.iter().map(String::as_str))
    }

    fn build(&self) -> Command {
        let mut c = Command::new(&self.program);
        c.args(self.args.iter().map(OsStr::new));
        if let Some(dir) = &self.cwd {
            c.current_dir(dir);
        }
        c
    }

    fn spawn_error(&self, e: std::io::Error) -> anyhow::Error {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!("`{}` not found in PATH", self.program)
        } else {
            anyhow::Error::new(e).context(format!("failed to run `{}`", self.display()))
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Runner {
    pub dry_run: bool,
    pub verbose: bool,
}

impl Runner {
    pub fn new(dry_run: bool, verbose: bool) -> Self {
        Runner { dry_run, verbose }
    }

    fn trace(&self, cmd: &Cmd, skipped: bool) {
        if skipped {
            eprintln!("would run: {}", cmd.display());
        } else if self.verbose {
            eprintln!("+ {}", cmd.display());
        }
    }

    /// Run a mutating command, streaming its output to the terminal.
    pub fn run(&self, cmd: &Cmd) -> Result<()> {
        if self.dry_run {
            self.trace(cmd, true);
            return Ok(());
        }
        self.trace(cmd, false);
        let status = cmd.build().status().map_err(|e| cmd.spawn_error(e))?;
        if !status.success() {
            bail!("`{}` failed with {}", cmd.display(), status);
        }
        Ok(())
    }

    /// Run a mutating command whose stdout is noise (herdr answers every
    /// control command with JSON). stderr stays visible so errors surface.
    pub fn run_quiet(&self, cmd: &Cmd) -> Result<()> {
        if self.dry_run {
            self.trace(cmd, true);
            return Ok(());
        }
        self.trace(cmd, false);
        let status = cmd
            .build()
            .stdout(Stdio::null())
            .status()
            .map_err(|e| cmd.spawn_error(e))?;
        if !status.success() {
            bail!("`{}` failed with {}", cmd.display(), status);
        }
        Ok(())
    }

    /// Run a read-only command and capture stdout. Always executes.
    pub fn capture(&self, cmd: &Cmd) -> Result<String> {
        self.try_capture(cmd)?
            .with_context(|| format!("`{}` failed", cmd.display()))
    }

    /// Like [`capture`](Self::capture) but returns `None` on a non-zero exit.
    pub fn try_capture(&self, cmd: &Cmd) -> Result<Option<String>> {
        self.trace(cmd, false);
        let out = cmd
            .build()
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| cmd.spawn_error(e))?;
        if !out.status.success() {
            return Ok(None);
        }
        Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()))
    }

    /// Run a mutating command and capture stdout. Under `--dry-run` the command
    /// is only printed and `placeholder` is returned, so callers that need an id
    /// from the output can keep going.
    pub fn capture_mut(&self, cmd: &Cmd, placeholder: &str) -> Result<String> {
        if self.dry_run {
            self.trace(cmd, true);
            return Ok(placeholder.to_string());
        }
        self.trace(cmd, false);
        let out = cmd
            .build()
            .stderr(Stdio::inherit())
            .output()
            .map_err(|e| cmd.spawn_error(e))?;
        if !out.status.success() {
            bail!("`{}` failed with {}", cmd.display(), out.status);
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Exit status of a read-only probe, with output discarded.
    pub fn ok(&self, cmd: &Cmd) -> bool {
        self.trace(cmd, false);
        cmd.build()
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Mutating command whose failure is not fatal (used for zoxide).
    pub fn run_best_effort(&self, cmd: &Cmd) {
        if let Err(e) = self.run(cmd) {
            eprintln!("warning: {e:#}");
        }
    }

    /// Replace the current process. Used to hand the terminal over to
    /// `tmux attach-session`.
    pub fn exec(&self, cmd: &Cmd) -> Result<()> {
        if self.dry_run {
            self.trace(cmd, true);
            return Ok(());
        }
        self.trace(cmd, false);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let err = cmd.build().exec();
            Err(cmd.spawn_error(err))
        }
        #[cfg(not(unix))]
        self.run(cmd)
    }
}

/// Whether a program can be found and executed, without running it.
pub fn program_available(program: &str) -> bool {
    let is_exec = |p: &Path| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            p.metadata()
                .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        }
        #[cfg(not(unix))]
        p.is_file()
    };

    if program.contains(std::path::MAIN_SEPARATOR) {
        return is_exec(Path::new(program));
    }
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| is_exec(&dir.join(program)))
}

/// Parse a launcher override from the environment, e.g. `WS_HERDR_CMD`.
pub fn launcher_from_env(var: &str) -> Option<Vec<String>> {
    let raw = std::env::var(var).ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    match shell_words::split(raw) {
        Ok(parts) if !parts.is_empty() => Some(parts),
        _ => {
            eprintln!("warning: ignoring malformed {var}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_quotes_arguments() {
        let cmd = Cmd::new("tmux").args(["new-session", "-s", "my ws"]);
        assert_eq!(cmd.display(), "tmux new-session -s 'my ws'");
    }

    #[test]
    fn launcher_keeps_fixed_arguments() {
        let parts: Vec<String> = ["nix", "run", "nixpkgs#herdr", "--"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let cmd = Cmd::launcher(&parts).unwrap().args(["workspace", "list"]);
        assert_eq!(cmd.program, "nix");
        assert_eq!(
            cmd.args,
            ["run", "nixpkgs#herdr", "--", "workspace", "list"]
        );
    }

    #[test]
    fn launcher_rejects_empty() {
        assert!(Cmd::launcher(&[]).is_err());
    }
}
