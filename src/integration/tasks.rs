//! Tasks: what a repository declares can be run in it.
//!
//! Three files declare them, each read by the tool that runs it:
//!
//! * a `justfile`, listed with `just --dump`,
//! * a `Taskfile.yml`, listed with `task --list-all --json`,
//! * the `scripts` of `package.json`, read directly and run with whichever of
//!   yarn, pnpm or npm the repository uses.
//!
//! A repository may have the same name in more than one of them; the
//! configured `tasks.order` says which one runs it. Asking the tools rather
//! than parsing their files keeps imports, modules and includes their
//! business.

use std::path::Path;

use crate::config::{Integrations, Manager, TaskSource, TaskToolConfig, TasksConfig};
use crate::exec::{launcher_from_env, program_available, Cmd, Runner};

use super::pm;

/// Files `just` looks for, in the order it does.
const JUSTFILES: &[&str] = &["justfile", "Justfile", ".justfile", "JUSTFILE"];

/// Files go-task looks for.
const TASKFILES: &[&str] = &[
    "Taskfile.yml",
    "taskfile.yml",
    "Taskfile.yaml",
    "taskfile.yaml",
    "Taskfile.dist.yml",
    "taskfile.dist.yml",
    "Taskfile.dist.yaml",
    "taskfile.dist.yaml",
];

/// One task a repository declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub name: String,
    pub source: TaskSource,
    /// Its doc comment, `desc`, or the script itself.
    pub description: Option<String>,
}

/// A source that is enabled and, for the two tools, present.
#[derive(Debug, Clone)]
struct Enabled {
    source: TaskSource,
    /// The launcher for `just` and `task`; the package managers bring their own.
    command: Vec<String>,
}

/// The task sources `ws run` reads, resolved once per run.
#[derive(Debug, Clone)]
pub struct Tasks {
    enabled: Vec<Enabled>,
    /// Launchers for the package managers, which are configured — and
    /// overridden — where the installs are.
    managers: Vec<(Manager, Vec<String>)>,
}

impl Tasks {
    pub fn new(config: &TasksConfig, integrations: &Integrations) -> Tasks {
        let mut enabled: Vec<Enabled> = Vec::new();
        for source in &config.order {
            if enabled.iter().any(|e| e.source == *source) {
                continue;
            }
            let resolved = match source {
                TaskSource::Just => resolve_tool(&config.just, "just", "WS_JUST_CMD"),
                TaskSource::Task => resolve_tool(&config.task, "task", "WS_TASK_CMD"),
                // Whether a repository has a package.json is asked per
                // repository, so `auto` is on here.
                TaskSource::Package => config.package.enabled.resolve(true).then(Vec::new),
            };
            if let Some(command) = resolved {
                enabled.push(Enabled {
                    source: *source,
                    command,
                });
            }
        }
        let managers = integrations
            .package_managers()
            .into_iter()
            .filter(|(manager, _)| manager.is_node())
            .map(|(manager, settings)| {
                let command = launcher_from_env(manager.command_env())
                    .unwrap_or_else(|| settings.command(manager));
                (manager, command)
            })
            .collect();
        Tasks { enabled, managers }
    }

    /// Names of the sources that will be read, as `ws status` lists them.
    pub fn names(&self) -> Vec<&'static str> {
        self.enabled.iter().map(|e| e.source.name()).collect()
    }

    /// Every task `worktree` declares, each name once: where two sources
    /// have the same one, the source earlier in `tasks.order` keeps it.
    pub fn discover(&self, runner: &Runner, worktree: &Path) -> Vec<Task> {
        let mut found: Vec<Task> = Vec::new();
        for enabled in &self.enabled {
            let tasks = match enabled.source {
                TaskSource::Just => just_tasks(runner, &enabled.command, worktree),
                TaskSource::Task => go_tasks(runner, &enabled.command, worktree),
                TaskSource::Package => package_scripts(worktree),
            };
            for task in tasks {
                if !found.iter().any(|t| t.name == task.name) {
                    found.push(task);
                }
            }
        }
        found
    }

    /// The command that runs `task` in `worktree`, with `args` passed on to it.
    pub fn command(&self, task: &Task, worktree: &Path, args: &[String]) -> Cmd {
        let manager = self.package_manager(worktree);
        let launcher = match task.source {
            TaskSource::Package => self.launcher(manager),
            source => self
                .enabled
                .iter()
                .find(|e| e.source == source)
                .map(|e| e.command.clone())
                .unwrap_or_else(|| vec![source.name().to_string()]),
        };
        let base = Cmd::launcher(&launcher).unwrap_or_else(|_| Cmd::new(task.source.name()));
        let cmd = match task.source {
            TaskSource::Just => base.arg(&task.name).args(args.iter().cloned()),
            // go-task hands what follows `--` to the task as CLI_ARGS.
            TaskSource::Task if args.is_empty() => base.arg(&task.name),
            TaskSource::Task => base.arg(&task.name).arg("--").args(args.iter().cloned()),
            // npm wants `--` before the script's own arguments; yarn and pnpm
            // pass everything after the name through, `--` included.
            TaskSource::Package => {
                let cmd = base.arg("run").arg(&task.name);
                if manager == Manager::Npm && !args.is_empty() {
                    cmd.arg("--").args(args.iter().cloned())
                } else {
                    cmd.args(args.iter().cloned())
                }
            }
        };
        cmd.cwd(worktree)
    }

    /// The package manager a repository runs its scripts with: the one its
    /// `packageManager` field names, else the one its lockfile belongs to,
    /// else npm. Unlike the installs, this does not ask whether the manager
    /// is enabled there — a script is the repository's, whoever installs.
    fn package_manager(&self, worktree: &Path) -> Manager {
        pm::declared_manager(worktree)
            .or_else(|| {
                Manager::ALL
                    .into_iter()
                    .find(|m| m.is_node() && pm::marked_by(worktree, *m))
            })
            .unwrap_or(Manager::Npm)
    }

    fn launcher(&self, manager: Manager) -> Vec<String> {
        self.managers
            .iter()
            .find(|(m, _)| *m == manager)
            .map(|(_, command)| command.clone())
            .unwrap_or_else(|| vec![manager.name().to_string()])
    }
}

fn resolve_tool(config: &TaskToolConfig, name: &str, env: &str) -> Option<Vec<String>> {
    let command = launcher_from_env(env)
        .or_else(|| config.command.clone())
        .unwrap_or_else(|| vec![name.to_string()]);
    let available = command.first().is_some_and(|p| program_available(p));
    config.enabled.resolve(available).then_some(command)
}

/// The tool has to find its file in the worktree itself — both of them would
/// otherwise walk up and find the branch directory's, or the workspace's.
fn has_any(worktree: &Path, names: &[&str]) -> bool {
    names.iter().any(|name| worktree.join(name).is_file())
}

/// The public recipes of a justfile, those of its modules as `module::recipe`.
fn just_tasks(runner: &Runner, command: &[String], worktree: &Path) -> Vec<Task> {
    if !has_any(worktree, JUSTFILES) {
        return Vec::new();
    }
    let Ok(base) = Cmd::launcher(command) else {
        return Vec::new();
    };
    let cmd = base.args(["--dump", "--dump-format", "json"]).cwd(worktree);
    let Ok(Some(out)) = runner.try_capture(&cmd) else {
        eprintln!("warning: cannot list the recipes in {}", worktree.display());
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&out) else {
        return Vec::new();
    };
    let mut tasks = Vec::new();
    collect_recipes(&json, "", &mut tasks);
    tasks
}

fn collect_recipes(module: &serde_json::Value, prefix: &str, tasks: &mut Vec<Task>) {
    if let Some(recipes) = module.get("recipes").and_then(|r| r.as_object()) {
        for (name, recipe) in recipes {
            if recipe.get("private").and_then(|p| p.as_bool()) == Some(true) {
                continue;
            }
            tasks.push(Task {
                name: format!("{prefix}{name}"),
                source: TaskSource::Just,
                description: text(recipe.get("doc")),
            });
        }
    }
    if let Some(modules) = module.get("modules").and_then(|m| m.as_object()) {
        for (name, inner) in modules {
            collect_recipes(inner, &format!("{prefix}{name}::"), tasks);
        }
    }
}

/// The tasks of a Taskfile, internal ones left out.
fn go_tasks(runner: &Runner, command: &[String], worktree: &Path) -> Vec<Task> {
    if !has_any(worktree, TASKFILES) {
        return Vec::new();
    }
    let Ok(base) = Cmd::launcher(command) else {
        return Vec::new();
    };
    let cmd = base.args(["--list-all", "--json"]).cwd(worktree);
    let Ok(Some(out)) = runner.try_capture(&cmd) else {
        eprintln!("warning: cannot list the tasks in {}", worktree.display());
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&out) else {
        return Vec::new();
    };
    json.get("tasks")
        .and_then(|t| t.as_array())
        .into_iter()
        .flatten()
        .filter_map(|task| {
            Some(Task {
                name: task.get("name")?.as_str()?.to_string(),
                source: TaskSource::Task,
                description: text(task.get("desc")),
            })
        })
        .collect()
}

/// The `scripts` of `package.json`, described by what they run.
fn package_scripts(worktree: &Path) -> Vec<Task> {
    let Ok(text) = std::fs::read_to_string(worktree.join("package.json")) else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        eprintln!("warning: cannot parse {}/package.json", worktree.display());
        return Vec::new();
    };
    json.get("scripts")
        .and_then(|s| s.as_object())
        .into_iter()
        .flatten()
        .map(|(name, script)| Task {
            name: name.clone(),
            source: TaskSource::Package,
            description: script.as_str().map(str::to_string),
        })
        .collect()
}

fn text(value: Option<&serde_json::Value>) -> Option<String> {
    let text = value?.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse;

    fn tasks(yaml: &str) -> Tasks {
        let cfg = parse(yaml).unwrap();
        Tasks::new(&cfg.tasks, &cfg.integrations)
    }

    fn task(name: &str, source: TaskSource) -> Task {
        Task {
            name: name.into(),
            source,
            description: None,
        }
    }

    #[test]
    fn package_scripts_are_read_with_their_bodies() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"dev": "vite", "test": "vitest run"}}"#,
        )
        .unwrap();
        let found = package_scripts(dir.path());
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "dev");
        assert_eq!(found[0].description.as_deref(), Some("vite"));
    }

    #[test]
    fn scripts_run_with_the_repositorys_package_manager() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path();
        std::fs::write(wt.join("package.json"), "{}").unwrap();
        let t = tasks("");
        let args = ["--watch".to_string()];

        // Nothing to go by: npm, which needs `--` before the arguments.
        let cmd = t.command(&task("test", TaskSource::Package), wt, &args);
        assert_eq!(cmd.display(), "npm run test -- --watch");

        std::fs::write(wt.join("pnpm-lock.yaml"), "").unwrap();
        let cmd = t.command(&task("test", TaskSource::Package), wt, &args);
        assert_eq!(cmd.display(), "pnpm run test --watch");

        // The declared manager beats the lockfile, and its launcher is the
        // one configured for the installs.
        std::fs::write(
            wt.join("package.json"),
            r#"{"packageManager": "yarn@4.1.0"}"#,
        )
        .unwrap();
        let t = tasks("integrations:\n  yarn:\n    command: [corepack, yarn]\n");
        let cmd = t.command(&task("test", TaskSource::Package), wt, &[]);
        assert_eq!(cmd.display(), "corepack yarn run test");
        assert_eq!(cmd.cwd.as_deref(), Some(wt));
    }

    #[test]
    fn just_and_task_take_their_arguments_their_own_way() {
        let t = tasks(
            "tasks:\n  just: { enabled: true }\n  task: { enabled: true, command: [go-task] }\n",
        );
        let wt = Path::new("/w");
        let args = ["a b".to_string()];
        assert_eq!(
            t.command(&task("test", TaskSource::Just), wt, &args)
                .display(),
            "just test 'a b'"
        );
        assert_eq!(
            t.command(&task("db:migrate", TaskSource::Task), wt, &args)
                .display(),
            "go-task db:migrate -- 'a b'"
        );
        assert_eq!(
            t.command(&task("build", TaskSource::Task), wt, &[])
                .display(),
            "go-task build"
        );
    }

    #[test]
    fn the_order_decides_and_disabled_sources_are_not_read() {
        let t = tasks("tasks:\n  order: [package, just]\n  just: { enabled: false }\n");
        assert_eq!(t.names(), ["package"]);
        let t = tasks("tasks:\n  order: [package, package]\n  package: { enabled: true }\n");
        assert_eq!(t.names(), ["package"]);
    }

    #[test]
    fn recipes_of_modules_are_namespaced_and_private_ones_hidden() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"recipes": {"build": {"doc": "Build it", "private": false},
                             "_helper": {"doc": null, "private": true}},
                "modules": {"db": {"recipes": {"migrate": {"doc": null, "private": false}},
                                   "modules": {}}}}"#,
        )
        .unwrap();
        let mut found = Vec::new();
        collect_recipes(&json, "", &mut found);
        let names: Vec<&str> = found.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["build", "db::migrate"]);
        assert_eq!(found[0].description.as_deref(), Some("Build it"));
    }
}
