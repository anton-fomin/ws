//! Interactive workspace/branch selection through fzf.

use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::{PickerMode, Popup, Workspace};
use crate::exec::{program_available, Cmd, Runner};
use crate::integration::Zoxide;
use crate::workspace;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Candidate {
    /// A branch of the current workspace, already checked out or not.
    Branch { branch: String, exists: bool },
    /// Another workspace root found through zoxide.
    Workspace { root: PathBuf },
}

impl Candidate {
    /// Stable machine-readable form, also what `ws __preview` receives.
    pub fn payload(&self) -> String {
        match self {
            Candidate::Branch { branch, .. } => format!("branch:{branch}"),
            Candidate::Workspace { root } => format!("ws:{}", root.display()),
        }
    }

    pub fn parse(payload: &str) -> Option<Candidate> {
        if let Some(branch) = payload.strip_prefix("branch:") {
            return Some(Candidate::Branch {
                branch: branch.to_string(),
                exists: false,
            });
        }
        payload.strip_prefix("ws:").map(|p| Candidate::Workspace {
            root: PathBuf::from(p),
        })
    }

    fn display(&self) -> String {
        match self {
            Candidate::Branch {
                branch,
                exists: true,
            } => format!("● {branch}"),
            Candidate::Branch {
                branch,
                exists: false,
            } => format!("  {branch}"),
            Candidate::Workspace { root } => format!("⌂ {}", root.display()),
        }
    }

    /// `display \t payload`; fzf shows the first field and previews the second.
    fn line(&self) -> String {
        format!("{}\t{}", self.display(), self.payload())
    }
}

/// Checked-out branches first, then the remote branches not yet materialised.
pub fn workspace_candidates(ws: &Workspace, runner: &Runner) -> Result<Vec<Candidate>> {
    let existing: Vec<String> = workspace::list(ws, runner, false)?
        .into_iter()
        .filter_map(|b| b.branch)
        .collect();
    let mut out: Vec<Candidate> = existing
        .iter()
        .map(|branch| Candidate::Branch {
            branch: branch.clone(),
            exists: true,
        })
        .collect();
    for branch in workspace::available_branches(ws, runner)? {
        if !existing.contains(&branch) {
            out.push(Candidate::Branch {
                branch,
                exists: false,
            });
        }
    }
    Ok(out)
}

/// Workspace roots from the zoxide database.
pub fn global_candidates(
    zoxide: &Zoxide,
    runner: &Runner,
    query: &[String],
    skip: Option<&Path>,
) -> Result<Vec<Candidate>> {
    Ok(zoxide
        .known_workspaces(runner, query)?
        .into_iter()
        .filter(|root| Some(root.as_path()) != skip)
        .map(|root| Candidate::Workspace { root })
        .collect())
}

pub struct Picker {
    command: Vec<String>,
    mode: PickerMode,
    popup: Popup,
}

impl Picker {
    pub fn new(command: Vec<String>, mode: PickerMode, popup: Popup) -> Picker {
        Picker {
            command,
            mode,
            popup,
        }
    }

    /// `auto` prefers a popup, falls back to inline; `popup` degrades to
    /// inline on backends without a popup primitive (herdr).
    pub fn resolve(&self, has_popup: bool) -> PickerMode {
        let available = self
            .command
            .first()
            .map(|p| program_available(p))
            .unwrap_or(false);
        if !available {
            return PickerMode::None;
        }
        match self.mode {
            PickerMode::None => PickerMode::None,
            PickerMode::Inline => PickerMode::Inline,
            PickerMode::Popup | PickerMode::Auto => {
                if has_popup {
                    PickerMode::Popup
                } else {
                    PickerMode::Inline
                }
            }
        }
    }

    pub fn popup(&self) -> &Popup {
        &self.popup
    }

    /// The fzf invocation, with a preview that calls back into `ws`.
    pub fn filter_cmd(&self, self_exe: &Path, inline: bool) -> Result<Cmd> {
        let preview = format!(
            "{} __preview {{2}}",
            shell_words::quote(&self_exe.to_string_lossy())
        );
        let mut cmd = Cmd::launcher(&self.command)?.args([
            "--delimiter=\t".to_string(),
            "--with-nth=1".to_string(),
            "--ansi".to_string(),
            "--no-sort".to_string(),
            "--prompt=ws> ".to_string(),
            format!("--preview={preview}"),
            "--preview-window=right,55%".to_string(),
        ]);
        if inline {
            cmd = cmd.args(["--height=40%", "--reverse"]);
        }
        Ok(cmd)
    }

    /// Feed candidates to fzf and return the chosen one.
    pub fn choose(
        &self,
        self_exe: &Path,
        inline: bool,
        candidates: &[Candidate],
    ) -> Result<Option<Candidate>> {
        if candidates.is_empty() {
            return Ok(None);
        }
        let cmd = self.filter_cmd(self_exe, inline)?;
        let input: String = candidates
            .iter()
            .map(|c| c.line())
            .collect::<Vec<_>>()
            .join("\n");
        let Some(selected) = run_filter(&cmd, &input)? else {
            return Ok(None);
        };
        let payload = selected.rsplit('\t').next().unwrap_or_default().trim();
        Ok(Candidate::parse(payload))
    }
}

/// Run an interactive filter: stdin is piped, stderr stays on the terminal so
/// the UI is drawn, and stdout carries the selection.
fn run_filter(cmd: &Cmd, input: &str) -> Result<Option<String>> {
    let mut child = Command::new(&cmd.program)
        .args(&cmd.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("cannot run `{}`", cmd.display()))?;
    child
        .stdin
        .take()
        .context("filter has no stdin")?
        .write_all(input.as_bytes())
        .ok();
    let out = child.wait_with_output()?;
    if !out.status.success() {
        // fzf exits non-zero when the user aborts.
        return Ok(None);
    }
    let selection = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok((!selection.is_empty()).then_some(selection))
}

/// Text shown in the fzf preview pane.
pub fn preview(ws: Option<&Workspace>, runner: &Runner, payload: &str) -> String {
    match Candidate::parse(payload) {
        Some(Candidate::Branch { branch, .. }) => match ws {
            Some(ws) => preview_branch(ws, runner, &branch),
            None => format!("branch {branch}"),
        },
        Some(Candidate::Workspace { root }) => preview_workspace(&root),
        None => String::new(),
    }
}

fn preview_branch(ws: &Workspace, runner: &Runner, branch: &str) -> String {
    let grouped = ws.is_grouped();
    let mode = ws.config.branch_dirs;
    let base = ws.worktree_root();
    let dir = crate::paths::branch_dir(&base, branch, mode);
    let mut out = format!("branch  {branch}\ndir     {}\n\n", dir.display());
    for repo in &ws.config.repos {
        let name = repo.name();
        let path = crate::paths::worktree_path(&base, branch, mode, grouped, &repo.dir());
        if !path.join(".git").exists() {
            out.push_str(&format!("{name:<12} (not checked out)\n"));
            continue;
        }
        let head = crate::git::current_branch(runner, &path).unwrap_or_else(|| "?".into());
        let dirty = if crate::git::is_dirty(runner, &path, ws.logs_in_worktree().as_deref()) {
            " dirty"
        } else {
            ""
        };
        let sync = match crate::git::ahead_behind(runner, &path) {
            Some((ahead, behind)) if ahead > 0 || behind > 0 => format!(" +{ahead}/-{behind}"),
            _ => String::new(),
        };
        out.push_str(&format!("{name:<12} {head}{sync}{dirty}\n"));
    }
    out
}

fn preview_workspace(root: &Path) -> String {
    match Workspace::load(root) {
        Ok(ws) => {
            let repos: Vec<String> = ws.config.repos.iter().map(|r| r.name()).collect();
            format!(
                "workspace {}\nroot      {}\nrepos     {}\n",
                ws.name(),
                root.display(),
                if repos.is_empty() {
                    "-".to_string()
                } else {
                    repos.join(", ")
                }
            )
        }
        Err(e) => format!("{}\n\n{e:#}", root.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picker(mode: PickerMode) -> Picker {
        Picker::new(vec!["fzf".into()], mode, Popup::default())
    }

    #[test]
    fn payload_round_trips() {
        let branch = Candidate::Branch {
            branch: "feature/x".into(),
            exists: true,
        };
        assert_eq!(branch.payload(), "branch:feature/x");
        assert_eq!(
            Candidate::parse("branch:feature/x"),
            Some(Candidate::Branch {
                branch: "feature/x".into(),
                exists: false
            })
        );
        let ws = Candidate::Workspace {
            root: PathBuf::from("/w/acme"),
        };
        assert_eq!(ws.payload(), "ws:/w/acme");
        assert_eq!(Candidate::parse("ws:/w/acme"), Some(ws));
        assert_eq!(Candidate::parse("garbage"), None);
    }

    #[test]
    fn lines_hide_the_payload_behind_a_tab() {
        let c = Candidate::Branch {
            branch: "main".into(),
            exists: true,
        };
        let line = c.line();
        let mut fields = line.split('\t');
        assert_eq!(fields.next().unwrap(), "● main");
        assert_eq!(fields.next().unwrap(), "branch:main");
    }

    #[test]
    fn filter_command_points_preview_back_at_ws() {
        let cmd = picker(PickerMode::Auto)
            .filter_cmd(Path::new("/usr/bin/ws"), true)
            .unwrap();
        assert_eq!(cmd.program, "fzf");
        assert!(cmd
            .args
            .contains(&"--preview=/usr/bin/ws __preview {2}".to_string()));
        assert!(cmd.args.contains(&"--height=40%".to_string()));
        assert!(cmd.args.contains(&"--with-nth=1".to_string()));

        let popup_cmd = picker(PickerMode::Auto)
            .filter_cmd(Path::new("/usr/bin/ws"), false)
            .unwrap();
        assert!(!popup_cmd.args.contains(&"--height=40%".to_string()));
    }

    #[test]
    fn empty_candidate_list_selects_nothing() {
        let chosen = picker(PickerMode::Auto)
            .choose(Path::new("/usr/bin/ws"), true, &[])
            .unwrap();
        assert!(chosen.is_none());
    }
}
