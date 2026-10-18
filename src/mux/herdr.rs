//! herdr backend: one workspace per branch, one pane per repository.
//!
//! Command shapes verified against herdr 0.9.0 (`herdr <subcommand>` with no
//! arguments prints the exact usage). Control commands answer with JSON and the
//! public ids (`w1`, `w1:t1`, `w1:p1`) are opaque, so ids are always read back
//! from `.result` rather than constructed.
//!
//! herdr also ships its own `herdr worktree create`, but that is single-repo and
//! keeps a separate notion of a workspace; `ws` owns the git side so the tmux and
//! herdr paths stay identical.

use anyhow::{Context, Result};
use serde_json::Value;
use std::path::Path;

use crate::config::{Direction, HerdrConfig};
use crate::exec::{launcher_from_env, Cmd, Runner};
use crate::mux::{inside_herdr, Mux, SessionPlan};

pub struct Herdr {
    command: Vec<String>,
    direction: Direction,
    session: Option<String>,
    /// Running inside a herdr pane, where the session context is inherited.
    inherits_context: bool,
}

impl Herdr {
    pub fn new(config: &HerdrConfig) -> Herdr {
        Herdr {
            command: launcher_from_env("WS_HERDR_CMD").unwrap_or_else(|| config.command.clone()),
            direction: config.split_direction,
            session: config.session.clone(),
            inherits_context: inside_herdr(),
        }
    }

    /// `--session` is a global flag and goes before the subcommand. Inside a
    /// pane the session/socket context is inherited, so it is left out there.
    fn base(&self) -> Result<Cmd> {
        let mut cmd = Cmd::launcher(&self.command)?;
        if !self.inherits_context {
            if let Some(session) = &self.session {
                cmd = cmd.args(["--session", session]);
            }
        }
        Ok(cmd)
    }

    pub fn list_cmd(&self) -> Result<Cmd> {
        Ok(self.base()?.args(["workspace", "list"]))
    }

    pub fn create_cmd(&self, plan: &SessionPlan) -> Result<Cmd> {
        Ok(self
            .base()?
            .args(["workspace", "create", "--cwd"])
            .path_arg(&plan.root_dir)
            .args(["--label", &plan.session]))
    }

    pub fn split_cmd(&self, pane: &str, cwd: &Path) -> Result<Cmd> {
        Ok(self
            .base()?
            .args([
                "pane",
                "split",
                pane,
                "--direction",
                self.direction.as_str(),
                "--cwd",
            ])
            .path_arg(cwd)
            .arg("--no-focus"))
    }

    pub fn rename_cmd(&self, pane: &str, label: &str) -> Result<Cmd> {
        Ok(self.base()?.args(["pane", "rename", pane, label]))
    }

    pub fn focus_cmd(&self, workspace_id: &str) -> Result<Cmd> {
        Ok(self.base()?.args(["workspace", "focus", workspace_id]))
    }

    fn workspace_id_by_label(&self, runner: &Runner, label: &str) -> Result<Option<String>> {
        let Some(out) = runner.try_capture(&self.list_cmd()?)? else {
            return Ok(None);
        };
        let Ok(json) = serde_json::from_str::<Value>(&out) else {
            return Ok(None);
        };
        Ok(find_labelled_id(&json, label, "workspace_id"))
    }
}

impl Mux for Herdr {
    fn name(&self) -> &'static str {
        "herdr"
    }

    fn open(&self, runner: &Runner, plan: &SessionPlan) -> Result<()> {
        if let Some(id) = self.workspace_id_by_label(runner, &plan.session)? {
            return runner.run_quiet(&self.focus_cmd(&id)?);
        }

        let created = runner.capture_mut(
            &self.create_cmd(plan)?,
            r#"{"result":{"workspace":{"workspace_id":"w1"},"root_pane":{"pane_id":"w1:p1"}}}"#,
        )?;
        let created: Value =
            serde_json::from_str(&created).context("herdr workspace create did not return JSON")?;
        let workspace_id = pick(&created, &["result", "workspace", "workspace_id"])
            .or_else(|| first_string(&created, "workspace_id"))
            .context("herdr workspace create did not report a workspace id")?;
        let mut pane = pick(&created, &["result", "root_pane", "pane_id"])
            .or_else(|| first_string(&created, "pane_id"))
            .context("herdr workspace create did not report a pane id")?;

        let slots = plan.slots();
        for (i, slot) in slots.iter().enumerate() {
            if i > 0 {
                let out = runner.capture_mut(
                    &self.split_cmd(&pane, &slot.path)?,
                    &format!(r#"{{"result":{{"pane":{{"pane_id":"w1:p{}"}}}}}}"#, i + 1),
                )?;
                let out: Value =
                    serde_json::from_str(&out).context("herdr pane split did not return JSON")?;
                pane = pick(&out, &["result", "pane", "pane_id"])
                    .or_else(|| first_string(&out, "pane_id"))
                    .context("herdr pane split did not report a pane id")?;
            }
            runner.run_quiet(&self.rename_cmd(&pane, &slot.name)?)?;
        }

        runner.run_quiet(&self.focus_cmd(&workspace_id)?)
    }

    fn close(&self, runner: &Runner, session: &str) -> Result<()> {
        if let Some(id) = self.workspace_id_by_label(runner, session)? {
            runner.run_quiet(&self.base()?.args(["workspace", "close", &id]))?;
        }
        Ok(())
    }
}

/// Read a nested string, e.g. `.result.workspace.workspace_id`.
fn pick(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(key)?;
    }
    current.as_str().map(str::to_string)
}

/// First value of `key` anywhere in the document, for shape changes.
fn first_string(value: &Value, key: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key).and_then(Value::as_str) {
                return Some(found.to_string());
            }
            map.values().find_map(|v| first_string(v, key))
        }
        Value::Array(items) => items.iter().find_map(|v| first_string(v, key)),
        _ => None,
    }
}

/// Id of the object whose `label` matches, searched anywhere in the document.
fn find_labelled_id(value: &Value, label: &str, id_key: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            let matches = map.get("label").and_then(Value::as_str) == Some(label);
            if matches {
                if let Some(id) = map.get(id_key).and_then(Value::as_str) {
                    return Some(id.to_string());
                }
            }
            map.values()
                .find_map(|v| find_labelled_id(v, label, id_key))
        }
        Value::Array(items) => items
            .iter()
            .find_map(|v| find_labelled_id(v, label, id_key)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mux::tests::plan;

    fn herdr(session: Option<&str>) -> Herdr {
        Herdr {
            command: vec!["herdr".into()],
            direction: Direction::Right,
            session: session.map(str::to_string),
            inherits_context: false,
        }
    }

    #[test]
    fn builds_verified_0_9_command_shapes() {
        let h = herdr(None);
        let p = plan(Some("ws"), &["api", "web"]);
        assert_eq!(h.list_cmd().unwrap().display(), "herdr workspace list");
        assert_eq!(
            h.create_cmd(&p).unwrap().display(),
            "herdr workspace create --cwd /w/feature-x --label acme/feature-x"
        );
        assert_eq!(
            h.split_cmd("w1:p1", Path::new("/w/feature-x/api"))
                .unwrap()
                .display(),
            "herdr pane split w1:p1 --direction right --cwd /w/feature-x/api --no-focus"
        );
        assert_eq!(
            h.rename_cmd("w1:p2", "api").unwrap().display(),
            "herdr pane rename w1:p2 api"
        );
        assert_eq!(
            h.focus_cmd("w1").unwrap().display(),
            "herdr workspace focus w1"
        );
    }

    #[test]
    fn session_flag_precedes_the_subcommand_outside_herdr() {
        assert_eq!(
            herdr(Some("work")).list_cmd().unwrap().display(),
            "herdr --session work workspace list"
        );
    }

    #[test]
    fn inside_a_pane_the_inherited_session_is_used() {
        let mut h = herdr(Some("work"));
        h.inherits_context = true;
        assert_eq!(h.list_cmd().unwrap().display(), "herdr workspace list");
    }

    #[test]
    fn launcher_override_keeps_fixed_arguments() {
        let h = Herdr {
            command: vec![
                "nix".into(),
                "run".into(),
                "nixpkgs#herdr".into(),
                "--".into(),
            ],
            direction: Direction::Down,
            session: None,
            inherits_context: false,
        };
        assert_eq!(
            h.list_cmd().unwrap().display(),
            "nix run 'nixpkgs#herdr' -- workspace list"
        );
        assert_eq!(
            h.split_cmd("w1:p1", Path::new("/w/a"))
                .unwrap()
                .args
                .last()
                .unwrap(),
            "--no-focus"
        );
    }

    #[test]
    fn reads_ids_out_of_result() {
        let created: Value = serde_json::from_str(
            r#"{"ok":true,"result":{"workspace":{"workspace_id":"w3","label":"acme"},
                 "tab":{"tab_id":"w3:t1"},"root_pane":{"pane_id":"w3:p1"}}}"#,
        )
        .unwrap();
        assert_eq!(
            pick(&created, &["result", "workspace", "workspace_id"]).as_deref(),
            Some("w3")
        );
        assert_eq!(
            pick(&created, &["result", "root_pane", "pane_id"]).as_deref(),
            Some("w3:p1")
        );
    }

    #[test]
    fn falls_back_to_a_document_wide_search() {
        let other: Value =
            serde_json::from_str(r#"{"data":{"panes":[{"pane_id":"w9:p4"}]}}"#).unwrap();
        assert!(pick(&other, &["result", "pane", "pane_id"]).is_none());
        assert_eq!(first_string(&other, "pane_id").as_deref(), Some("w9:p4"));
    }

    #[test]
    fn finds_an_existing_workspace_by_label() {
        let list: Value = serde_json::from_str(
            r#"{"result":{"workspaces":[
                 {"workspace_id":"w1","label":"other"},
                 {"workspace_id":"w2","label":"acme/feature-x"}]}}"#,
        )
        .unwrap();
        assert_eq!(
            find_labelled_id(&list, "acme/feature-x", "workspace_id").as_deref(),
            Some("w2")
        );
        assert_eq!(find_labelled_id(&list, "missing", "workspace_id"), None);
    }
}
