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
        .args(["--mux", "none"]);
    cmd
}

fn write_config(root: &Path, body: &str) {
    fs::write(root.join("ws.toml"), body).unwrap();
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
name = "acme"
[[repo]]
name = "api"
url = "{}"
setup = "touch setup-done"

[[repo]]
name = "web"
url = "{}"
setup = ["touch setup-done", "touch second"]
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
        &format!("[[repo]]\nname = \"api\"\nurl = \"{}\"\n", api.display()),
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
            "branch_dirs = \"nested\"\nrepo_layout = \"grouped\"\n\
             [[repo]]\nname = \"api\"\nurl = \"{}\"\n",
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
        &format!("[[repo]]\nname = \"api\"\nurl = \"{}\"\n", api.display()),
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
        &format!("[[repo]]\nname = \"api\"\nurl = \"{}\"\n", api.display()),
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
            "[[repo]]\nname = \"api\"\nurl = \"{}\"\n[[repo]]\nname = \"web\"\nurl = \"{}\"\n",
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
            "[[repo]]\nname = \"api\"\nurl = \"{}\"\n\n\
             [ui]\npicker = \"inline\"\npicker_cmd = [\"{}\"]\n",
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
        &format!("[[repo]]\nname = \"api\"\nurl = \"{}\"\n", api.display()),
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
            "worktree_dir = \"trees\"\n\
             [[repo]]\nname = \"api\"\nurl = \"{}\"\n\
             [[repo]]\nname = \"web\"\nurl = \"{}\"\npath = \"services/web\"\n",
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
            "worktree_dir = \"{}\"\nrepo_layout = \"grouped\"\n\
             [[repo]]\nname = \"api\"\nurl = \"{}\"\n",
            elsewhere.display(),
            api.display()
        ),
    );

    ws(&root).args(["switch", "main"]).assert().success();
    let worktree = elsewhere.join("main/api");
    assert!(worktree.join(".git").exists());

    // No ws.toml in any parent of the worktree: discovery has to follow the
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
    let text = fs::read_to_string(root.join("ws.toml")).unwrap();
    assert!(text.contains("name = \"demo\""));

    // A second init refuses to clobber.
    Command::cargo_bin("ws")
        .unwrap()
        .current_dir(&root)
        .arg("init")
        .assert()
        .failure();

    Command::cargo_bin("ws")
        .unwrap()
        .current_dir(&root)
        .args(["status"])
        .assert()
        .success();
}
