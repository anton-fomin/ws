//! `.ws/state.yaml` — the only thing `ws` remembers on its own.
//!
//! Which branch lives in which directory is read back from git, so it cannot
//! drift. This file records only which per-repo `setup` commands already ran;
//! what a branch was started from belongs to the branch directory itself, in
//! the [`crate::marker`] it carries.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    #[serde(default, rename = "worktrees")]
    entries: Vec<Entry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    branch: String,
    #[serde(default)]
    setup_done: Vec<String>,
}

/// Parse a state file. An emptied one holds no YAML node at all, which is
/// null — read that as the state of having remembered nothing.
fn parse(text: &str) -> Result<State> {
    Ok(serde_yaml_ng::from_str::<Option<State>>(text)?.unwrap_or_default())
}

impl State {
    pub fn load(path: &Path) -> Result<State> {
        match std::fs::read_to_string(path) {
            Ok(text) => parse(&text).with_context(|| format!("cannot parse {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
            Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        let text = serde_yaml_ng::to_string(self).context("cannot serialize state")?;
        std::fs::write(path, text).with_context(|| format!("cannot write {}", path.display()))
    }

    pub fn setup_done(&self, branch: &str, repo: &str) -> bool {
        self.entries
            .iter()
            .any(|e| e.branch == branch && e.setup_done.iter().any(|r| r == repo))
    }

    pub fn mark_setup_done(&mut self, branch: &str, repo: &str) {
        match self.entries.iter_mut().find(|e| e.branch == branch) {
            Some(entry) => {
                if !entry.setup_done.iter().any(|r| r == repo) {
                    entry.setup_done.push(repo.to_string());
                }
            }
            None => self.entries.push(Entry {
                branch: branch.to_string(),
                setup_done: vec![repo.to_string()],
            }),
        }
    }

    pub fn forget(&mut self, branch: &str) {
        self.entries.retain(|e| e.branch != branch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_forgets_setup() {
        let mut state = State::default();
        assert!(!state.setup_done("feature/x", "api"));
        state.mark_setup_done("feature/x", "api");
        state.mark_setup_done("feature/x", "api");
        state.mark_setup_done("feature/x", "web");
        assert!(state.setup_done("feature/x", "api"));
        assert!(state.setup_done("feature/x", "web"));
        assert!(!state.setup_done("main", "api"));
        state.forget("feature/x");
        assert!(!state.setup_done("feature/x", "api"));
    }

    #[test]
    fn round_trips_through_yaml() {
        let mut state = State::default();
        state.mark_setup_done("feature/x", "api");
        let text = serde_yaml_ng::to_string(&state).unwrap();
        let back = parse(&text).unwrap();
        assert!(back.setup_done("feature/x", "api"));
    }

    #[test]
    fn missing_file_is_an_empty_state() {
        let state = State::load(Path::new("/nonexistent/ws/state.yaml")).unwrap();
        assert!(!state.setup_done("x", "y"));
    }

    #[test]
    fn an_emptied_file_is_an_empty_state() {
        // Deleting the entries by hand leaves a document with no node in it,
        // which YAML reads as null rather than as an empty mapping.
        assert!(!parse("").unwrap().setup_done("x", "y"));
        assert!(!parse("worktrees: []\n").unwrap().setup_done("x", "y"));
    }
}
