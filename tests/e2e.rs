//! End-to-end tests against local fixture repositories — no network involved.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(["-c", "user.email=ws@example.com", "-c", "user.name=ws"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git is available");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A normal repository with a commit on `main` and a `feature/JIRA-123` branch.
fn origin_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "--initial-branch=main", "."]);
    fs::write(path.join("README.md"), format!("# {name}\n")).unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-m", "init"]);
    git(&path, &["branch", "feature/JIRA-123"]);
    path
}

/// A stub that records every invocation, standing in for zoxide or fzf.
fn stub(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

fn ws(root: &Path) -> Command {
    let mut cmd = Command::cargo_bin("ws").unwrap();
    cmd.current_dir(root)
        .env("WS_ZOXIDE_CMD", "true")
        // An empty WS_CONFIG turns the global layer off, so whatever the
        // developer running these tests keeps in ~/.config/ws cannot reach them.
        .env("WS_CONFIG", "")
        .args(["--mux", "none"]);
    cmd
}

/// Like [`ws`], but with a global configuration file in play.
fn ws_with_global(root: &Path, global: &Path) -> Command {
    let mut cmd = ws(root);
    cmd.env("WS_CONFIG", global);
    cmd
}

fn write_config(root: &Path, body: &str) {
    fs::write(root.join("ws.yaml"), body).unwrap();
}

#[test]
fn multi_repo_switch_creates_grouped_worktrees_and_runs_setup() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo(&origins, "api");
    let web = origin_repo(&origins, "web");

    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!(
            r#"
name: acme
repos:
  - name: api
    url: "{}"
    setup: touch setup-done

  - name: web
    url: "{}"
    setup: [touch setup-done, touch second]
"#,
            api.display(),
            web.display()
        ),
    );

    ws(&root).arg("fetch").assert().success();
    assert!(root.join(".bare/api.git").is_dir());
    assert!(root.join(".bare/web.git").is_dir());

    ws(&root)
        .args(["switch", "feature/JIRA-123"])
        .assert()
        .success();

    let dir = root.join("feature-JIRA-123");
    for repo in ["api", "web"] {
        let wt = dir.join(repo);
        assert!(wt.join(".git").exists(), "{repo} worktree missing");
        assert!(wt.join("setup-done").exists(), "{repo} setup did not run");
        let head = String::from_utf8(
            std::process::Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(&wt)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        assert_eq!(head.trim(), "feature/JIRA-123");
    }
    assert!(dir.join("web/second").exists());

    // Setup is remembered, so a second switch does not re-run it.
    fs::remove_file(dir.join("api/setup-done")).unwrap();
    ws(&root)
        .args(["switch", "feature/JIRA-123"])
        .assert()
        .success();
    assert!(!dir.join("api/setup-done").exists());

    // ...unless asked.
    ws(&root)
        .args(["switch", "feature/JIRA-123", "--rerun-setup"])
        .assert()
        .success();
    assert!(dir.join("api/setup-done").exists());

    let listed = ws(&root).arg("list").output().unwrap();
    let listed = String::from_utf8(listed.stdout).unwrap();
    assert!(listed.contains("feature-JIRA-123"), "{listed}");
    assert!(listed.contains("feature/JIRA-123"), "{listed}");

    let path = ws(&root)
        .args(["path", "feature/JIRA-123", "api"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(path.stdout).unwrap().trim(),
        dir.join("api").to_string_lossy()
    );

    // The setup commands left untracked files behind, so rm needs --force here.
    ws(&root)
        .args(["rm", "feature/JIRA-123", "--force"])
        .assert()
        .success();
    assert!(!dir.exists(), "branch directory should be gone");
}

#[test]
fn single_repo_uses_the_branch_directory_as_the_worktree() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo(&origins, "api");

    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!("repos:\n  - name: api\n    url: \"{}\"\n", api.display()),
    );

    ws(&root).args(["switch", "main"]).assert().success();
    assert!(root.join("main/.git").exists());
    assert!(!root.join("main/api").exists());
}

#[test]
fn nested_branch_dirs_keep_the_hierarchy() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo(&origins, "api");

    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!(
            "branch_dirs: nested\nrepo_layout: grouped\n\
             repos:\n  - name: api\n    url: \"{}\"\n",
            api.display()
        ),
    );

    ws(&root)
        .args(["switch", "feature/JIRA-123"])
        .assert()
        .success();
    assert!(root.join("feature/JIRA-123/api/.git").exists());
}

#[test]
fn a_new_branch_forks_from_the_base_and_tracks_nothing() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo(&origins, "api");

    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!("repos:\n  - name: api\n    url: \"{}\"\n", api.display()),
    );

    ws(&root).args(["switch", "brand-new"]).assert().success();
    let head = String::from_utf8(
        std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(root.join("brand-new"))
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert_eq!(head.trim(), "brand-new");
}

#[test]
fn dirty_worktrees_are_protected_from_rm() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo(&origins, "api");

    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!("repos:\n  - name: api\n    url: \"{}\"\n", api.display()),
    );

    ws(&root).args(["switch", "main"]).assert().success();
    fs::write(root.join("main/uncommitted.txt"), "wip").unwrap();

    let out = ws(&root).args(["rm", "main"]).output().unwrap();
    assert!(!out.status.success(), "rm should refuse a dirty worktree");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(stderr.contains("uncommitted changes"), "{stderr}");
    assert!(root.join("main/.git").exists());

    ws(&root).args(["rm", "main", "--force"]).assert().success();
    assert!(!root.join("main").exists());
}

#[test]
fn zoxide_registers_and_forgets_the_paths() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo(&origins, "api");
    let web = origin_repo(&origins, "web");

    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!(
            "repos:\n  - name: api\n    url: \"{}\"\n  - name: web\n    url: \"{}\"\n",
            api.display(),
            web.display()
        ),
    );

    let log = tmp.path().join("zoxide.log");
    let stub_path = stub(
        tmp.path(),
        "zoxide-stub",
        &format!("printf '%s\\n' \"$*\" >> {}", log.display()),
    );

    let mut cmd = Command::cargo_bin("ws").unwrap();
    cmd.current_dir(&root)
        .env("WS_ZOXIDE_CMD", stub_path.to_string_lossy().to_string())
        .env("WS_CONFIG", "")
        .args(["--mux", "none", "switch", "feature/JIRA-123"])
        .assert()
        .success();

    let recorded = fs::read_to_string(&log).unwrap();
    let dir = root.join("feature-JIRA-123");
    assert!(recorded.starts_with("add "), "{recorded}");
    assert!(
        recorded.contains(&dir.to_string_lossy().to_string()),
        "{recorded}"
    );
    assert!(recorded.contains(&dir.join("api").to_string_lossy().to_string()));
    assert!(recorded.contains(&dir.join("web").to_string_lossy().to_string()));

    let mut cmd = Command::cargo_bin("ws").unwrap();
    cmd.current_dir(&root)
        .env("WS_ZOXIDE_CMD", stub_path.to_string_lossy().to_string())
        .env("WS_CONFIG", "")
        .args(["--mux", "none", "rm", "feature/JIRA-123"])
        .assert()
        .success();
    let recorded = fs::read_to_string(&log).unwrap();
    assert!(recorded.contains("remove "), "{recorded}");
}

#[test]
fn picker_selection_switches_to_the_chosen_branch() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo(&origins, "api");

    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();

    // The stub filter picks the feature branch out of what ws feeds it.
    let picker = stub(tmp.path(), "pick-stub", "grep 'feature/JIRA-123' | head -1");
    write_config(
        &root,
        &format!(
            "repos:\n  - name: api\n    url: \"{}\"\n\n\
             ui:\n  picker: inline\n  picker_cmd: [\"{}\"]\n",
            api.display(),
            picker.display()
        ),
    );

    ws(&root).arg("fetch").assert().success();
    ws(&root).args(["pick", "--inline"]).assert().success();
    assert!(root.join("feature-JIRA-123/.git").exists());
}

#[test]
fn preview_describes_a_branch() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo(&origins, "api");

    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!("repos:\n  - name: api\n    url: \"{}\"\n", api.display()),
    );
    ws(&root).args(["switch", "main"]).assert().success();

    let out = ws(&root)
        .args(["__preview", "branch:main"])
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("branch  main"), "{text}");
    assert!(text.contains("api"), "{text}");
}

#[test]
fn worktrees_go_under_the_configured_base_directory() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo(&origins, "api");
    let web = origin_repo(&origins, "web");

    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!(
            "worktree_dir: trees\n\
             repos:\n  - name: api\n    url: \"{}\"\n\
             \x20 - name: web\n    url: \"{}\"\n    path: services/web\n",
            api.display(),
            web.display()
        ),
    );

    ws(&root)
        .args(["switch", "feature/JIRA-123"])
        .assert()
        .success();

    let dir = root.join("trees/feature-JIRA-123");
    assert!(dir.join("api/.git").exists(), "api worktree misplaced");
    assert!(
        dir.join("services/web/.git").exists(),
        "nested repo path ignored"
    );
    assert!(!root.join("feature-JIRA-123").exists());

    // Listing and path reporting follow the same base.
    let listed = String::from_utf8(ws(&root).arg("list").output().unwrap().stdout).unwrap();
    assert!(listed.contains("feature-JIRA-123"), "{listed}");
    assert!(!listed.contains("trees/feature"), "{listed}");
    let path = ws(&root)
        .args(["path", "feature/JIRA-123", "web"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(path.stdout).unwrap().trim(),
        dir.join("services/web").to_string_lossy()
    );

    // rm takes the directories a nested repo path created with it.
    ws(&root)
        .args(["rm", "feature/JIRA-123"])
        .assert()
        .success();
    assert!(!dir.exists(), "{} should be gone", dir.display());
    assert!(root.join("trees").exists(), "the base itself stays");
}

#[test]
fn a_worktree_outside_the_root_still_finds_its_workspace() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo(&origins, "api");

    let root = tmp.path().join("work");
    let elsewhere = tmp.path().join("elsewhere");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!(
            "worktree_dir: \"{}\"\nrepo_layout: grouped\n\
             repos:\n  - name: api\n    url: \"{}\"\n",
            elsewhere.display(),
            api.display()
        ),
    );

    ws(&root).args(["switch", "main"]).assert().success();
    let worktree = elsewhere.join("main/api");
    assert!(worktree.join(".git").exists());

    // No ws.yaml in any parent of the worktree: discovery has to follow the
    // .git file back to the bare repo and up to the root from there.
    let out = ws(&worktree).arg("status").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains(&root.to_string_lossy().to_string()), "{text}");
    assert!(text.contains("branch     main"), "{text}");
}

#[test]
fn the_global_configuration_supplies_settings_and_the_workspace_overrides_them() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo(&origins, "api");
    let web = origin_repo(&origins, "web");

    let global = tmp.path().join("config/ws/config.yaml");
    fs::create_dir_all(global.parent().unwrap()).unwrap();
    fs::write(
        &global,
        "branch_dirs: nested\nworktree_dir: trees\n\
         mux:\n  root_window: shared\n  session: \"proj-{workspace}-{branch}\"\n",
    )
    .unwrap();

    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    // Almost empty: identity, repositories, and the one key it disagrees on.
    write_config(
        &root,
        &format!(
            "name: acme\nbranch_dirs: flat\n\
             repos:\n  - name: api\n    url: \"{}\"\n\
             \x20 - name: web\n    url: \"{}\"\n",
            api.display(),
            web.display()
        ),
    );

    ws_with_global(&root, &global)
        .args(["switch", "feature/JIRA-123"])
        .assert()
        .success();

    // worktree_dir came from the global file, branch_dirs from the workspace.
    assert!(root.join("trees/feature-JIRA-123/api/.git").exists());
    assert!(!root.join("trees/feature/JIRA-123").exists());

    // A key the workspace never mentions still comes from the global `mux`.
    let status = ws_with_global(&root, &global)
        .current_dir(root.join("trees/feature-JIRA-123/api"))
        .arg("status")
        .output()
        .unwrap();
    let text = String::from_utf8(status.stdout).unwrap();
    assert!(
        text.contains("session    proj-acme-feature-JIRA-123"),
        "{text}"
    );
    // Both files are reported as sources.
    assert!(
        text.contains(&global.to_string_lossy().to_string()),
        "{text}"
    );
    assert!(text.contains("ws.yaml"), "{text}");
}

#[test]
fn repositories_are_refused_in_the_global_configuration() {
    let tmp = TempDir::new().unwrap();
    let global = tmp.path().join("config.yaml");
    fs::write(&global, "repos:\n  - name: api\n    url: x/api.git\n").unwrap();

    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(&root, "name: acme\n");

    let out = ws_with_global(&root, &global).arg("list").output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(stderr.contains("repositories belong in"), "{stderr}");
}

#[test]
fn a_missing_global_configuration_is_not_an_error() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo(&origins, "api");

    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!("repos:\n  - name: api\n    url: \"{}\"\n", api.display()),
    );

    ws_with_global(&root, &tmp.path().join("nowhere/config.yaml"))
        .args(["switch", "main"])
        .assert()
        .success();
    assert!(root.join("main/.git").exists());
}

#[test]
fn init_global_writes_a_settings_file_without_repositories() {
    let tmp = TempDir::new().unwrap();
    let global = tmp.path().join("config/ws/config.yaml");
    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();

    ws_with_global(&root, &global)
        .args(["init", "--global"])
        .assert()
        .success();
    let text = fs::read_to_string(&global).unwrap();
    assert!(text.contains("mux:"), "{text}");
    assert!(!text.contains("\nrepos:"), "{text}");

    // It refuses to clobber, and the workspace file stays minimal.
    ws_with_global(&root, &global)
        .args(["init", "--global"])
        .assert()
        .failure();

    ws_with_global(&root, &global)
        .args([
            "init",
            "--name",
            "demo",
            "--repo",
            "git@example.com:org/api.git",
        ])
        .assert()
        .success();
    let local = fs::read_to_string(root.join("ws.yaml")).unwrap();
    assert!(local.contains("name: \"demo\""), "{local}");
    // Identity and repositories, and only comments beyond that: the settings
    // live in the global file, with the rest shown as commented examples.
    let live_key = local
        .lines()
        .filter(|line| !line.starts_with(' ') && !line.trim_start().starts_with('#'))
        .find_map(|line| line.split_once(':').map(|(key, _)| key))
        .filter(|key| !["name", "repos"].contains(key));
    assert_eq!(
        live_key, None,
        "the workspace file should stay small:\n{local}"
    );
}

#[test]
fn init_writes_a_parseable_config() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("fresh");
    fs::create_dir_all(&root).unwrap();

    Command::cargo_bin("ws")
        .unwrap()
        .current_dir(&root)
        .args([
            "init",
            "--name",
            "demo",
            "--repo",
            "git@example.com:org/api.git",
        ])
        .assert()
        .success();
    let text = fs::read_to_string(root.join("ws.yaml")).unwrap();
    assert!(text.contains("name: \"demo\""));

    // A second init refuses to clobber.
    Command::cargo_bin("ws")
        .unwrap()
        .current_dir(&root)
        .env("WS_CONFIG", "")
        .arg("init")
        .assert()
        .failure();

    Command::cargo_bin("ws")
        .unwrap()
        .current_dir(&root)
        .env("WS_CONFIG", "")
        .args(["status"])
        .assert()
        .success();
}

#[test]
fn init_suggests_the_agent_skill_until_an_agent_has_it() {
    let skill =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/ws/SKILL.md"))
            .unwrap();
    // Claude Code, OpenCode, and the shared Agent Skills directory.
    for installed in [".claude", ".config/opencode", ".agents"] {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("work");
        let home = tmp.path().join("home");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&home).unwrap();

        let init = |args: &[&str]| {
            let out = ws(&root)
                .env("HOME", &home)
                .env_remove("CLAUDE_CONFIG_DIR")
                .env_remove("XDG_CONFIG_HOME")
                .args(args)
                .assert()
                .success();
            String::from_utf8(out.get_output().stderr.clone()).unwrap()
        };
        let stderr = init(&["init"]);
        assert!(
            stderr.contains("github.com/anton-fomin/ws#coding-agents"),
            "{stderr}"
        );

        let dir = home.join(installed).join("skills/ws");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), &skill).unwrap();
        let stderr = init(&["init", "--force"]);
        assert!(!stderr.contains("coding-agents"), "{installed}: {stderr}");
    }
}

#[test]
fn completion_scripts_are_emitted_for_every_supported_shell() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();

    // The zsh script has to carry the `#compdef` tag, or compinit ignores it.
    let zsh = ws(&root).args(["completions", "zsh"]).assert().success();
    let zsh = String::from_utf8(zsh.get_output().stdout.clone()).unwrap();
    assert!(zsh.starts_with("#compdef ws"), "zsh script: {zsh:.40}");
    assert!(zsh.contains("shell-init"));
    // The internal fzf hook is not something to offer at the prompt.
    assert!(!zsh.contains("__preview"));

    // Bash binds the completion function to the command name, which is also
    // the name of the `ws shell-init` wrapper function.
    let bash = ws(&root).args(["completions", "bash"]).assert().success();
    let bash = String::from_utf8(bash.get_output().stdout.clone()).unwrap();
    assert!(bash.contains("complete -F _ws"), "bash script: {bash:.40}");
    assert!(!bash.contains("__preview"));
    // bash alone needs teaching to keep the options behind a dash, and the
    // wrapper has to be the one finally registered.
    let last = bash
        .rfind("complete -F")
        .expect("the script registers itself");
    assert!(bash[last..].contains("_ws_prefer_commands"));
    assert!(!zsh.contains("_ws_prefer_commands"));
    // bash has nowhere to put a description, so the script carries its own
    // table — including the one help string that has to survive quoting.
    assert!(bash.contains("_ws_describe"));
    assert!(bash.contains(r"one repo'\''s worktree"));
    assert!(bash.contains("'switch' | 'sw')"));
    // Branch arguments are asked of `ws` itself, in every shell: the worktrees
    // for `rm` and `path`, every branch for `switch`, branches and tags for
    // its `--from`.
    assert!(bash.contains(r#"command ws __complete "$what" "$word""#));
    for kind in ["worktrees", "branches", "refs"] {
        assert!(bash.contains(&format!("candidates={kind}")), "bash: {kind}");
        assert!(
            zsh.contains(&format!("_ws_candidates {kind} ")),
            "zsh: {kind}"
        );
    }
    // Four rewritten actions for the worktrees — `rm`, `path` and the two
    // spellings of `run --branch` — one each for the branches of `switch`,
    // the refs of `--from` and the tasks of `run`.
    assert_eq!(zsh.matches(":_ws_worktrees'").count(), 4, "zsh: {zsh}");
    assert_eq!(zsh.matches(":_ws_branches'").count(), 1, "zsh: {zsh}");
    assert_eq!(zsh.matches(":REF:_ws_refs'").count(), 1, "zsh: {zsh}");
    assert_eq!(zsh.matches(":_ws_tasks'").count(), 1, "zsh: {zsh}");
    // Autoloaded from fpath, the script calls `_ws` at its dispatch, so every
    // helper has to be defined above it — an `eval` of it would not notice.
    let dispatch = zsh
        .find(r#"if [ "$funcstack[1]" = "_ws" ]; then"#)
        .expect("clap's zsh script dispatches at its end");
    for helper in [
        "_ws_worktrees() {",
        "_ws_branches() {",
        "_ws_refs() {",
        "_ws_tasks() {",
        "_ws_candidates() {",
    ] {
        let at = zsh
            .find(helper)
            .unwrap_or_else(|| panic!("zsh: {helper} missing"));
        assert!(at < dispatch, "zsh: {helper} comes after the dispatch");
    }
    assert!(bash.contains("_ws_candidates tasks "));

    let fish = ws(&root).args(["completions", "fish"]).assert().success();
    let fish = String::from_utf8(fish.get_output().stdout.clone()).unwrap();
    assert_eq!(fish.matches("ws __complete worktrees").count(), 3);
    assert_eq!(fish.matches("ws __complete tasks").count(), 1);
    assert_eq!(fish.matches("ws __complete branches").count(), 1);
    assert_eq!(fish.matches("ws __complete refs").count(), 1);
    ws(&root)
        .args(["completions", "nushell"])
        .assert()
        .failure();
}

#[test]
fn completion_candidates_are_the_branches_with_worktrees() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo(&origins, "api");
    let web = origin_repo(&origins, "web");

    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!(
            "name: acme\nrepos:\n  - url: \"{}\"\n  - url: \"{}\"\n",
            api.display(),
            web.display()
        ),
    );
    ws(&root).arg("fetch").assert().success();

    // Nothing is checked out yet, so there is no worktree to offer — but the
    // branches of the repositories are all there is to switch to.
    let empty = ws(&root)
        .args(["__complete", "worktrees"])
        .assert()
        .success();
    assert!(empty.get_output().stdout.is_empty());
    let all = ws(&root)
        .args(["__complete", "branches"])
        .assert()
        .success();
    let all = String::from_utf8(all.get_output().stdout.clone()).unwrap();
    assert_eq!(
        all.lines().collect::<Vec<_>>(),
        [
            // Both repositories have both branches: one line each, naming the
            // repositories the branch was found in.
            "feature/JIRA-123\tapi, web",
            "main\tapi, web"
        ],
        "all: {all}"
    );

    ws(&root).args(["switch", "main"]).assert().success();
    ws(&root)
        .args(["switch", "feature/JIRA-123"])
        .assert()
        .success();

    let out = ws(&root)
        .args(["__complete", "worktrees"])
        .assert()
        .success();
    let listed = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let lines: Vec<&str> = listed.lines().collect();
    assert_eq!(lines.len(), 2, "listed: {listed}");
    // Value and description, tab separated, the description naming the
    // directory it would remove when that is not the branch name itself.
    assert_eq!(lines[0], "feature/JIRA-123\tfeature-JIRA-123 — api, web");
    assert_eq!(lines[1], "main\tapi, web");

    // A branch with a worktree is offered by `switch` too, marked as such,
    // and never twice however many repositories it is in.
    let all = ws(&root)
        .args(["__complete", "branches"])
        .assert()
        .success();
    let all = String::from_utf8(all.get_output().stdout.clone()).unwrap();
    assert_eq!(
        all.lines().collect::<Vec<_>>(),
        [
            "feature/JIRA-123\tchecked out — api, web",
            "main\tchecked out — api, web"
        ],
        "all: {all}"
    );

    // Outside a workspace it is an empty list, not an error — a shell asks
    // this on every <Tab>, wherever the prompt happens to be.
    for what in ["worktrees", "branches"] {
        let outside = ws(tmp.path()).args(["__complete", what]).assert().success();
        assert!(outside.get_output().stdout.is_empty());
    }
}

#[test]
fn from_starts_a_new_branch_at_a_tag_a_remote_ref_or_a_commit() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo(&origins, "api");

    // A second commit on main, with the tag left behind on the first one, so
    // the base `--from` picks is visible in the worktree's history.
    git(&api, &["checkout", "-q", "main"]);
    git(&api, &["tag", "-a", "release/v0.4.5", "-m", "release"]);
    fs::write(api.join("later.txt"), "later\n").unwrap();
    git(&api, &["add", "."]);
    git(&api, &["commit", "-m", "later"]);

    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!("repos:\n  - name: api\n    url: \"{}\"\n", api.display()),
    );
    ws(&root).arg("fetch").assert().success();

    // A tag, which the default base would not have given: the later commit
    // is not in the new branch.
    ws(&root)
        .args(["switch", "hotfix/tag", "--from", "release/v0.4.5"])
        .assert()
        .success();
    assert!(!root.join("hotfix-tag/later.txt").exists());

    // The remote form of a branch, written both ways.
    for (branch, from) in [
        ("hotfix/remote", "origin/feature/JIRA-123"),
        ("hotfix/plain", "feature/JIRA-123"),
    ] {
        ws(&root)
            .args(["switch", branch, "--from", from])
            .assert()
            .success();
    }

    // A ref the repository does not have is refused outright.
    let refused = ws(&root)
        .args(["switch", "hotfix/missing", "--from", "no/such/ref"])
        .assert()
        .failure();
    let message = String::from_utf8(refused.get_output().stderr.clone()).unwrap();
    assert!(message.contains("is not in api"), "stderr: {message}");
    assert!(!root.join("hotfix-missing").exists());

    // An existing branch is checked out as it is; `--from` says so instead of
    // quietly starting it somewhere else.
    let existing = ws(&root)
        .args(["switch", "feature/JIRA-123", "--from", "release/v0.4.5"])
        .assert()
        .success();
    let warning = String::from_utf8(existing.get_output().stderr.clone()).unwrap();
    assert!(warning.contains("--from is not used"), "stderr: {warning}");
}

#[test]
fn from_must_be_in_every_repository_before_anything_is_created() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo(&origins, "api");
    let web = origin_repo(&origins, "web");
    // The tag is in one repository only.
    git(&api, &["tag", "-a", "release/v0.4.5", "-m", "release"]);

    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!(
            "name: acme\nrepos:\n  - url: \"{}\"\n  - url: \"{}\"\n",
            api.display(),
            web.display()
        ),
    );
    ws(&root).arg("fetch").assert().success();

    let refused = ws(&root)
        .args(["switch", "hotfix/x", "--from", "release/v0.4.5"])
        .assert()
        .failure();
    let message = String::from_utf8(refused.get_output().stderr.clone()).unwrap();
    assert!(message.contains("is not in web"), "stderr: {message}");
    // Nothing of the branch exists: api must not be left with a worktree the
    // other repository never got.
    assert!(!root.join("hotfix-x").exists());
}

#[test]
fn a_new_branch_forks_from_the_branch_directory_it_was_started_in() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo(&origins, "api");

    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!("repos:\n  - name: api\n    url: \"{}\"\n", api.display()),
    );

    // A branch with a commit of its own, which the base branch does not have.
    ws(&root)
        .args(["switch", "feature/parent"])
        .assert()
        .success();
    let parent = root.join("feature-parent");
    fs::write(parent.join("only-here.txt"), "x\n").unwrap();
    git(&parent, &["add", "."]);
    git(&parent, &["commit", "-m", "parent work"]);

    // Run from inside that directory: the new branch continues it.
    Command::cargo_bin("ws")
        .unwrap()
        .current_dir(&parent)
        .env("WS_ZOXIDE_CMD", "true")
        .env("WS_CONFIG", "")
        .args(["--mux", "none", "switch", "feature/child"])
        .assert()
        .success();
    assert!(root.join("feature-child/only-here.txt").exists());

    // One repository, so the branch directory is its worktree: `ws` leaves
    // nothing of its own in there, where `git add .` would commit it.
    assert!(!root.join("feature-child/.ws.yaml").exists());
    assert!(!root.join("feature-parent/.ws.yaml").exists());

    // From the workspace root there is no branch directory to continue, so
    // the base branch is the base again.
    ws(&root)
        .args(["switch", "feature/sibling"])
        .assert()
        .success();
    assert!(!root.join("feature-sibling/only-here.txt").exists());
}

#[test]
fn the_branch_marker_names_the_directory_it_is_in() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo(&origins, "api");
    let web = origin_repo(&origins, "web");

    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!(
            "name: acme\nrepos:\n  - url: \"{}\"\n  - url: \"{}\"\n",
            api.display(),
            web.display()
        ),
    );
    ws(&root)
        .args(["switch", "feature/JIRA-123"])
        .assert()
        .success();

    let dir = root.join("feature-JIRA-123");
    let marker: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&fs::read_to_string(dir.join(".ws.yaml")).unwrap())
            .expect("the marker is yaml");
    assert_eq!(marker["branch"].as_str(), Some("feature/JIRA-123"));
    // A bare clone brings the branch along, so `ws` did not start it
    // anywhere and says nothing about where it began.
    assert!(marker.get("from").is_none(), "{marker:?}");

    // A branch `ws` does create records what it was created from.
    ws(&root).args(["switch", "feature/new"]).assert().success();
    let created: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&fs::read_to_string(root.join("feature-new/.ws.yaml")).unwrap())
            .unwrap();
    assert_eq!(created["from"].as_str(), Some("origin/main"));

    // A marker that does not name the directory it sits in is not one: a repo
    // may carry a `.ws.yaml` of its own in its files.
    fs::write(
        dir.join("api/.ws.yaml"),
        "branch: somewhere/else\nfrom: nowhere\n",
    )
    .unwrap();
    let status = Command::cargo_bin("ws")
        .unwrap()
        .current_dir(dir.join("api"))
        .env("WS_ZOXIDE_CMD", "true")
        .env("WS_CONFIG", "")
        .args(["--mux", "none", "status"])
        .assert()
        .success();
    let status = String::from_utf8(status.get_output().stdout.clone()).unwrap();
    assert!(status.contains("branch     feature/JIRA-123"), "{status}");

    // Removing the branch takes the marker with it.
    ws(&root)
        .args(["rm", "feature/JIRA-123", "--force"])
        .assert()
        .success();
    assert!(!dir.exists());
}

/// A repository that is a node project of whichever manager `lockfile` marks,
/// with the same `feature/JIRA-123` branch as [`origin_repo`].
fn node_repo(parent: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
    let path = parent.join(name);
    fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "--initial-branch=main", "."]);
    fs::write(path.join("package.json"), "{}\n").unwrap();
    fs::write(path.join(".gitignore"), "node_modules/\n").unwrap();
    for (file, body) in files {
        fs::write(path.join(file), body).unwrap();
    }
    git(&path, &["add", "."]);
    git(&path, &["commit", "-m", "init"]);
    git(&path, &["branch", "feature/JIRA-123"]);
    path
}

/// A package manager stand-in: answers `--version` with `version`, and on
/// anything else logs its arguments and whether a seeded `node_modules` was
/// already there, then leaves a `node_modules` of its own behind.
fn manager_stub(dir: &Path, name: &str, version: &str, log: &Path) -> PathBuf {
    stub(
        dir,
        name,
        &format!(
            r#"if [ "$1" = "--version" ]; then echo {version}; exit 0; fi
if [ -e node_modules/seed ]; then state=seeded; else state=empty; fi
printf '%s|%s|%s\n' "$*" "$PWD" "$state" >> {log}
mkdir -p node_modules/pkg
echo dep > node_modules/seed"#,
            log = log.display()
        ),
    )
}

fn single_repo_workspace(tmp: &Path, origin: &Path) -> PathBuf {
    let root = tmp.join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!("repos:\n  - url: \"{}\"\n", origin.display()),
    );
    root
}

#[test]
fn pnpm_installs_frozen_and_seeds_from_the_branch_you_came_from() {
    let tmp = TempDir::new().unwrap();
    let origin = node_repo(tmp.path(), "app", &[("pnpm-lock.yaml", "")]);
    let root = single_repo_workspace(tmp.path(), &origin);
    let log = tmp.path().join("pnpm.log");
    let pnpm = manager_stub(tmp.path(), "pnpm-stub", "9.1.0", &log);

    ws(&root)
        .env("WS_PNPM_CMD", &pnpm)
        .args(["switch", "main"])
        .assert()
        .success();
    let main = root.join("main");
    let first = fs::read_to_string(&log).unwrap();
    assert_eq!(
        first,
        format!("install --frozen-lockfile|{}|empty\n", main.display())
    );

    // From inside main: the new worktree starts with main's node_modules,
    // hard-linked, and the install reconciles it rather than starting over.
    ws(&main)
        .env("WS_PNPM_CMD", &pnpm)
        .args(["switch", "feature/JIRA-123"])
        .assert()
        .success();
    let feature = root.join("feature-JIRA-123");
    let second = fs::read_to_string(&log).unwrap();
    assert!(
        second.ends_with(&format!(
            "install --frozen-lockfile|{}|seeded\n",
            feature.display()
        )),
        "{second}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let original = fs::metadata(main.join("node_modules/seed")).unwrap();
        let replica = fs::metadata(feature.join("node_modules/seed")).unwrap();
        assert_eq!(original.ino(), replica.ino(), "not a hard link");
    }

    // Already installed: a second switch leaves it alone.
    ws(&main)
        .env("WS_PNPM_CMD", &pnpm)
        .args(["switch", "feature/JIRA-123"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(&log).unwrap(), second);
}

#[test]
fn seeding_falls_back_to_the_default_branch() {
    let tmp = TempDir::new().unwrap();
    let origin = node_repo(tmp.path(), "app", &[("pnpm-lock.yaml", "")]);
    git(&origin, &["branch", "other"]);
    let root = single_repo_workspace(tmp.path(), &origin);
    let log = tmp.path().join("pnpm.log");
    let pnpm = manager_stub(tmp.path(), "pnpm-stub", "9.1.0", &log);

    for branch in ["main", "feature/JIRA-123"] {
        ws(&root)
            .env("WS_PNPM_CMD", &pnpm)
            .args(["switch", branch])
            .assert()
            .success();
    }
    // From the workspace root, which is on no branch at all.
    let recorded = fs::read_to_string(&log).unwrap();
    assert!(recorded.ends_with("|seeded\n"), "{recorded}");
}

#[test]
fn npm_runs_ci_and_does_not_seed_what_ci_would_delete() {
    let tmp = TempDir::new().unwrap();
    let origin = node_repo(tmp.path(), "app", &[("package-lock.json", "{}")]);
    let root = single_repo_workspace(tmp.path(), &origin);
    let log = tmp.path().join("npm.log");
    let npm = manager_stub(tmp.path(), "npm-stub", "10.0.0", &log);

    ws(&root)
        .env("WS_NPM_CMD", &npm)
        .args(["switch", "main"])
        .assert()
        .success();
    ws(&root.join("main"))
        .env("WS_NPM_CMD", &npm)
        .args(["switch", "feature/JIRA-123"])
        .assert()
        .success();
    let recorded = fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = recorded.lines().collect();
    assert_eq!(lines.len(), 2, "{recorded}");
    for line in lines {
        assert!(
            line.starts_with("ci|") && line.ends_with("|empty"),
            "{line}"
        );
    }
}

#[test]
fn yarn_gets_the_lockfile_flag_its_version_understands() {
    for (version, flag) in [("4.1.0", "--immutable"), ("1.22.19", "--frozen-lockfile")] {
        let tmp = TempDir::new().unwrap();
        let origin = node_repo(tmp.path(), "app", &[("yarn.lock", "")]);
        let root = single_repo_workspace(tmp.path(), &origin);
        let log = tmp.path().join("yarn.log");
        let yarn = manager_stub(tmp.path(), "yarn-stub", version, &log);

        ws(&root)
            .env("WS_YARN_CMD", &yarn)
            .args(["switch", "main"])
            .assert()
            .success();
        let recorded = fs::read_to_string(&log).unwrap();
        assert!(
            recorded.starts_with(&format!("install {flag}|")),
            "{version}: {recorded}"
        );
    }
}

#[test]
fn a_failed_install_warns_but_still_switches() {
    let tmp = TempDir::new().unwrap();
    let origin = node_repo(tmp.path(), "app", &[("pnpm-lock.yaml", "")]);
    let root = single_repo_workspace(tmp.path(), &origin);
    let pnpm = stub(tmp.path(), "pnpm-stub", "exit 1");

    let out = ws(&root)
        .env("WS_PNPM_CMD", &pnpm)
        .args(["switch", "main"])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).into_owned();
    assert!(stderr.contains("warning:"), "{stderr}");
    assert!(root.join("main/.git").exists());
}

#[test]
fn no_setup_skips_the_package_managers_too() {
    let tmp = TempDir::new().unwrap();
    let origin = node_repo(tmp.path(), "app", &[("pnpm-lock.yaml", "")]);
    let root = single_repo_workspace(tmp.path(), &origin);
    let log = tmp.path().join("pnpm.log");
    let pnpm = manager_stub(tmp.path(), "pnpm-stub", "9.1.0", &log);

    ws(&root)
        .env("WS_PNPM_CMD", &pnpm)
        .args(["switch", "main", "--no-setup"])
        .assert()
        .success();
    assert!(!log.exists());
}

#[test]
fn a_disabled_manager_does_nothing() {
    let tmp = TempDir::new().unwrap();
    let origin = node_repo(tmp.path(), "app", &[("pnpm-lock.yaml", "")]);
    let root = single_repo_workspace(tmp.path(), &origin);
    fs::write(
        root.join("ws.yaml"),
        format!(
            "repos:\n  - url: \"{}\"\nintegrations:\n  pnpm:\n    enabled: false\n",
            origin.display()
        ),
    )
    .unwrap();
    let log = tmp.path().join("pnpm.log");
    let pnpm = manager_stub(tmp.path(), "pnpm-stub", "9.1.0", &log);

    ws(&root)
        .env("WS_PNPM_CMD", &pnpm)
        .args(["switch", "main"])
        .assert()
        .success();
    assert!(!log.exists());
}

#[test]
fn the_old_top_level_zoxide_key_is_rejected() {
    let tmp = TempDir::new().unwrap();
    write_config(tmp.path(), "zoxide:\n  enabled: false\n");
    ws(tmp.path()).arg("status").assert().failure();
}

#[test]
fn local_files_go_into_every_repository_of_a_grouped_workspace() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo(&origins, "api");
    let web = origin_repo(&origins, "web");
    let root = tmp.path().join("work");
    fs::create_dir_all(root.join("local/api")).unwrap();
    fs::create_dir_all(root.join("local/web/.vscode")).unwrap();
    fs::write(root.join("local/api/.env"), "API=1").unwrap();
    fs::write(root.join("local/web/.env"), "WEB=1").unwrap();
    fs::write(root.join("local/web/.vscode/settings.json"), "{}").unwrap();
    // Tracked in the repository: kept, not replaced.
    fs::write(root.join("local/web/README.md"), "local").unwrap();
    write_config(
        &root,
        &format!(
            r#"repos:
  - name: api
    url: "{}"
  - name: web
    url: "{}"
integrations:
  local:
    paths:
      - {{ pattern: '\.env', mode: copy }}
      - {{ pattern: '.*', mode: link }}
"#,
            api.display(),
            web.display()
        ),
    );

    ws(&root).args(["switch", "main"]).assert().success();
    let dir = root.join("main");
    assert_eq!(fs::read_to_string(dir.join("api/.env")).unwrap(), "API=1");
    assert!(!fs::symlink_metadata(dir.join("web/.env"))
        .unwrap()
        .is_symlink());
    assert_eq!(
        fs::read_link(dir.join("web/.vscode")).unwrap(),
        root.join("local/web/.vscode")
    );
    assert_eq!(
        fs::read_to_string(dir.join("web/README.md")).unwrap(),
        "# web\n"
    );
    // The local directory is not a branch directory.
    let listed = ws(&root).arg("list").output().unwrap();
    assert!(!String::from_utf8_lossy(&listed.stdout).contains("local"));
}

#[test]
fn a_single_repository_gets_the_whole_local_directory() {
    let tmp = TempDir::new().unwrap();
    let origin = origin_repo(tmp.path(), "app");
    let root = single_repo_workspace(tmp.path(), &origin);
    fs::create_dir_all(root.join("local")).unwrap();
    fs::write(root.join("local/.env"), "X=1").unwrap();

    ws(&root).args(["switch", "main"]).assert().success();
    assert_eq!(
        fs::read_link(root.join("main/.env")).unwrap(),
        root.join("local/.env")
    );
}

#[test]
fn a_branch_named_like_the_local_directory_is_refused() {
    let tmp = TempDir::new().unwrap();
    let origin = origin_repo(tmp.path(), "app");
    git(&origin, &["branch", "local"]);
    let root = single_repo_workspace(tmp.path(), &origin);
    fs::create_dir_all(root.join("local")).unwrap();
    fs::write(root.join("local/.env"), "X=1").unwrap();

    ws(&root).args(["switch", "local"]).assert().failure();
    assert!(root.join("local/.env").is_file());
}

#[test]
fn setup_interpolates_and_runs_multi_line_scripts_strictly() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo(&origins, "api");
    let web = origin_repo(&origins, "web");
    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!(
            r#"repos:
  - name: api
    url: "{}"
    setup: |
      echo "{{repo}} {{branch}} {{env:WS_TEST_TOKEN}}" > info
      echo "{{root}}|{{worktree}}|{{branch_dir}}" >> info
      echo "$WS_REPO $WS_WORKTREE" >> info
      awk '{{print $1}}' info > first
  - name: web
    url: "{}"
    setup: |
      false
      touch should-not-exist
"#,
            api.display(),
            web.display()
        ),
    );

    let out = ws(&root)
        .env("WS_TEST_TOKEN", "s3cret")
        .args(["switch", "main"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).into_owned();
    assert!(stderr.contains("setup command for web failed"), "{stderr}");

    let dir = root.join("main");
    let info = fs::read_to_string(dir.join("api/info")).unwrap();
    let canonical_root = root.canonicalize().unwrap();
    let wt = canonical_root.join("main/api");
    assert_eq!(
        info,
        format!(
            "api main s3cret\n{}|{}|{}\napi {}\n",
            canonical_root.display(),
            wt.display(),
            canonical_root.join("main").display(),
            wt.display()
        )
    );
    // awk's braces reached the shell untouched: it printed each first field.
    let first = fs::read_to_string(dir.join("api/first")).unwrap();
    let firsts: Vec<&str> = first.lines().collect();
    assert_eq!(firsts.len(), 3, "{first}");
    assert_eq!((firsts[0], firsts[2]), ("api", "api"));
    // `sh -e`: the failing first line stopped the script.
    assert!(!dir.join("web/should-not-exist").exists());
}

#[test]
fn setup_env_defaults_and_escaped_placeholders() {
    let tmp = TempDir::new().unwrap();
    let origin = origin_repo(tmp.path(), "app");
    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!(
            r#"repos:
  - url: "{}"
    setup: |
      echo "{{env:WS_E2E_UNSET:-dev}} {{env:WS_E2E_EMPTY:-was-empty}} {{env:WS_E2E_SET:-no}}" > out
      echo '{{{{env:WS_E2E_SET}}}} {{{{repo}}}}' >> out
"#,
            origin.display()
        ),
    );

    ws(&root)
        .env_remove("WS_E2E_UNSET")
        .env("WS_E2E_EMPTY", "")
        .env("WS_E2E_SET", "yes")
        .args(["switch", "main"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(root.join("main/out")).unwrap(),
        "dev was-empty yes\n{env:WS_E2E_SET} {repo}\n"
    );
}

#[test]
fn an_unset_variable_without_a_default_names_the_way_out() {
    let tmp = TempDir::new().unwrap();
    let origin = origin_repo(tmp.path(), "app");
    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!(
            "repos:\n  - url: \"{}\"\n    setup: echo {{env:WS_E2E_UNSET}}\n",
            origin.display()
        ),
    );
    let out = ws(&root)
        .env_remove("WS_E2E_UNSET")
        .args(["switch", "main"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).into_owned();
    assert!(stderr.contains("WS_E2E_UNSET is not set"), "{stderr}");
    assert!(stderr.contains("{env:WS_E2E_UNSET:-"), "{stderr}");
}

/// A repository whose `main` carries the given files.
fn origin_repo_with(parent: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
    let path = origin_repo(parent, name);
    for (file, body) in files {
        fs::write(path.join(file), body).unwrap();
    }
    git(&path, &["add", "."]);
    git(&path, &["commit", "-m", "tasks"]);
    path
}

/// Three repositories — a justfile, a package.json, and neither — checked
/// out on `main`, with `just` and `npm` stubbed out.
fn task_workspace(tmp: &Path, extra: &str) -> (PathBuf, PathBuf) {
    let origins = tmp.join("origins");
    fs::create_dir_all(&origins).unwrap();
    let api = origin_repo_with(&origins, "api", &[("justfile", "dev:\n  true\n")]);
    let web = origin_repo_with(
        &origins,
        "web",
        &[(
            "package.json",
            r#"{"scripts": {"dev": "vite", "build": "vite build", "fail": "exit 2"}}"#,
        )],
    );
    let docs = origin_repo(&origins, "docs");

    let bin = tmp.join("bin");
    fs::create_dir_all(&bin).unwrap();
    stub(
        &bin,
        "just",
        r#"if [ "$1" = --dump ]; then
  echo '{"recipes": {"dev": {"doc": "serve the api", "private": false},
                     "_hidden": {"doc": null, "private": true}}, "modules": {}}'
  exit 0
fi
echo "just $* in $WS_REPO on $WS_BRANCH""#,
    );
    stub(
        &bin,
        "npm",
        r#"echo "npm $*"
echo "careful" >&2
[ "$2" = fail ] && exit 2
exit 0"#,
    );

    let root = tmp.join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!(
            "repos:\n  - url: \"{}\"\n  - url: \"{}\"\n  - url: \"{}\"\n\
             tasks:\n  task: {{ enabled: false }}\n{extra}",
            api.display(),
            web.display(),
            docs.display()
        ),
    );
    ws(&root).arg("fetch").assert().success();
    ws(&root)
        .args(["switch", "main", "--no-setup"])
        .assert()
        .success();
    (root, bin)
}

fn fails_with(cmd: &mut Command, message: &str) {
    let out = cmd.output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains(message), "{stderr}");
}

fn ws_tasks(root: &Path, bin: &Path) -> Command {
    let mut cmd = ws(root);
    cmd.env("WS_JUST_CMD", bin.join("just"))
        .env("WS_NPM_CMD", bin.join("npm"));
    cmd
}

#[test]
fn run_lists_the_tasks_of_every_repository() {
    let tmp = TempDir::new().unwrap();
    let (root, bin) = task_workspace(tmp.path(), "");

    let out = ws_tasks(&root, &bin).arg("run").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("api\n  dev    just     serve the api"),
        "{text}"
    );
    assert!(
        text.contains("web\n  build  package  vite build\n  dev    package  vite\n"),
        "{text}"
    );
    assert!(text.contains("docs\n  (no tasks)"), "{text}");
    assert!(!text.contains("_hidden"), "{text}");

    let out = ws_tasks(&root, &bin)
        .args(["run", "@web"])
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("web\n"), "{text}");
    assert!(!text.contains("api"), "{text}");

    let out = ws_tasks(&root, &bin)
        .args(["__complete", "tasks", "@w"])
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("@web:build\tpackage vite build\n"), "{text}");
}

#[test]
fn run_runs_a_task_everywhere_it_exists_with_a_prefix_per_repository() {
    let tmp = TempDir::new().unwrap();
    let (root, bin) = task_workspace(tmp.path(), "");

    let out = ws_tasks(&root, &bin).args(["run", "dev"]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    // Padded to the longest label; no colour without a terminal.
    assert!(
        stdout.contains("api | just dev in api on main\n"),
        "{stdout}"
    );
    assert!(stdout.contains("web | npm run dev\n"), "{stdout}");
    assert!(stderr.contains("web | careful\n"), "{stderr}");
    assert!(!stdout.contains("docs"), "{stdout}");
    assert!(!stdout.contains('\x1b'), "{stdout}");

    // One repository, the arguments after `--` passed on; npm wants its own `--`.
    let out = ws_tasks(&root, &bin)
        .args(["run", "@web:build", "--", "--watch"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        stdout.lines().next(),
        Some("web | npm run build -- --watch"),
        "{stdout}"
    );

    // Different tasks are told apart by both names.
    let out = ws_tasks(&root, &bin)
        .args(["run", "@api:dev", "@web:build"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("api:dev   | just dev"), "{stdout}");
    assert!(stdout.contains("web:build | npm run build"), "{stdout}");

    // From inside a repository the branch is the one you stand in.
    ws_tasks(&root.join("main/web"), &bin)
        .args(["run", "@api:dev"])
        .assert()
        .success();
}

#[test]
fn run_fails_when_a_task_fails_or_does_not_exist() {
    let tmp = TempDir::new().unwrap();
    let (root, bin) = task_workspace(tmp.path(), "");

    let out = ws_tasks(&root, &bin)
        .args(["run", "fail", "@api:dev"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    // The other task still ran to the end.
    assert!(stdout.contains("api:dev  | just dev"), "{stdout}");
    assert!(stderr.contains("web:fail | exited with code 2"), "{stderr}");
    assert!(stderr.contains("1 of 2 failed: web:fail"), "{stderr}");

    fails_with(
        ws_tasks(&root, &bin).args(["run", "nope"]),
        "has a task named nope",
    );
    fails_with(
        ws_tasks(&root, &bin).args(["run", "@docs:dev"]),
        "docs has no task named dev",
    );
    fails_with(
        ws_tasks(&root, &bin).args(["run", "@nope:dev"]),
        "no repository named nope",
    );
    fails_with(
        ws_tasks(&root, &bin).args(["run", "dev", "--branch", "feature/JIRA-123"]),
        "has no worktrees",
    );
}

#[test]
fn run_logs_every_task_to_its_own_rotated_file() {
    let tmp = TempDir::new().unwrap();
    let (root, bin) = task_workspace(
        tmp.path(),
        "logs:\n  enabled: true\n  max_size: 100\n  keep: 1\n  timestamps: false\n",
    );

    ws_tasks(&root, &bin)
        .args(["run", "dev"])
        .assert()
        .success();
    let log = root.join("main/logs/web.dev.log");
    let text = fs::read_to_string(&log).unwrap();
    // stdout and stderr are separate pipes, so the order between their lines
    // is the order they were read in — not something to assert on.
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4, "{text}");
    assert!(
        lines[0].starts_with("--- ") && lines[0].ends_with("npm run dev ---"),
        "{text}"
    );
    let mut middle = lines[1..3].to_vec();
    middle.sort();
    assert_eq!(middle, ["careful", "npm run dev"], "{text}");
    assert_eq!(lines[3], "--- exited with code 0 ---", "{text}");
    assert!(root.join("main/logs/api.dev.log").is_file());

    // The second run pushes the file past 100 bytes: what it had moves to
    // `.1`, and with `keep: 1` nothing older survives.
    ws_tasks(&root, &bin)
        .args(["run", "@web:dev"])
        .assert()
        .success();
    ws_tasks(&root, &bin)
        .args(["run", "@web:dev"])
        .assert()
        .success();
    assert!(root.join("main/logs/web.dev.log.1").is_file());
    assert!(!root.join("main/logs/web.dev.log.2").exists());
    let current = fs::read_to_string(&log).unwrap();
    assert!(current.len() <= 100, "{current}");

    // --no-logs keeps a run out of them.
    fs::remove_dir_all(root.join("main/logs")).unwrap();
    ws_tasks(&root, &bin)
        .args(["run", "dev", "--no-logs"])
        .assert()
        .success();
    assert!(!root.join("main/logs").exists());

    // The logs belong to the branch directory, and go with it.
    ws_tasks(&root, &bin)
        .args(["run", "dev", "--logs"])
        .assert()
        .success();
    ws(&root).args(["rm", "main"]).assert().success();
    assert!(!root.join("main").exists());
}

#[test]
fn run_logs_in_a_single_repository_do_not_dirty_its_worktree() {
    let tmp = TempDir::new().unwrap();
    let origins = tmp.path().join("origins");
    fs::create_dir_all(&origins).unwrap();
    let web = origin_repo_with(
        &origins,
        "web",
        &[("package.json", r#"{"scripts": {"dev": "vite"}}"#)],
    );
    let bin = tmp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    stub(&bin, "npm", "echo \"npm $*\"");
    let root = tmp.path().join("work");
    fs::create_dir_all(&root).unwrap();
    write_config(
        &root,
        &format!(
            "repos:\n  - url: \"{}\"\nlogs:\n  enabled: true\n",
            web.display()
        ),
    );
    ws(&root).arg("fetch").assert().success();
    ws(&root)
        .args(["switch", "main", "--no-setup"])
        .assert()
        .success();

    ws_tasks(&root, &bin)
        .args(["run", "dev"])
        .assert()
        .success();
    assert!(root.join("main/logs/web.dev.log").is_file());
    let out = ws(&root).arg("list").output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(!text.contains("web*"), "{text}");

    // Not dirty, so no --force; and the worktree comes out whole.
    ws(&root).args(["rm", "main"]).assert().success();
    assert!(!root.join("main").exists());
}

#[test]
fn run_logs_can_be_kept_in_the_workspace_root_instead() {
    let tmp = TempDir::new().unwrap();
    let (root, bin) = task_workspace(
        tmp.path(),
        "logs:\n  enabled: true\n  dir: logs\n  file: \"{branch}/{repo}.{task}.log\"\n",
    );
    ws_tasks(&root, &bin)
        .args(["run", "dev"])
        .assert()
        .success();
    assert!(root.join("logs/main/web.dev.log").is_file());
    assert!(!root.join("main/logs").exists());
}

#[test]
fn run_names_a_worktree_git_has_lost_track_of() {
    let tmp = TempDir::new().unwrap();
    let (root, bin) = task_workspace(tmp.path(), "");
    // What moving the workspace does to a worktree: its `.git` file points
    // at an administrative directory that is no longer there.
    // Both sides still name each other by their old absolute paths.
    let web = root.join("main/web");
    fs::write(
        web.join(".git"),
        "gitdir: /old/.bare/web.git/worktrees/web\n",
    )
    .unwrap();
    for admin in fs::read_dir(root.join(".bare/web.git/worktrees")).unwrap() {
        fs::write(admin.unwrap().path().join("gitdir"), "/old/main/web/.git\n").unwrap();
    }

    // From the branch directory, and from the root, where the record of the
    // old path must not pass for the default branch's directory.
    for dir in [root.join("main"), root.clone()] {
        let out = ws_tasks(&dir, &bin)
            .args(["run", "@web:dev"])
            .output()
            .unwrap();
        assert!(!out.status.success());
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(
            stderr.contains("git does not list it as a worktree of web"),
            "{stderr}"
        );
        assert!(stderr.contains("worktree repair"), "{stderr}");
    }
}
