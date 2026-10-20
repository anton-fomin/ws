//! Per-repo initialisation commands, run once per new worktree.
//!
//! `setup` is a command or a list of them, and each one may span several
//! lines — a YAML block scalar is the natural way to write a short script.
//! Every one runs through `sh -e` in the worktree, so a line that fails stops
//! the setup just as a failing entry of the list does.
//!
//! Before running, these placeholders are replaced:
//!
//! | | |
//! |---|---|
//! | `{root}` | the workspace root, where `ws.yaml` is |
//! | `{branch}` | the branch, as git names it |
//! | `{branch_dir}` | the branch directory |
//! | `{worktree}` | this repository's worktree — the directory it runs in |
//! | `{repo}` | the repository's name |
//! | `{env:NAME}` | the environment variable `NAME`; unset is an error |
//! | `{env:NAME:-default}` | `NAME`, or `default` when it is unset or empty |
//!
//! Only those are touched: anything else in braces — `${HOME}`, awk's
//! `'{print $1}'`, a brace expansion — reaches the shell as written. To pass
//! a placeholder itself through, double its braces: `{{env:X}}` becomes the
//! literal `{env:X}`. The
//! values go in verbatim, unquoted, so a path with a space in it needs quotes
//! around the placeholder; the same values are also exported as `WS_ROOT`,
//! `WS_BRANCH`, `WS_BRANCH_DIR`, `WS_WORKTREE` and `WS_REPO`, which the shell
//! quotes for you as `"$WS_WORKTREE"`.

use anyhow::{bail, Context as _, Result};
use std::path::Path;

use crate::exec::{Cmd, Runner};

/// Where a setup command runs, and what its placeholders stand for.
#[derive(Debug, Clone)]
pub struct Context<'a> {
    pub root: &'a Path,
    pub branch: &'a str,
    pub branch_dir: &'a Path,
    pub worktree: &'a Path,
    pub repo: &'a str,
}

impl Context<'_> {
    /// What the placeholder `{token}` stands for, or `None` when `token` is
    /// not one — which leaves it to the shell as written.
    fn value(&self, token: &str) -> Option<Result<String>> {
        let path = |p: &Path| p.to_string_lossy().into_owned();
        let value = match token {
            "root" => path(self.root),
            "branch" => self.branch.to_string(),
            "branch_dir" => path(self.branch_dir),
            "worktree" => path(self.worktree),
            "repo" => self.repo.to_string(),
            _ => return env_value(token),
        };
        Some(Ok(value))
    }

    fn exported(&self) -> [(&'static str, String); 5] {
        let path = |p: &Path| p.to_string_lossy().into_owned();
        [
            ("WS_ROOT", path(self.root)),
            ("WS_BRANCH", self.branch.to_string()),
            ("WS_BRANCH_DIR", path(self.branch_dir)),
            ("WS_WORKTREE", path(self.worktree)),
            ("WS_REPO", self.repo.to_string()),
        ]
    }
}

/// `env:NAME` or `env:NAME:-default`, the default used — as the shell's
/// `${NAME:-default}` does — when the variable is unset or empty.
fn env_value(token: &str) -> Option<Result<String>> {
    let spec = token.strip_prefix("env:")?;
    let (name, default) = match spec.split_once(":-") {
        Some((name, default)) => (name, Some(default)),
        None => (spec, None),
    };
    let valid = !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid {
        return None;
    }
    Some(match (std::env::var(name), default) {
        (Ok(value), Some(default)) if value.is_empty() => Ok(default.to_string()),
        (Ok(value), _) => Ok(value),
        (Err(_), Some(default)) => Ok(default.to_string()),
        (Err(_), None) => Err(anyhow::anyhow!(
            "environment variable {name} is not set — give it a default as {{env:{name}:-…}}"
        )),
    })
}

/// Replace the known placeholders of `command`, leaving every other brace
/// alone.
///
/// A placeholder with its braces doubled is the escape: `{{env:X}}` comes out
/// as the literal `{env:X}`. Only a doubled pair around something that is a
/// placeholder counts, so the `}}` closing nested awk blocks is never taken
/// for one.
pub fn interpolate(command: &str, context: &Context) -> Result<String> {
    let mut out = String::with_capacity(command.len());
    let mut rest = command;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];

        if let Some(inner) = after.strip_prefix('{') {
            let escaped = inner
                .find("}}")
                .map(|close| &inner[..close])
                .filter(|token| !token.contains('}') && context.value(token).is_some());
            if let Some(token) = escaped {
                out.push('{');
                out.push_str(token);
                out.push('}');
                rest = &inner[token.len() + 2..];
                continue;
            }
        }

        let replaced = after
            .find('}')
            .and_then(|close| Some((context.value(&after[..close])?, close)));
        match replaced {
            Some((value, close)) => {
                out.push_str(&value?);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Ok(out)
}

/// Run `commands` in the worktree through `sh -e`, streaming their output.
pub fn run(runner: &Runner, context: &Context, commands: &[String]) -> Result<()> {
    let repo = context.repo;
    for command in commands {
        if command.trim().is_empty() {
            continue;
        }
        let command = interpolate(command, context)
            .with_context(|| format!("setup command for {repo}: {}", command.trim()))?;
        for line in command.trim().lines() {
            eprintln!("  [{repo}] {line}");
        }
        let mut cmd = Cmd::new("sh")
            .args(["-e", "-c", &command])
            .cwd(context.worktree);
        for (key, value) in context.exported() {
            cmd = cmd.env(key, value);
        }
        if let Err(e) = runner.run(&cmd) {
            bail!("setup command for {repo} failed: {}\n{e:#}", command.trim());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn with<T>(f: impl FnOnce(&Context) -> T) -> T {
        let root = PathBuf::from("/w");
        let dir = PathBuf::from("/w/feature-x");
        let worktree = PathBuf::from("/w/feature-x/api");
        f(&Context {
            root: &root,
            branch: "feature/x",
            branch_dir: &dir,
            worktree: &worktree,
            repo: "api",
        })
    }

    #[test]
    fn replaces_every_placeholder() {
        let out = with(|c| {
            interpolate(
                "cp {root}/shared/.env {worktree}/.env && echo {repo} {branch} {branch_dir}",
                c,
            )
        })
        .unwrap();
        assert_eq!(
            out,
            "cp /w/shared/.env /w/feature-x/api/.env && echo api feature/x /w/feature-x"
        );
    }

    #[test]
    fn reads_the_environment() {
        let home = std::env::var("HOME").unwrap();
        let out = with(|c| interpolate("ls {env:HOME}", c)).unwrap();
        assert_eq!(out, format!("ls {home}"));
        assert!(with(|c| interpolate("{env:WS_SURELY_NOT_SET_ANYWHERE}", c)).is_err());
    }

    #[test]
    fn an_env_default_covers_unset_and_empty() {
        let home = std::env::var("HOME").unwrap();
        let unset = "WS_SURELY_NOT_SET_ANYWHERE";
        let run = |text: &str| with(|c| interpolate(text, c)).unwrap();
        assert_eq!(run(&format!("{{env:{unset}:-fallback}}")), "fallback");
        assert_eq!(run(&format!("{{env:{unset}:-}}")), "");
        assert_eq!(run(&format!("{{env:{unset}:-a b:-c}}")), "a b:-c");
        assert_eq!(run("{env:HOME:-fallback}"), home);
        // `PATH` is never empty, so this checks only that a set one wins; the
        // empty case is exercised by the e2e test, which owns its environment.
        assert_ne!(run("{env:PATH:-x}"), "x");
    }

    #[test]
    fn doubled_braces_pass_a_placeholder_through() {
        let run = |text: &str| with(|c| interpolate(text, c)).unwrap();
        assert_eq!(run("echo {{env:SOMETHING}}"), "echo {env:SOMETHING}");
        assert_eq!(run("echo {{env:SOMETHING:-x}}"), "echo {env:SOMETHING:-x}");
        assert_eq!(run("{{repo}} is {repo}"), "{repo} is api");
        // Not a placeholder inside, so not an escape: left exactly as written.
        assert_eq!(run("awk '{if(x){print}}'"), "awk '{if(x){print}}'");
        assert_eq!(run("echo {{nope}}"), "echo {{nope}}");
        // Only the opening is doubled: the inner one is still a placeholder.
        assert_eq!(run("{{repo}"), "{api");
    }

    #[test]
    fn invalid_env_names_are_not_placeholders() {
        let run = |text: &str| with(|c| interpolate(text, c)).unwrap();
        for text in ["{env:}", "{env:1X}", "{env:A B}", "{env::-x}"] {
            assert_eq!(run(text), text, "{text}");
        }
    }

    #[test]
    fn leaves_the_shells_own_braces_alone() {
        for text in [
            "echo ${HOME}",
            "awk '{print $1}' file",
            "cp x.{a,b} y",
            "echo {unknown}",
            "echo {",
            "echo }{",
            "fn() { true; }",
        ] {
            assert_eq!(with(|c| interpolate(text, c)).unwrap(), text, "{text}");
        }
    }
}
