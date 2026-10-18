//! Terminal multiplexer backends.
//!
//! Each backend turns a [`SessionPlan`] into concrete commands. Building the
//! commands is kept separate from running them so `--dry-run` and the
//! command-shape tests exercise exactly what a real run would do.

pub mod herdr;
pub mod none;
pub mod tmux;

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::config::{Backend, MuxConfig, Popup};
use crate::exec::{program_available, Cmd, Runner};

/// One repository inside a workspace session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
}

/// Everything a backend needs to build or reuse a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPlan {
    pub session: String,
    /// The branch directory; cwd of the shared window/pane.
    pub root_dir: PathBuf,
    /// Name of the shared window/pane, when one should exist.
    pub root_window: Option<String>,
    pub entries: Vec<Entry>,
}

impl SessionPlan {
    /// Windows/panes to create, in order: the shared one first when present.
    pub fn slots(&self) -> Vec<Entry> {
        let mut slots = Vec::with_capacity(self.entries.len() + 1);
        if let Some(name) = &self.root_window {
            slots.push(Entry {
                name: name.clone(),
                path: self.root_dir.clone(),
            });
        }
        slots.extend(self.entries.iter().cloned());
        slots
    }
}

pub trait Mux {
    fn name(&self) -> &'static str;

    /// Create the session if needed, then focus it.
    fn open(&self, runner: &Runner, plan: &SessionPlan) -> Result<()>;

    /// Tear the session down. Best effort: a missing session is not an error.
    fn close(&self, runner: &Runner, session: &str) -> Result<()>;

    /// Command that runs `inner` in a popup, when the backend has one.
    fn popup(&self, _popup: &Popup, _cwd: &Path, _title: &str, _inner: &Cmd) -> Option<Cmd> {
        None
    }
}

pub fn inside_tmux() -> bool {
    std::env::var("TMUX")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

pub fn inside_herdr() -> bool {
    std::env::var("HERDR_ENV")
        .map(|v| v == "1")
        .unwrap_or(false)
        || std::env::var("HERDR_PANE_ID").is_ok()
}

/// Pick a backend from config plus an optional `--mux` override.
pub fn select(config: &MuxConfig, force: Option<Backend>) -> Box<dyn Mux> {
    let tmux = || Box::new(tmux::Tmux::new(&config.tmux)) as Box<dyn Mux>;
    let herdr = || Box::new(herdr::Herdr::new(&config.herdr)) as Box<dyn Mux>;
    let none = || Box::new(none::None_::new(config.on_none)) as Box<dyn Mux>;

    match force.unwrap_or(config.backend) {
        Backend::Tmux => tmux(),
        Backend::Herdr => herdr(),
        Backend::None => none(),
        Backend::Auto => {
            if inside_tmux() {
                tmux()
            } else if inside_herdr() {
                herdr()
            } else if program_available(first(&config.tmux.command)) {
                tmux()
            } else if program_available(first(&config.herdr.command)) {
                herdr()
            } else {
                none()
            }
        }
    }
}

fn first(cmd: &[String]) -> &str {
    cmd.first().map(String::as_str).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn plan(root_window: Option<&str>, repos: &[&str]) -> SessionPlan {
        SessionPlan {
            session: "acme/feature-x".into(),
            root_dir: PathBuf::from("/w/feature-x"),
            root_window: root_window.map(str::to_string),
            entries: repos
                .iter()
                .map(|r| Entry {
                    name: r.to_string(),
                    path: PathBuf::from(format!("/w/feature-x/{r}")),
                })
                .collect(),
        }
    }

    #[test]
    fn slots_put_the_shared_window_first() {
        let p = plan(Some("ws"), &["api", "web"]);
        let names: Vec<_> = p.slots().into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["ws", "api", "web"]);

        let p = plan(None, &["api"]);
        let slots = p.slots();
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].path, PathBuf::from("/w/feature-x/api"));
    }
}
