//! tmux backend: one window per repository, named after it.

use anyhow::Result;
use std::path::Path;

use crate::config::{Popup, TmuxConfig, TmuxLayout};
use crate::exec::{launcher_from_env, Cmd, Runner};
use crate::mux::{inside_tmux, Mux, SessionPlan};
use crate::paths::sanitize_tmux_session;

pub struct Tmux {
    command: Vec<String>,
    layout: TmuxLayout,
    /// Running inside tmux, where a client can be switched instead of attached.
    inside: bool,
}

impl Tmux {
    pub fn new(config: &TmuxConfig) -> Tmux {
        Tmux {
            command: launcher_from_env("WS_TMUX_CMD").unwrap_or_else(|| config.command.clone()),
            layout: config.layout,
            inside: inside_tmux(),
        }
    }

    fn base(&self) -> Result<Cmd> {
        Cmd::launcher(&self.command)
    }

    /// `=name` asks tmux for an exact match instead of a prefix match.
    /// tmux reads `.` and `:` as separators, so the name is folded first.
    fn target(session: &str) -> String {
        format!("={}", sanitize_tmux_session(session))
    }

    fn window_target(session: &str) -> String {
        format!("{}:", Self::target(session))
    }

    pub fn has_session(&self, runner: &Runner, session: &str) -> bool {
        match self.base() {
            Ok(base) => runner.ok(&base.args(["has-session", "-t", &Self::target(session)])),
            Err(_) => false,
        }
    }

    /// Every command needed to build the session from scratch.
    pub fn create_cmds(&self, plan: &SessionPlan) -> Result<Vec<Cmd>> {
        let slots = plan.slots();
        let Some((first, rest)) = slots.split_first() else {
            return Ok(Vec::new());
        };
        let session = sanitize_tmux_session(&plan.session);
        let session = session.as_str();
        let win = Self::window_target(session);
        let mut cmds = vec![self
            .base()?
            .args(["new-session", "-d", "-s", session, "-n", &first.name, "-c"])
            .path_arg(&first.path)];

        match self.layout {
            TmuxLayout::Windows => {
                for slot in rest {
                    cmds.push(
                        self.base()?
                            .args(["new-window", "-t", &win, "-n", &slot.name, "-c"])
                            .path_arg(&slot.path),
                    );
                }
                cmds.push(
                    self.base()?
                        .args(["select-window", "-t", &format!("{win}^")]),
                );
            }
            TmuxLayout::Panes => {
                // Pane titles are only visible with a pane border status line.
                cmds.push(self.base()?.args([
                    "set-option",
                    "-t",
                    &Self::target(session),
                    "pane-border-status",
                    "top",
                ]));
                cmds.push(
                    self.base()?
                        .args(["select-pane", "-t", &win, "-T", &first.name]),
                );
                for slot in rest {
                    // split-window makes the new pane active, so the following
                    // select-pane targeting the window hits exactly that pane.
                    cmds.push(
                        self.base()?
                            .args(["split-window", "-t", &win, "-c"])
                            .path_arg(&slot.path),
                    );
                    cmds.push(
                        self.base()?
                            .args(["select-pane", "-t", &win, "-T", &slot.name]),
                    );
                }
                cmds.push(self.base()?.args(["select-layout", "-t", &win, "tiled"]));
                cmds.push(
                    self.base()?
                        .args(["select-pane", "-t", &format!("{win}.0")]),
                );
            }
        }
        Ok(cmds)
    }

    /// Inside tmux we switch the current client; outside we take over the
    /// terminal with `attach-session`.
    pub fn focus_cmd(&self, session: &str) -> Result<Cmd> {
        let base = self.base()?;
        Ok(if self.inside {
            base.args(["switch-client", "-t", &Self::target(session)])
        } else {
            base.args(["attach-session", "-t", &Self::target(session)])
        })
    }
}

impl Mux for Tmux {
    fn name(&self) -> &'static str {
        "tmux"
    }

    fn open(&self, runner: &Runner, plan: &SessionPlan) -> Result<()> {
        if !self.has_session(runner, &plan.session) {
            for cmd in self.create_cmds(plan)? {
                runner.run(&cmd)?;
            }
        }
        let focus = self.focus_cmd(&plan.session)?;
        if self.inside {
            runner.run(&focus)
        } else {
            runner.exec(&focus)
        }
    }

    fn close(&self, runner: &Runner, session: &str) -> Result<()> {
        if self.has_session(runner, session) {
            runner.run(
                &self
                    .base()?
                    .args(["kill-session", "-t", &Self::target(session)]),
            )?;
        }
        Ok(())
    }

    fn popup(&self, popup: &Popup, cwd: &Path, title: &str, inner: &Cmd) -> Option<Cmd> {
        if !self.inside {
            return None;
        }
        Some(
            self.base()
                .ok()?
                .args([
                    "display-popup",
                    "-E",
                    "-w",
                    &popup.width,
                    "-h",
                    &popup.height,
                    "-T",
                    title,
                    "-d",
                ])
                .path_arg(cwd)
                // A single shell-quoted string is the form tmux reliably runs.
                .arg(inner.display()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mux::tests::plan;

    fn tmux(layout: TmuxLayout) -> Tmux {
        Tmux {
            command: vec!["tmux".into()],
            layout,
            inside: true,
        }
    }

    #[test]
    fn windows_layout_creates_one_window_per_repo() {
        let cmds: Vec<String> = tmux(TmuxLayout::Windows)
            .create_cmds(&plan(Some("ws"), &["api", "web"]))
            .unwrap()
            .iter()
            .map(Cmd::display)
            .collect();
        assert_eq!(
            cmds,
            [
                "tmux new-session -d -s acme-feature-x -n ws -c /w/feature-x",
                "tmux new-window -t '=acme-feature-x:' -n api -c /w/feature-x/api",
                "tmux new-window -t '=acme-feature-x:' -n web -c /w/feature-x/web",
                "tmux select-window -t '=acme-feature-x:^'",
            ]
        );
    }

    #[test]
    fn single_repo_without_shared_window_creates_one_window() {
        let cmds = tmux(TmuxLayout::Windows)
            .create_cmds(&plan(None, &["api"]))
            .unwrap();
        assert_eq!(cmds.len(), 2);
        assert_eq!(
            cmds[0].display(),
            "tmux new-session -d -s acme-feature-x -n api -c /w/feature-x/api"
        );
    }

    #[test]
    fn panes_layout_splits_and_titles_each_pane() {
        let cmds: Vec<String> = tmux(TmuxLayout::Panes)
            .create_cmds(&plan(Some("ws"), &["api"]))
            .unwrap()
            .iter()
            .map(Cmd::display)
            .collect();
        assert_eq!(
            cmds,
            [
                "tmux new-session -d -s acme-feature-x -n ws -c /w/feature-x",
                "tmux set-option -t '=acme-feature-x' pane-border-status top",
                "tmux select-pane -t '=acme-feature-x:' -T ws",
                "tmux split-window -t '=acme-feature-x:' -c /w/feature-x/api",
                "tmux select-pane -t '=acme-feature-x:' -T api",
                "tmux select-layout -t '=acme-feature-x:' tiled",
                "tmux select-pane -t '=acme-feature-x:.0'",
            ]
        );
    }

    #[test]
    fn popup_wraps_the_inner_command_as_one_string() {
        let inner = Cmd::new("/usr/bin/ws").args(["pick", "--inline"]);
        let cmd = tmux(TmuxLayout::Windows)
            .popup(&Popup::default(), Path::new("/w"), " ws ", &inner)
            .expect("inside tmux there is a popup");
        assert_eq!(
            cmd.display(),
            "tmux display-popup -E -w '80%' -h '60%' -T ' ws ' -d /w '/usr/bin/ws pick --inline'"
        );
    }

    #[test]
    fn outside_tmux_there_is_no_popup_and_we_attach() {
        let mut t = tmux(TmuxLayout::Windows);
        t.inside = false;
        let inner = Cmd::new("/usr/bin/ws").arg("pick");
        assert!(t
            .popup(&Popup::default(), Path::new("/w"), " ws ", &inner)
            .is_none());
        assert_eq!(
            t.focus_cmd("acme-main").unwrap().display(),
            "tmux attach-session -t '=acme-main'"
        );
        assert_eq!(
            tmux(TmuxLayout::Windows)
                .focus_cmd("acme-main")
                .unwrap()
                .display(),
            "tmux switch-client -t '=acme-main'"
        );
    }
}
