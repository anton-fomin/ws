//! `ws.toml` parsing, defaults and workspace discovery.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const CONFIG_FILE: &str = "ws.toml";
pub const STATE_DIR: &str = ".ws";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "def_default_branch")]
    pub default_branch: String,
    /// What new branches fork from. `None` means the bare clone's own HEAD.
    #[serde(default)]
    pub base_branch: Option<String>,
    #[serde(default)]
    pub repo_layout: RepoLayout,
    #[serde(default)]
    pub branch_dirs: BranchDirs,
    #[serde(default = "def_bare_dir")]
    pub bare_dir: String,
    /// Base directory the branch directories are created in. Relative to the
    /// workspace root, or absolute; `~` is expanded.
    #[serde(default = "def_worktree_dir")]
    pub worktree_dir: String,
    #[serde(default, rename = "repo")]
    pub repos: Vec<RepoConfig>,
    #[serde(default)]
    pub mux: MuxConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub zoxide: ZoxideConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoConfig {
    #[serde(default)]
    pub name: Option<String>,
    pub url: String,
    /// Where this repo sits inside a branch directory; defaults to its name.
    /// May be nested, e.g. `services/api`.
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub setup: Option<Setup>,
}

impl RepoConfig {
    /// Configured name, or the last path segment of the URL without `.git`.
    pub fn name(&self) -> String {
        if let Some(n) = &self.name {
            return n.clone();
        }
        repo_name_from_url(&self.url)
    }

    /// Directory of this repo relative to a branch directory.
    pub fn dir(&self) -> PathBuf {
        match &self.path {
            Some(path) => PathBuf::from(path),
            None => PathBuf::from(self.name()),
        }
    }

    pub fn setup_commands(&self) -> Vec<String> {
        match &self.setup {
            None => Vec::new(),
            Some(Setup::One(s)) => vec![s.clone()],
            Some(Setup::Many(v)) => v.clone(),
        }
    }
}

pub fn repo_name_from_url(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    let tail = trimmed
        .rsplit(['/', ':'])
        .find(|s| !s.is_empty())
        .unwrap_or(trimmed);
    tail.to_string()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Setup {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RepoLayout {
    /// One repo -> the branch directory itself is the worktree; more -> grouped.
    #[default]
    Auto,
    /// The branch directory is always the single repo's worktree.
    Single,
    /// The branch directory always holds one subdirectory per repo.
    Grouped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum BranchDirs {
    /// `feature/x` -> `feature-x`
    #[default]
    Flat,
    /// `feature/x` -> `feature/x`
    Nested,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct MuxConfig {
    pub backend: Backend,
    pub session: String,
    /// Name of the shared window/pane; empty disables it.
    pub root_window: String,
    pub on_none: OnNone,
    pub tmux: TmuxConfig,
    pub herdr: HerdrConfig,
}

impl Default for MuxConfig {
    fn default() -> Self {
        MuxConfig {
            backend: Backend::Auto,
            session: "{workspace}-{branch}".into(),
            root_window: "ws".into(),
            on_none: OnNone::PrintPath,
            tmux: TmuxConfig::default(),
            herdr: HerdrConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    #[default]
    Auto,
    Tmux,
    Herdr,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum OnNone {
    #[default]
    PrintPath,
    Nothing,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TmuxConfig {
    pub layout: TmuxLayout,
    pub command: Vec<String>,
}

impl Default for TmuxConfig {
    fn default() -> Self {
        TmuxConfig {
            layout: TmuxLayout::Windows,
            command: vec!["tmux".into()],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TmuxLayout {
    #[default]
    Windows,
    Panes,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HerdrConfig {
    pub split_direction: Direction,
    /// Named herdr session; only passed when running from outside herdr.
    pub session: Option<String>,
    pub command: Vec<String>,
}

impl Default for HerdrConfig {
    fn default() -> Self {
        HerdrConfig {
            split_direction: Direction::Right,
            session: None,
            command: vec!["herdr".into()],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    #[default]
    Right,
    Down,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Right => "right",
            Direction::Down => "down",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct UiConfig {
    pub picker: PickerMode,
    pub picker_cmd: Vec<String>,
    pub popup: Popup,
}

impl Default for UiConfig {
    fn default() -> Self {
        UiConfig {
            picker: PickerMode::Auto,
            picker_cmd: vec!["fzf".into()],
            popup: Popup::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PickerMode {
    #[default]
    Auto,
    Popup,
    Inline,
    None,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Popup {
    pub width: String,
    pub height: String,
}

impl Default for Popup {
    fn default() -> Self {
        Popup {
            width: "80%".into(),
            height: "60%".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ZoxideConfig {
    pub enabled: Toggle,
    pub register: Register,
    pub command: Vec<String>,
}

impl Default for ZoxideConfig {
    fn default() -> Self {
        ZoxideConfig {
            enabled: Toggle::default(),
            register: Register::Both,
            command: vec!["zoxide".into()],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Register {
    Worktrees,
    BranchDirs,
    #[default]
    Both,
    None,
}

/// `true` / `false` / `"auto"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum Toggle {
    Bool(bool),
    Named(NamedToggle),
}

impl Default for Toggle {
    fn default() -> Self {
        Toggle::Named(NamedToggle::Auto)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NamedToggle {
    Auto,
}

impl Toggle {
    /// Resolve against whether the tool is actually present.
    pub fn resolve(self, available: bool) -> bool {
        match self {
            Toggle::Bool(b) => b,
            Toggle::Named(NamedToggle::Auto) => available,
        }
    }
}

fn def_default_branch() -> String {
    "main".into()
}

fn def_bare_dir() -> String {
    ".bare".into()
}

fn def_worktree_dir() -> String {
    ".".into()
}

/// Resolve a configured directory against the workspace root.
///
/// `.` and the empty string mean the root itself; `~` is expanded from $HOME;
/// an absolute path is taken as is; anything else hangs off the root.
pub fn resolve_dir(root: &Path, value: &str) -> PathBuf {
    let value = value.trim();
    if value.is_empty() || value == "." {
        return root.to_path_buf();
    }
    if value == "~" || value.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            return match value.strip_prefix("~/") {
                Some(rest) => home.join(rest),
                None => home,
            };
        }
    }
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

/// A workspace root plus its parsed configuration.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub config: Config,
    /// Configuration files that went into `config`, least specific first.
    pub sources: Vec<PathBuf>,
}

impl Workspace {
    /// Walk up from `start` looking for `ws.toml`.
    pub fn discover(start: &Path) -> Result<Workspace> {
        let start = start
            .canonicalize()
            .with_context(|| format!("cannot resolve {}", start.display()))?;
        for dir in start.ancestors() {
            let candidate = dir.join(CONFIG_FILE);
            if candidate.is_file() {
                return Workspace::load(dir);
            }
        }
        // With `worktree_dir` pointing outside the workspace, no parent of a
        // worktree holds the config. The worktree's `.git` file points at the
        // bare repo, which does live under the root.
        if let Some(root) = root_via_gitdir(&start) {
            return Workspace::load(&root);
        }
        bail!(
            "no {CONFIG_FILE} found in {} or any parent directory — run `ws init` first",
            start.display()
        )
    }

    /// Load `ws.toml`, layered on top of the user's global configuration.
    ///
    /// Settings come from `~/.config/ws/config.toml`; the workspace file lists
    /// the repositories and overrides whatever it needs to, key by key.
    pub fn load(root: &Path) -> Result<Workspace> {
        let path = root.join(CONFIG_FILE);
        let local = read_toml(&path)?;
        let mut sources = Vec::new();

        let merged = match global_config_path().filter(|p| p.is_file()) {
            Some(global_path) => {
                let global = read_toml(&global_path)?;
                // Deserialize the global file on its own first, so an unknown
                // or malformed key is reported against the file that holds it.
                let standalone: Config = global
                    .clone()
                    .try_into()
                    .with_context(|| format!("cannot parse {}", global_path.display()))?;
                if !standalone.repos.is_empty() {
                    bail!(
                        "{}: repositories belong in a workspace's {CONFIG_FILE}, \
                         not in the global configuration",
                        global_path.display()
                    );
                }
                sources.push(global_path);
                merge_into(global, local)
            }
            None => local,
        };
        sources.push(path.clone());

        let config: Config = merged
            .try_into()
            .with_context(|| format!("cannot parse {}", path.display()))?;
        config.validate(&path)?;
        Ok(Workspace {
            root: root.to_path_buf(),
            config,
            sources,
        })
    }

    /// Workspace name: the `name` key, else the root directory name.
    pub fn name(&self) -> String {
        if let Some(n) = &self.config.name {
            return n.clone();
        }
        self.root
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "ws".into())
    }

    pub fn bare_dir(&self) -> PathBuf {
        resolve_dir(&self.root, &self.config.bare_dir)
    }

    /// Base directory the branch directories live in.
    pub fn worktree_root(&self) -> PathBuf {
        resolve_dir(&self.root, &self.config.worktree_dir)
    }

    pub fn bare_path(&self, repo: &RepoConfig) -> PathBuf {
        self.bare_dir().join(format!("{}.git", repo.name()))
    }

    pub fn state_path(&self) -> PathBuf {
        self.root.join(STATE_DIR).join("state.toml")
    }

    /// Whether each branch directory holds one subdirectory per repo.
    pub fn is_grouped(&self) -> bool {
        match self.config.repo_layout {
            RepoLayout::Grouped => true,
            RepoLayout::Single => false,
            RepoLayout::Auto => self.config.repos.len() > 1,
        }
    }
}

impl Config {
    fn validate(&self, path: &Path) -> Result<()> {
        let mut seen = HashSet::new();
        for repo in &self.repos {
            if repo.url.trim().is_empty() {
                bail!("{}: repo with an empty url", path.display());
            }
            let name = repo.name();
            if name.is_empty() || name.contains('/') || name.starts_with('.') {
                bail!("{}: invalid repo name {name:?}", path.display());
            }
            if !seen.insert(name.clone()) {
                bail!("{}: duplicate repo name {name:?}", path.display());
            }
            if let Some(sub) = &repo.path {
                let sub = sub.trim();
                if sub.is_empty() || Path::new(sub).is_absolute() {
                    bail!(
                        "{}: repo {name}: path must be relative and non-empty",
                        path.display()
                    );
                }
                if Path::new(sub)
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    bail!(
                        "{}: repo {name}: path must not contain '..'",
                        path.display()
                    );
                }
            }
        }
        if self.repo_layout == RepoLayout::Single && self.repos.len() > 1 {
            bail!(
                "{}: repo_layout = \"single\" needs exactly one repo, found {}",
                path.display(),
                self.repos.len()
            );
        }
        if self.bare_dir.trim().is_empty() || self.worktree_dir.trim().is_empty() {
            bail!(
                "{}: bare_dir and worktree_dir must not be empty",
                path.display()
            );
        }
        if self.mux.tmux.command.is_empty() || self.mux.herdr.command.is_empty() {
            bail!("{}: mux command must not be empty", path.display());
        }
        if self.ui.picker_cmd.is_empty() {
            bail!("{}: ui.picker_cmd must not be empty", path.display());
        }
        if self.zoxide.command.is_empty() {
            bail!("{}: zoxide.command must not be empty", path.display());
        }
        Ok(())
    }
}

fn read_toml(path: &Path) -> Result<toml::Value> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("cannot parse {}", path.display()))
}

/// Path of the user's global configuration.
///
/// `WS_CONFIG` overrides it; setting it to the empty string turns the global
/// layer off entirely, which is what the tests do so a developer's own file
/// cannot reach them.
pub fn global_config_path() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("WS_CONFIG") {
        if explicit.is_empty() {
            return None;
        }
        return Some(PathBuf::from(explicit));
    }
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("ws").join("config.toml"))
}

/// Lay `over` on top of `base`, one key at a time.
///
/// Tables merge so that a workspace setting one key of `[mux]` keeps the rest
/// of the global `[mux]`. Everything else, arrays included, is replaced.
fn merge_into(base: toml::Value, over: toml::Value) -> toml::Value {
    match (base, over) {
        (toml::Value::Table(mut base), toml::Value::Table(over)) => {
            for (key, value) in over {
                let merged = match base.remove(&key) {
                    Some(existing) => merge_into(existing, value),
                    None => value,
                };
                base.insert(key, merged);
            }
            toml::Value::Table(base)
        }
        (_, over) => over,
    }
}

/// Follow a worktree's `.git` file back to the workspace that owns it.
fn root_via_gitdir(start: &Path) -> Option<PathBuf> {
    let git_file = start
        .ancestors()
        .map(|dir| dir.join(".git"))
        .find(|path| path.is_file())?;
    let text = std::fs::read_to_string(&git_file).ok()?;
    let target = text
        .lines()
        .find_map(|line| line.strip_prefix("gitdir:"))?
        .trim();
    // `<root>/<bare_dir>/<repo>.git/worktrees/<id>`, absolute as git writes it,
    // but tolerate a relative path too.
    let gitdir = match Path::new(target) {
        path if path.is_absolute() => path.to_path_buf(),
        path => git_file.parent()?.join(path),
    };
    gitdir
        .ancestors()
        .find(|dir| dir.join(CONFIG_FILE).is_file())
        .map(Path::to_path_buf)
}

/// The `ws.toml` written by `ws init` — repositories and little else.
///
/// Settings live in the global configuration; a workspace only writes down
/// what is actually its own, and overrides the rest key by key when it has to.
pub fn local_template(name: &str, repos: &[String]) -> String {
    // Order matters: in TOML a bare key belongs to the table above it, so the
    // single-key overrides have to be offered before the first [[repo]] and
    // the table overrides after the last one.
    let mut out = format!(
        "name = \"{name}\"\n\
         \n\
         # Settings come from the global configuration, which `ws init --global`\n\
         # writes with every key documented. Override single keys here, above\n\
         # the repositories:\n\
         # branch_dirs = \"nested\"\n\
         # worktree_dir = \"~/trees/{name}\"\n\n"
    );

    if repos.is_empty() {
        out.push_str(
            "# One [[repo]] block per repository.\n\
             # [[repo]]\n\
             # name = \"api\"\n\
             # url = \"git@github.com:org/api.git\"\n\
             # path = \"services/api\"   # where it sits inside a branch directory\n\
             # setup = \"pnpm install\"  # runs once, in each new worktree\n\n",
        );
    } else {
        for url in repos {
            out.push_str(&format!(
                "[[repo]]\n\
                 name = \"{}\"\n\
                 url = \"{url}\"\n\
                 # setup = \"pnpm install\"\n\n",
                repo_name_from_url(url)
            ));
        }
    }

    out.push_str(
        "# Whole-section overrides belong below the repositories, for example:\n\
         # [mux]\n\
         # root_window = \"ws\"\n",
    );
    out
}

/// The commented settings file written by `ws init --global`. Hand-written on
/// purpose: serializing the struct would drop every comment.
pub fn global_template() -> String {
    "# ws — global configuration, applied to every workspace.\n\
     # A workspace's ws.toml lists the repositories and overrides any key here.\n\
     \n\
     # Branch checked out by `ws fetch`.\n\
     default_branch = \"main\"\n\
     # What new branches fork from; omit to use each repo's own default branch.\n\
     # base_branch = \"main\"\n\
     \n\
     # auto: one repo -> the branch directory is the worktree;\n\
     #       several -> <branch>/<repo>/. Force with \"single\" or \"grouped\".\n\
     repo_layout = \"auto\"\n\
     # flat: feature/x -> feature-x   |   nested: feature/x -> feature/x\n\
     branch_dirs = \"flat\"\n\
     # Both are resolved per workspace: relative to its ws.toml, or absolute\n\
     # with \"~\" expanded. An absolute worktree_dir here would put every\n\
     # workspace's branches in one directory, so set that one per workspace.\n\
     bare_dir = \".bare\"\n\
     worktree_dir = \".\"\n\
     \n\
     [mux]\n\
     # auto | tmux | herdr | none\n\
     backend = \"auto\"\n\
     # placeholders: {workspace} {branch} {branch_raw}\n\
     session = \"{workspace}-{branch}\"\n\
     # shared window/pane for the branch directory; \"\" disables it\n\
     root_window = \"ws\"\n\
     # what to do with no multiplexer: print-path | nothing\n\
     on_none = \"print-path\"\n\
     \n\
     [mux.tmux]\n\
     layout = \"windows\"\n\
     command = [\"tmux\"]\n\
     \n\
     [mux.herdr]\n\
     split_direction = \"right\"\n\
     command = [\"herdr\"]\n\
     # command = [\"nix\", \"run\", \"nixpkgs#herdr\", \"--\"]\n\
     \n\
     [ui]\n\
     # auto | popup | inline | none\n\
     picker = \"auto\"\n\
     picker_cmd = [\"fzf\"]\n\
     popup = { width = \"80%\", height = \"60%\" }\n\
     \n\
     [zoxide]\n\
     enabled = \"auto\"\n\
     # worktrees | branch-dirs | both | none\n\
     register = \"both\"\n\
     command = [\"zoxide\"]\n"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_fill_in() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.default_branch, "main");
        assert_eq!(cfg.bare_dir, ".bare");
        assert_eq!(cfg.branch_dirs, BranchDirs::Flat);
        assert_eq!(cfg.repo_layout, RepoLayout::Auto);
        assert_eq!(cfg.mux.session, "{workspace}-{branch}");
        assert_eq!(cfg.mux.tmux.command, ["tmux"]);
        assert_eq!(cfg.ui.picker, PickerMode::Auto);
        assert_eq!(cfg.zoxide.register, Register::Both);
        assert_eq!(cfg.zoxide.enabled, Toggle::Named(NamedToggle::Auto));
    }

    #[test]
    fn repo_name_defaults_to_url_tail() {
        for (url, want) in [
            ("git@github.com:org/api.git", "api"),
            ("https://github.com/org/web", "web"),
            ("file:///srv/git/core.git/", "core"),
            ("git@github.com:org/api", "api"),
        ] {
            assert_eq!(repo_name_from_url(url), want, "{url}");
        }
    }

    #[test]
    fn setup_accepts_string_or_list() {
        let cfg: Config = toml::from_str(
            r#"
            [[repo]]
            url = "x/a.git"
            setup = "pnpm i"
            [[repo]]
            url = "x/b.git"
            setup = ["pnpm i", "cp .env.example .env"]
            "#,
        )
        .unwrap();
        assert_eq!(cfg.repos[0].setup_commands(), ["pnpm i"]);
        assert_eq!(cfg.repos[1].setup_commands().len(), 2);
    }

    #[test]
    fn zoxide_enabled_accepts_bool_and_auto() {
        let cfg: Config = toml::from_str("[zoxide]\nenabled = false\n").unwrap();
        assert_eq!(cfg.zoxide.enabled, Toggle::Bool(false));
        assert!(!cfg.zoxide.enabled.resolve(true));
        assert!(Toggle::default().resolve(true));
        assert!(!Toggle::default().resolve(false));
    }

    #[test]
    fn duplicate_repo_names_are_rejected() {
        let cfg: Config = toml::from_str(
            r#"
            [[repo]]
            url = "a/api.git"
            [[repo]]
            url = "b/api.git"
            "#,
        )
        .unwrap();
        assert!(cfg.validate(Path::new("ws.toml")).is_err());
    }

    #[test]
    fn resolve_dir_handles_root_absolute_and_home() {
        let root = Path::new("/w");
        assert_eq!(resolve_dir(root, "."), PathBuf::from("/w"));
        assert_eq!(resolve_dir(root, ""), PathBuf::from("/w"));
        assert_eq!(resolve_dir(root, "trees"), PathBuf::from("/w/trees"));
        assert_eq!(resolve_dir(root, "/srv/trees"), PathBuf::from("/srv/trees"));
        assert_eq!(resolve_dir(root, "../trees"), PathBuf::from("/w/../trees"));
        // Read $HOME rather than setting it: the tests share one process.
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            assert_eq!(resolve_dir(root, "~/trees"), home.join("trees"));
            assert_eq!(resolve_dir(root, "~"), home);
        }
    }

    #[test]
    fn repo_dir_defaults_to_the_name_and_may_be_nested() {
        let cfg: Config = toml::from_str(
            r#"
            [[repo]]
            url = "x/api.git"
            [[repo]]
            name = "web"
            url = "x/web.git"
            path = "services/web"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.repos[0].dir(), PathBuf::from("api"));
        assert_eq!(cfg.repos[1].dir(), PathBuf::from("services/web"));
        cfg.validate(Path::new("ws.toml")).unwrap();
    }

    #[test]
    fn a_repo_path_may_not_escape_the_branch_directory() {
        for bad in ["/abs", "../up", "a/../../b", ""] {
            let cfg: Config = toml::from_str(&format!(
                "[[repo]]\nname = \"api\"\nurl = \"x/api.git\"\npath = \"{bad}\"\n"
            ))
            .unwrap();
            assert!(
                cfg.validate(Path::new("ws.toml")).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(toml::from_str::<Config>("nope = 1").is_err());
    }

    #[test]
    fn both_templates_parse_back() {
        let local = local_template("demo", &["git@github.com:org/api.git".into()]);
        let cfg: Config = toml::from_str(&local).unwrap();
        assert_eq!(cfg.name.as_deref(), Some("demo"));
        assert_eq!(cfg.repos.len(), 1);
        assert_eq!(cfg.repos[0].name(), "api");
        cfg.validate(Path::new("ws.toml")).unwrap();

        let global: Config = toml::from_str(&global_template()).unwrap();
        assert!(global.repos.is_empty(), "the global template has no repos");
        global.validate(Path::new("config.toml")).unwrap();
    }

    #[test]
    fn uncommenting_the_local_templates_hints_produces_valid_config() {
        // A bare key after a [[repo]] header would land inside that repo, so
        // the template has to offer its examples in workable positions.
        let text = local_template("demo", &["git@github.com:org/api.git".into()]);
        let uncommented: String = text
            .lines()
            .map(|line| match line.trim_start().strip_prefix("# ") {
                // A commented example is a table header or a `key = value`;
                // anything else in the template is prose and stays a comment.
                Some(rest) if rest.starts_with('[') || rest.contains(" = ") => rest.to_string(),
                _ => line.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        let cfg: Config = toml::from_str(&uncommented)
            .unwrap_or_else(|e| panic!("{e}\n--- in ---\n{uncommented}"));
        assert_eq!(cfg.branch_dirs, BranchDirs::Nested);
        assert_eq!(cfg.mux.root_window, "ws");
        assert_eq!(cfg.repos.len(), 1);
        cfg.validate(Path::new("ws.toml")).unwrap();
    }

    #[test]
    fn the_local_template_carries_only_identity_and_repos() {
        let text = local_template("demo", &["git@github.com:org/api.git".into()]);
        let value: toml::Value = toml::from_str(&text).unwrap();
        let keys: Vec<&str> = value
            .as_table()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["name", "repo"]);
    }

    #[test]
    fn merging_replaces_keys_and_keeps_the_rest_of_a_table() {
        let global: toml::Value = toml::from_str(
            "branch_dirs = \"nested\"\n\
             [mux]\nbackend = \"herdr\"\nroot_window = \"ws\"\n\
             [ui]\npicker_cmd = [\"fzf\", \"--border\"]\n",
        )
        .unwrap();
        let local: toml::Value = toml::from_str(
            "name = \"acme\"\n\
             [mux]\nroot_window = \"acme\"\n\
             [ui]\npicker_cmd = [\"sk\"]\n",
        )
        .unwrap();

        let cfg: Config = merge_into(global, local).try_into().unwrap();
        assert_eq!(cfg.name.as_deref(), Some("acme"));
        // untouched by the workspace, so the global value survives
        assert_eq!(cfg.branch_dirs, BranchDirs::Nested);
        assert_eq!(cfg.mux.backend, Backend::Herdr);
        // overridden key by key, not table by table
        assert_eq!(cfg.mux.root_window, "acme");
        // arrays are replaced whole, never appended to
        assert_eq!(cfg.ui.picker_cmd, ["sk"]);
    }

    #[test]
    fn merging_an_empty_workspace_file_changes_nothing() {
        let global: toml::Value = toml::from_str("branch_dirs = \"nested\"\n").unwrap();
        let local: toml::Value = toml::from_str("").unwrap();
        let cfg: Config = merge_into(global, local).try_into().unwrap();
        assert_eq!(cfg.branch_dirs, BranchDirs::Nested);
    }
}
