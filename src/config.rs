//! `ws.yaml` parsing, defaults and workspace discovery.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_yaml_ng::{Mapping, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const CONFIG_FILE: &str = "ws.yaml";
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
    #[serde(default)]
    pub repos: Vec<RepoConfig>,
    #[serde(default)]
    pub mux: MuxConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub integrations: Integrations,
    #[serde(default)]
    pub tasks: TasksConfig,
    #[serde(default)]
    pub logs: LogsConfig,
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
            session: "{workspace}/{branch}".into(),
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
    pub layout: HerdrLayout,
    /// Only read by the `panes` layout.
    pub split_direction: Direction,
    /// Named herdr session; only passed when running from outside herdr.
    pub session: Option<String>,
    pub command: Vec<String>,
}

impl Default for HerdrConfig {
    fn default() -> Self {
        HerdrConfig {
            layout: HerdrLayout::Tabs,
            split_direction: Direction::Right,
            session: None,
            command: vec!["herdr".into()],
        }
    }
}

/// How the repositories of a branch are laid out inside the herdr workspace —
/// the counterpart of `[mux.tmux] layout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum HerdrLayout {
    #[default]
    Tabs,
    Panes,
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

/// The tools `ws` drives around a workspace, each under its own key.
///
/// Every one of them resolves `enabled: auto` against whether its program is
/// actually on `PATH`, so a workspace picks up what the machine has without
/// being told, and `enabled: false` opts out.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Integrations {
    pub zoxide: ZoxideConfig,
    pub local: LocalConfig,
    pub yarn: PackageManagerConfig,
    pub pnpm: PackageManagerConfig,
    pub npm: PackageManagerConfig,
    pub uv: PackageManagerConfig,
}

impl Integrations {
    /// The package managers, each paired with the key it is configured under.
    /// The order is the one detection falls back to when a repository holds
    /// more than one lockfile.
    pub fn package_managers(&self) -> [(Manager, &PackageManagerConfig); 4] {
        [
            (Manager::Yarn, &self.yarn),
            (Manager::Pnpm, &self.pnpm),
            (Manager::Npm, &self.npm),
            (Manager::Uv, &self.uv),
        ]
    }
}

/// A package manager `ws` knows how to drive.
///
/// What varies between them is held here rather than in the configuration, so
/// a workspace only writes down what it wants to differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager {
    Yarn,
    Pnpm,
    Npm,
    Uv,
}

impl Manager {
    /// Every manager, in the order detection prefers them.
    pub const ALL: [Manager; 4] = [Manager::Yarn, Manager::Pnpm, Manager::Npm, Manager::Uv];

    pub fn name(self) -> &'static str {
        match self {
            Manager::Yarn => "yarn",
            Manager::Pnpm => "pnpm",
            Manager::Npm => "npm",
            Manager::Uv => "uv",
        }
    }

    /// Directory the dependencies land in, relative to the project.
    pub fn dependency_dir(self) -> &'static str {
        match self {
            Manager::Uv => ".venv",
            _ => "node_modules",
        }
    }

    /// Files that mark a project as this manager's. The first one found wins,
    /// so the more specific name comes first.
    pub fn lockfiles(self) -> &'static [&'static str] {
        match self {
            Manager::Yarn => &["yarn.lock"],
            Manager::Pnpm => &["pnpm-lock.yaml"],
            Manager::Npm => &["package-lock.json", "npm-shrinkwrap.json"],
            Manager::Uv => &["uv.lock", "pyproject.toml"],
        }
    }

    /// The three that share `node_modules` and a `package.json`, and so are
    /// alternatives to one another rather than independent integrations.
    pub fn is_node(self) -> bool {
        !matches!(self, Manager::Uv)
    }

    /// Whether the dependency directory can be seeded from another worktree by
    /// hard-linking it at all. A virtualenv cannot: `pyvenv.cfg` and the
    /// shebangs in `bin/` carry the absolute path of the directory it was
    /// created in, so a copy of it points back at the original.
    pub fn links(self) -> bool {
        self.is_node()
    }

    /// Whether seeding is worth doing by default.
    ///
    /// `npm ci` deletes `node_modules` before it starts — that is what makes
    /// it reproducible — so seeding one for it is work thrown away. It is
    /// only turned off, not ruled out: a workspace that would rather have the
    /// speed sets `install: [install]` and `link: true` and gets both.
    pub fn seeds_by_default(self) -> bool {
        self.links() && self != Manager::Npm
    }

    /// Environment variable that overrides the launcher, like `WS_ZOXIDE_CMD`.
    pub fn command_env(self) -> &'static str {
        match self {
            Manager::Yarn => "WS_YARN_CMD",
            Manager::Pnpm => "WS_PNPM_CMD",
            Manager::Npm => "WS_NPM_CMD",
            Manager::Uv => "WS_UV_CMD",
        }
    }
}

/// One package manager's settings. Every key is optional and falls back to
/// what [`Manager`] says, so `yarn: { enabled: false }` is a complete entry.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PackageManagerConfig {
    pub enabled: Toggle,
    /// Launcher; defaults to the manager's own name.
    pub command: Option<Vec<String>>,
    /// Arguments that install the dependencies. Left out, each manager gets
    /// the ones that hold it to its lockfile — see
    /// [`crate::integration::pm::default_install_args`], which resolves them
    /// against the worktree because yarn's depend on its version.
    pub install: Option<Vec<String>>,
    /// Seed a new worktree's dependency directory from an existing one.
    /// Ignored by a manager that cannot be seeded that way.
    pub link: Option<bool>,
}

impl PackageManagerConfig {
    pub fn command(&self, manager: Manager) -> Vec<String> {
        match &self.command {
            Some(command) => command.clone(),
            None => vec![manager.name().to_string()],
        }
    }

    pub fn link(&self, manager: Manager) -> bool {
        manager.links() && self.link.unwrap_or(manager.seeds_by_default())
    }
}

/// Untracked files kept beside the workspace — `.env`, editor settings, a
/// personal `Makefile.local` — and placed into every worktree.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LocalConfig {
    /// `auto` means: whenever `dir` exists.
    pub enabled: Toggle,
    /// Where the files are kept, resolved like `bare_dir`. With several repos
    /// it holds one subdirectory per repo, named after it.
    pub dir: String,
    /// Rules matched in order against each entry's path relative to the
    /// repo's local directory; the first one that matches decides how it is
    /// placed, and an entry no rule matches is left out.
    pub paths: Vec<LocalPath>,
}

impl Default for LocalConfig {
    fn default() -> Self {
        LocalConfig {
            enabled: Toggle::default(),
            dir: def_local_dir(),
            paths: vec![LocalPath::default()],
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LocalPath {
    /// Regular expression the whole relative path has to match, with `/`
    /// between its components.
    pub pattern: String,
    pub mode: LocalMode,
}

impl Default for LocalPath {
    fn default() -> Self {
        LocalPath {
            pattern: ".*".into(),
            mode: LocalMode::Link,
        }
    }
}

impl LocalPath {
    /// The pattern, anchored at both ends: `.env` means `.env`, not every
    /// path with `.env` somewhere in it.
    pub fn regex(&self) -> Result<regex::Regex, regex::Error> {
        regex::Regex::new(&format!("^(?:{})$", self.pattern))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LocalMode {
    /// A symlink to the file in the local directory: one file, shared by
    /// every worktree, edited in one place.
    #[default]
    Link,
    /// An independent copy per worktree.
    Copy,
    /// A hard link: shared like `link`, but a real file to tools that refuse
    /// to follow a symlink. An editor that saves by renaming breaks it.
    Hardlink,
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

/// Where `ws run` finds a repository's tasks, and how it shows them.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TasksConfig {
    /// The sources a task name is looked up in, in order: the first one a
    /// repository has the task in runs it. A source left out is not read.
    pub order: Vec<TaskSource>,
    pub just: TaskToolConfig,
    pub task: TaskToolConfig,
    pub package: PackageScriptsConfig,
    /// Colour the prefixes — and let the tasks colour their own output,
    /// which they stop doing once it goes through a pipe.
    pub color: ColorMode,
}

impl Default for TasksConfig {
    fn default() -> Self {
        TasksConfig {
            order: TaskSource::ALL.to_vec(),
            just: TaskToolConfig::default(),
            task: TaskToolConfig::default(),
            package: PackageScriptsConfig::default(),
            color: ColorMode::Auto,
        }
    }
}

/// A file a repository declares its tasks in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskSource {
    /// A `justfile`, run with `just`.
    Just,
    /// A `Taskfile.yml`, run with go-task's `task`.
    Task,
    /// The `scripts` of `package.json`, run with the repository's own
    /// package manager.
    Package,
}

impl TaskSource {
    pub const ALL: [TaskSource; 3] = [TaskSource::Just, TaskSource::Task, TaskSource::Package];

    pub fn name(self) -> &'static str {
        match self {
            TaskSource::Just => "just",
            TaskSource::Task => "task",
            TaskSource::Package => "package",
        }
    }
}

/// `just` or `task`: on when the program is on `PATH`, unless told otherwise.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TaskToolConfig {
    pub enabled: Toggle,
    /// Launcher; defaults to the tool's own name.
    pub command: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PackageScriptsConfig {
    /// `auto` means: whenever a repository has a `package.json`.
    pub enabled: Toggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ColorMode {
    /// When the output is a terminal and `NO_COLOR` is not set.
    #[default]
    Auto,
    Always,
    Never,
}

/// Where the output of `ws run` is kept, beyond the terminal.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LogsConfig {
    pub enabled: bool,
    /// `{branch_dir}` stands for the branch directory the tasks run in;
    /// otherwise resolved like `bare_dir`.
    pub dir: String,
    /// The file inside `dir`, with `{branch}`, `{repo}` and `{task}` in it.
    /// Tasks whose files coincide share one, each line marked with its task.
    pub file: String,
    /// A file is rotated once it would grow past this: `10MB`, `512K`, or a
    /// number of bytes. `0` never rotates.
    pub max_size: ByteSize,
    /// Rotated files kept beside the live one, `<file>.1` the most recent.
    pub keep: u32,
    /// Start every line with the local time it was written.
    pub timestamps: bool,
}

impl Default for LogsConfig {
    fn default() -> Self {
        LogsConfig {
            enabled: false,
            dir: def_logs_dir(),
            file: "{repo}.{task}.log".into(),
            max_size: ByteSize(10 * 1024 * 1024),
            keep: 5,
            timestamps: true,
        }
    }
}

/// A size written the way people write one: `10MB`, `512k`, `1GiB`, `4096`.
/// The units are binary — a megabyte is 1024 kilobytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "SizeValue")]
pub struct ByteSize(pub u64);

#[derive(Deserialize)]
#[serde(untagged)]
enum SizeValue {
    Number(u64),
    Text(String),
}

impl TryFrom<SizeValue> for ByteSize {
    type Error = String;

    fn try_from(value: SizeValue) -> Result<Self, Self::Error> {
        match value {
            SizeValue::Number(n) => Ok(ByteSize(n)),
            SizeValue::Text(text) => ByteSize::parse(&text),
        }
    }
}

impl ByteSize {
    pub fn parse(text: &str) -> Result<ByteSize, String> {
        let text = text.trim();
        let split = text
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(text.len());
        let (digits, unit) = text.split_at(split);
        let number: u64 = digits
            .parse()
            .map_err(|_| format!("invalid size {text:?} — write it like 10MB or 512K"))?;
        let shift = match unit.trim().to_ascii_lowercase().as_str() {
            "" | "b" => 0,
            "k" | "kb" | "kib" => 10,
            "m" | "mb" | "mib" => 20,
            "g" | "gb" | "gib" => 30,
            other => return Err(format!("unknown size unit {other:?} in {text:?}")),
        };
        number
            .checked_mul(1 << shift)
            .map(ByteSize)
            .ok_or_else(|| format!("size {text:?} is too large"))
    }
}

impl std::fmt::Display for ByteSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (shift, unit) in [(30, "GB"), (20, "MB"), (10, "KB")] {
            if self.0 >= 1 << shift && self.0.is_multiple_of(1 << shift) {
                return write!(f, "{}{unit}", self.0 >> shift);
            }
        }
        write!(f, "{}B", self.0)
    }
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

fn def_local_dir() -> String {
    "local".into()
}

fn def_logs_dir() -> String {
    format!("{BRANCH_DIR}/logs")
}

/// The placeholder `logs.dir` may start with.
pub const BRANCH_DIR: &str = "{branch_dir}";

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
    /// Walk up from `start` looking for `ws.yaml`.
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

    /// Load `ws.yaml`, layered on top of the user's global configuration.
    ///
    /// Settings come from `~/.config/ws/config.yaml`; the workspace file lists
    /// the repositories and overrides whatever it needs to, key by key.
    pub fn load(root: &Path) -> Result<Workspace> {
        let path = root.join(CONFIG_FILE);
        let local = read_yaml(&path)?;
        let mut sources = Vec::new();

        let merged = match global_config_path().filter(|p| p.is_file()) {
            Some(global_path) => {
                let global = read_yaml(&global_path)?;
                // Deserialize the global file on its own first, so an unknown
                // or malformed key is reported against the file that holds it.
                let standalone: Config = from_value(global.clone())
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

        let config: Config =
            from_value(merged).with_context(|| format!("cannot parse {}", path.display()))?;
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

    /// Directory the `local` integration takes its files from.
    pub fn local_dir(&self) -> PathBuf {
        resolve_dir(&self.root, &self.config.integrations.local.dir)
    }

    /// Directory `ws run` writes the logs of `branch_dir` to.
    pub fn logs_dir(&self, branch_dir: &Path) -> PathBuf {
        match self.logs_in_branch_dir() {
            Some(relative) => branch_dir.join(relative),
            None => resolve_dir(&self.root, &self.config.logs.dir),
        }
    }

    /// The logs directory inside a worktree, which is where they are when the
    /// branch directory is the single repository's worktree.
    pub fn logs_in_worktree(&self) -> Option<PathBuf> {
        if self.is_grouped() {
            return None;
        }
        self.logs_in_branch_dir()
    }

    /// Where the logs sit inside each branch directory, when `logs.dir`
    /// puts them there: `logs` for the default `{branch_dir}/logs`.
    pub fn logs_in_branch_dir(&self) -> Option<PathBuf> {
        let dir = self.config.logs.dir.trim();
        let rest = dir.strip_prefix(BRANCH_DIR)?;
        Some(PathBuf::from(rest.trim_start_matches('/')))
    }

    pub fn state_path(&self) -> PathBuf {
        self.root.join(STATE_DIR).join("state.yaml")
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
                "{}: repo_layout: single needs exactly one repo, found {}",
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
        if self.integrations.zoxide.command.is_empty() {
            bail!(
                "{}: integrations.zoxide.command must not be empty",
                path.display()
            );
        }
        let local = &self.integrations.local;
        if local.dir.trim().is_empty() {
            bail!(
                "{}: integrations.local.dir must not be empty",
                path.display()
            );
        }
        for rule in &local.paths {
            if let Err(e) = rule.regex() {
                bail!(
                    "{}: integrations.local.paths: invalid pattern {:?}: {e}",
                    path.display(),
                    rule.pattern
                );
            }
        }
        for (name, tool) in [("just", &self.tasks.just), ("task", &self.tasks.task)] {
            if tool.command.as_ref().is_some_and(Vec::is_empty) {
                bail!("{}: tasks.{name}.command must not be empty", path.display());
            }
        }
        let logs = &self.logs;
        if logs.dir.trim().is_empty() || logs.file.trim().is_empty() {
            bail!(
                "{}: logs.dir and logs.file must not be empty",
                path.display()
            );
        }
        if logs.dir.contains(BRANCH_DIR) {
            let dir = logs.dir.trim();
            let rest = dir
                .strip_prefix(BRANCH_DIR)
                .filter(|rest| rest.starts_with('/'))
                .map(|rest| rest.trim_start_matches('/'))
                .filter(|rest| !rest.is_empty() && !rest.contains('{'));
            let Some(rest) = rest else {
                bail!(
                    "{}: logs.dir: {BRANCH_DIR} has to start it and be followed by a \
                     directory, like {BRANCH_DIR}/logs",
                    path.display()
                );
            };
            let rest = Path::new(rest);
            if rest
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                bail!(
                    "{}: logs.dir must stay inside the branch directory",
                    path.display()
                );
            }
            // In a branch directory of several repositories, the logs sit
            // beside them and must not be one of them, or inside one.
            let first = rest.components().next();
            if let Some(repo) = self
                .repos
                .iter()
                .find(|r| r.dir().components().next() == first)
            {
                bail!(
                    "{}: logs.dir {} would be inside or around repository {}",
                    path.display(),
                    logs.dir,
                    repo.name()
                );
            }
        }
        if Path::new(&logs.file).is_absolute()
            || Path::new(&logs.file)
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            bail!("{}: logs.file must stay inside logs.dir", path.display());
        }
        for (manager, settings) in self.integrations.package_managers() {
            if settings.command.as_ref().is_some_and(Vec::is_empty) {
                bail!(
                    "{}: integrations.{}.command must not be empty",
                    path.display(),
                    manager.name()
                );
            }
        }
        Ok(())
    }
}

/// Read a configuration file into an untyped document, so the global and the
/// workspace file can be merged before either is given a shape.
fn read_yaml(path: &Path) -> Result<Value> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    parse_yaml(&text).with_context(|| format!("cannot parse {}", path.display()))
}

/// Parse a configuration document. A file holding nothing but comments is an
/// empty mapping rather than YAML's null, which no struct would accept.
fn parse_yaml(text: &str) -> Result<Value> {
    let value: Value = serde_yaml_ng::from_str(text)?;
    Ok(match value {
        Value::Null => Value::Mapping(Mapping::new()),
        other => other,
    })
}

/// Give a parsed document the shape of a `Config`.
fn from_value(value: Value) -> Result<Config> {
    Ok(serde_yaml_ng::from_value(value)?)
}

/// Parse configuration text straight into a `Config` — what a single file,
/// unlayered, amounts to.
pub fn parse(text: &str) -> Result<Config> {
    from_value(parse_yaml(text)?)
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
    Some(base.join("ws").join("config.yaml"))
}

/// Lay `over` on top of `base`, one key at a time.
///
/// Mappings merge so that a workspace setting one key of `mux:` keeps the rest
/// of the global `mux:`. Everything else, sequences included, is replaced.
fn merge_into(base: Value, over: Value) -> Value {
    match (base, over) {
        (Value::Mapping(mut base), Value::Mapping(over)) => {
            for (key, value) in over {
                let merged = match base.remove(&key) {
                    Some(existing) => merge_into(existing, value),
                    None => value,
                };
                base.insert(key, merged);
            }
            Value::Mapping(base)
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

/// Double-quote a value that came from outside, so a name or a URL holding a
/// colon, a quote or a leading indicator cannot bend the document around it.
fn quoted(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// The `ws.yaml` written by `ws init` — repositories and little else.
///
/// Settings live in the global configuration; a workspace only writes down
/// what is actually its own, and overrides the rest key by key when it has to.
pub fn local_template(name: &str, repos: &[String]) -> String {
    let mut out = format!(
        "name: {}\n\
         \n\
         # Settings come from the global configuration, which `ws init --global`\n\
         # writes with every key documented. Override single keys or whole\n\
         # sections here, in any order.\n\
         # branch_dirs: nested\n\
         # worktree_dir: {}\n\
         # mux:\n\
         #   root_window: ws\n\n",
        quoted(name),
        quoted(&format!("~/trees/{name}")),
    );

    if repos.is_empty() {
        out.push_str(
            "# One entry per repository under `repos`.\n\
             # repos:\n\
             #   - name: api\n\
             #     url: \"git@github.com:org/api.git\"\n\
             #     path: services/api    # where it sits inside a branch directory\n\
             #     setup: cp .env.example .env   # once per new worktree, after the install\n\
             #     # or a script; {root} {branch} {branch_dir} {worktree} {repo}\n\
             #     # {env:NAME} {env:NAME:-default} — {{...}} escapes one\n\
             #     # setup: |\n\
             #     #   cp {root}/shared/.env .env\n\
             #     #   make codegen\n",
        );
    } else {
        out.push_str("repos:\n");
        for url in repos {
            out.push_str(&format!(
                "  - name: {}\n    url: {}\n    # setup: cp .env.example .env\n",
                quoted(&repo_name_from_url(url)),
                quoted(url),
            ));
        }
    }
    out
}

/// The commented settings file written by `ws init --global`. Hand-written on
/// purpose: serializing the struct would drop every comment.
pub fn global_template() -> String {
    "# ws — global configuration, applied to every workspace.\n\
     # A workspace's ws.yaml lists the repositories and overrides any key here.\n\
     \n\
     # Branch checked out by `ws fetch`.\n\
     default_branch: main\n\
     # What new branches fork from; omit to use each repo's own default branch.\n\
     # base_branch: main\n\
     \n\
     # auto — one repo -> the branch directory is the worktree;\n\
     #        several -> <branch>/<repo>/. Force with \"single\" or \"grouped\".\n\
     repo_layout: auto\n\
     # flat: feature/x -> feature-x   |   nested: feature/x -> feature/x\n\
     branch_dirs: flat\n\
     # Both are resolved per workspace: relative to its ws.yaml, or absolute\n\
     # with \"~\" expanded. An absolute worktree_dir here would put every\n\
     # workspace's branches in one directory, so set that one per workspace.\n\
     bare_dir: .bare\n\
     worktree_dir: .\n\
     \n\
     mux:\n\
     \x20 # auto | tmux | herdr | none\n\
     \x20 backend: auto\n\
     \x20 # placeholders: {workspace} {branch} {branch_raw}\n\
     \x20 session: \"{workspace}/{branch}\"\n\
     \x20 # shared window/pane for the branch directory; \"\" disables it\n\
     \x20 root_window: ws\n\
     \x20 # what to do with no multiplexer: print-path | nothing\n\
     \x20 on_none: print-path\n\
     \x20 tmux:\n\
     \x20   layout: windows\n\
     \x20   command: [tmux]\n\
     \x20 herdr:\n\
     \x20   # tabs: one tab per repository, plus one for the branch directory;\n\
     \x20   # panes: all of them side by side in a single tab.\n\
     \x20   layout: tabs\n\
     \x20   # Where `panes` splits; ignored by `tabs`.\n\
     \x20   split_direction: right\n\
     \x20   command: [herdr]\n\
     \x20   # command: [nix, run, \"nixpkgs#herdr\", --]\n\
     \n\
     ui:\n\
     \x20 # auto | popup | inline | none\n\
     \x20 picker: auto\n\
     \x20 picker_cmd: [fzf]\n\
     \x20 popup: { width: \"80%\", height: \"60%\" }\n\
     \n\
     # Every integration resolves \"auto\" against whether its program is on\n\
     # PATH; `false` opts out and `true` insists on it.\n\
     integrations:\n\
     \x20 zoxide:\n\
     \x20   enabled: auto\n\
     \x20   # worktrees | branch-dirs | both | none\n\
     \x20   register: both\n\
     \x20   command: [zoxide]\n\
     \x20 # Untracked files kept in `dir` (resolved like bare_dir) and placed\n\
     \x20 # into every worktree on switch: everything in it for a single repo,\n\
     \x20 # <dir>/<repo>/ for each repo of several. Each entry's relative path\n\
     \x20 # is matched against the `paths` rules in order — the whole path, as\n\
     \x20 # a regular expression — and the first match picks the mode:\n\
     \x20 #   link     a symlink, shared by every worktree\n\
     \x20 #   copy     an independent copy per worktree\n\
     \x20 #   hardlink shared like link, but a real file\n\
     \x20 # A path no rule matches is left out; a file the worktree already\n\
     \x20 # has is never replaced. `auto` means \"when dir exists\".\n\
     \x20 local:\n\
     \x20   enabled: auto\n\
     \x20   dir: local\n\
     \x20   paths:\n\
     \x20     # - { pattern: \"\\\\.env.*\", mode: copy }\n\
     \x20     - { pattern: \".*\", mode: link }\n\
     \x20 # The package managers pick themselves per repository, from the\n\
     \x20 # packageManager field of its package.json or from its lockfile, and\n\
     \x20 # install in every worktree they are created in. The install is held\n\
     \x20 # to the lockfile, so it is the same every time:\n\
     \x20 #   yarn install --immutable        (--frozen-lockfile on yarn 1.x)\n\
     \x20 #   pnpm install --frozen-lockfile\n\
     \x20 #   npm ci\n\
     \x20 #   uv sync\n\
     \x20 # Set `install` to replace those arguments entirely.\n\
     \x20 #\n\
     \x20 # `link` seeds a new worktree's node_modules by hard-linking the one\n\
     \x20 # from the branch you switched from, or from the default branch,\n\
     \x20 # before installing — an inode per file instead of a download.\n\
     \x20 yarn:\n\
     \x20   enabled: auto\n\
     \x20   command: [yarn]\n\
     \x20   # install: [install, --immutable]\n\
     \x20   link: true\n\
     \x20 pnpm:\n\
     \x20   enabled: auto\n\
     \x20   command: [pnpm]\n\
     \x20   # install: [install, --frozen-lockfile]\n\
     \x20   link: true\n\
     \x20 npm:\n\
     \x20   enabled: auto\n\
     \x20   command: [npm]\n\
     \x20   # `npm ci` deletes node_modules before installing, which is what\n\
     \x20   # makes it reproducible, so there is nothing to seed. For the\n\
     \x20   # speed instead, set both of these.\n\
     \x20   # install: [install]\n\
     \x20   link: false\n\
     \x20 # A virtualenv is not relocatable, so uv is never seeded — its own\n\
     \x20 # global cache already hard-links the packages into place.\n\
     \x20 uv:\n\
     \x20   enabled: auto\n\
     \x20   command: [uv]\n\
     \x20   # install: [sync]\n\
     \n\
     # `ws run <task>` runs a task in every repository of the branch directory\n\
     # that has it, side by side; `ws run @repo:task` in that one alone.\n\
     tasks:\n\
     \x20 # Where a task name is looked up, in order — the first source a\n\
     \x20 # repository has it in runs it:\n\
     \x20 #   just     a justfile, run with `just`\n\
     \x20 #   task     a Taskfile.yml, run with go-task's `task`\n\
     \x20 #   package  package.json scripts, run with the repository's own\n\
     \x20 #            yarn, pnpm or npm\n\
     \x20 order: [just, task, package]\n\
     \x20 just:\n\
     \x20   enabled: auto\n\
     \x20   command: [just]\n\
     \x20 task:\n\
     \x20   enabled: auto\n\
     \x20   command: [task]\n\
     \x20 # `auto` means \"when the repository has a package.json\".\n\
     \x20 package:\n\
     \x20   enabled: auto\n\
     \x20 # Coloured prefixes, and FORCE_COLOR for the tasks so they keep\n\
     \x20 # theirs through the pipe: auto | always | never\n\
     \x20 color: auto\n\
     \n\
     # Everything `ws run` prints, also written to files — without colours.\n\
     logs:\n\
     \x20 enabled: false\n\
     \x20 # {branch_dir} is the branch directory the tasks run in, and `ws rm`\n\
     \x20 # takes its logs along; anything else is resolved like bare_dir.\n\
     \x20 dir: \"{branch_dir}/logs\"\n\
     \x20 # Inside dir; placeholders {branch} {repo} {task}. Tasks that land in\n\
     \x20 # the same file share it, each line marked with its task.\n\
     \x20 file: \"{repo}.{task}.log\"\n\
     \x20 # A file past max_size moves to <file>.1, the one before to .2, and\n\
     \x20 # so on; `keep` of them are kept. 10MB, 512K, or bytes; 0 never rotates.\n\
     \x20 max_size: 10MB\n\
     \x20 keep: 5\n\
     \x20 # Start every line with the local time it was written.\n\
     \x20 timestamps: true\n"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_fill_in() {
        let cfg = parse("").unwrap();
        assert_eq!(cfg.default_branch, "main");
        assert_eq!(cfg.bare_dir, ".bare");
        assert_eq!(cfg.branch_dirs, BranchDirs::Flat);
        assert_eq!(cfg.repo_layout, RepoLayout::Auto);
        assert_eq!(cfg.mux.session, "{workspace}/{branch}");
        assert_eq!(cfg.mux.tmux.command, ["tmux"]);
        assert_eq!(cfg.ui.picker, PickerMode::Auto);
        assert_eq!(cfg.integrations.zoxide.register, Register::Both);
        assert_eq!(
            cfg.integrations.zoxide.enabled,
            Toggle::Named(NamedToggle::Auto)
        );
        for (manager, settings) in cfg.integrations.package_managers() {
            assert_eq!(settings.enabled, Toggle::Named(NamedToggle::Auto));
            assert_eq!(settings.command(manager), [manager.name()]);
        }
    }

    #[test]
    fn package_manager_defaults_come_from_the_manager_not_the_file() {
        // A workspace writes down only what it wants to differ, so a single
        // key has to leave the rest of the manager's own defaults standing.
        let cfg = parse("integrations:\n  pnpm:\n    enabled: false\n").unwrap();
        let pnpm = &cfg.integrations.pnpm;
        assert_eq!(pnpm.enabled, Toggle::Bool(false));
        assert_eq!(pnpm.command(Manager::Pnpm), ["pnpm"]);
        assert!(pnpm.install.is_none());
        assert!(pnpm.link(Manager::Pnpm));

        // `npm ci` throws a seeded node_modules away, so npm does not seed one
        // unless it is asked to; a virtualenv cannot be seeded at all.
        assert!(!cfg.integrations.npm.link(Manager::Npm));
        let cfg = parse("integrations:\n  npm:\n    link: true\n  uv:\n    link: true\n").unwrap();
        assert!(cfg.integrations.npm.link(Manager::Npm));
        assert!(!cfg.integrations.uv.link(Manager::Uv));
    }

    #[test]
    fn local_defaults_to_linking_everything_from_local() {
        let cfg = parse("").unwrap();
        let local = &cfg.integrations.local;
        assert_eq!(local.enabled, Toggle::Named(NamedToggle::Auto));
        assert_eq!(local.dir, "local");
        assert_eq!(local.paths.len(), 1);
        assert_eq!(local.paths[0].pattern, ".*");
        assert_eq!(local.paths[0].mode, LocalMode::Link);

        // A rule that names only its pattern links.
        let cfg = parse("integrations:\n  local:\n    paths:\n      - pattern: x\n").unwrap();
        assert_eq!(cfg.integrations.local.paths[0].mode, LocalMode::Link);
    }

    #[test]
    fn an_invalid_local_pattern_is_rejected_at_load() {
        let cfg = parse("integrations:\n  local:\n    paths:\n      - pattern: \"(\"\n").unwrap();
        assert!(cfg.validate(Path::new(CONFIG_FILE)).is_err());
    }

    #[test]
    fn an_empty_launcher_is_rejected() {
        let cfg = parse("integrations:\n  yarn:\n    command: []\n").unwrap();
        assert!(cfg.validate(Path::new(CONFIG_FILE)).is_err());
    }

    #[test]
    fn a_file_of_nothing_but_comments_is_an_empty_config() {
        // YAML reads a document without a node as null, which no struct would
        // accept — a workspace file may well be all comments.
        let cfg = parse("# nothing here yet\n").unwrap();
        assert_eq!(cfg.default_branch, "main");
        assert!(cfg.repos.is_empty());
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
        let cfg = parse(
            r#"
            repos:
              - url: x/a.git
                setup: pnpm i
              - url: x/b.git
                setup: [pnpm i, cp .env.example .env]
            "#,
        )
        .unwrap();
        assert_eq!(cfg.repos[0].setup_commands(), ["pnpm i"]);
        assert_eq!(cfg.repos[1].setup_commands().len(), 2);
    }

    #[test]
    fn zoxide_enabled_accepts_bool_and_auto() {
        let cfg = parse("integrations:\n  zoxide:\n    enabled: false\n").unwrap();
        assert_eq!(cfg.integrations.zoxide.enabled, Toggle::Bool(false));
        assert!(!cfg.integrations.zoxide.enabled.resolve(true));
        assert!(Toggle::default().resolve(true));
        assert!(!Toggle::default().resolve(false));
    }

    #[test]
    fn duplicate_repo_names_are_rejected() {
        let cfg = parse("repos:\n  - url: a/api.git\n  - url: b/api.git\n").unwrap();
        assert!(cfg.validate(Path::new(CONFIG_FILE)).is_err());
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
        let cfg = parse(
            r#"
            repos:
              - url: x/api.git
              - name: web
                url: x/web.git
                path: services/web
            "#,
        )
        .unwrap();
        assert_eq!(cfg.repos[0].dir(), PathBuf::from("api"));
        assert_eq!(cfg.repos[1].dir(), PathBuf::from("services/web"));
        cfg.validate(Path::new(CONFIG_FILE)).unwrap();
    }

    #[test]
    fn a_repo_path_may_not_escape_the_branch_directory() {
        for bad in ["/abs", "../up", "a/../../b", ""] {
            let cfg = parse(&format!(
                "repos:\n  - name: api\n    url: x/api.git\n    path: {}\n",
                quoted(bad)
            ))
            .unwrap();
            assert!(
                cfg.validate(Path::new(CONFIG_FILE)).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn sizes_read_the_way_people_write_them() {
        for (text, want) in [
            ("4096", 4096),
            ("512K", 512 * 1024),
            ("10MB", 10 << 20),
            ("10 mb", 10 << 20),
            ("1GiB", 1 << 30),
            ("0", 0),
        ] {
            assert_eq!(ByteSize::parse(text), Ok(ByteSize(want)), "{text}");
        }
        for bad in ["", "MB", "10XB", "-1"] {
            assert!(ByteSize::parse(bad).is_err(), "{bad:?} should be rejected");
        }
        let cfg = parse("logs:\n  max_size: 2048\n").unwrap();
        assert_eq!(cfg.logs.max_size, ByteSize(2048));
        assert!(parse("logs:\n  max_size: lots\n").is_err());
        assert_eq!(ByteSize(10 << 20).to_string(), "10MB");
        assert_eq!(ByteSize(1500).to_string(), "1500B");
    }

    #[test]
    fn logs_default_to_the_branch_directory() {
        let ws = Workspace {
            root: PathBuf::from("/w"),
            config: parse("").unwrap(),
            sources: Vec::new(),
        };
        assert_eq!(ws.logs_in_branch_dir(), Some(PathBuf::from("logs")));
        assert_eq!(
            ws.logs_dir(Path::new("/w/main")),
            PathBuf::from("/w/main/logs")
        );
        let ws = Workspace {
            config: parse("logs:\n  dir: logs\n").unwrap(),
            ..ws
        };
        assert_eq!(ws.logs_in_branch_dir(), None);
        assert_eq!(ws.logs_dir(Path::new("/w/main")), PathBuf::from("/w/logs"));
    }

    #[test]
    fn a_branch_logs_directory_stays_out_of_the_repositories() {
        for bad in [
            "{branch_dir}",
            "{branch_dir}/",
            "{branch_dir}/../x",
            "x/{branch_dir}/logs",
            "{branch_dir}/api",
            "{branch_dir}/api/logs",
            "{branch_dir}/services",
        ] {
            let cfg = parse(&format!(
                "repos:\n  - url: x/api.git\n  - url: x/web.git\n    path: services/web\n\
                 logs:\n  dir: {}\n",
                quoted(bad)
            ))
            .unwrap();
            assert!(cfg.validate(Path::new(CONFIG_FILE)).is_err(), "{bad}");
        }
        let cfg =
            parse("repos:\n  - url: x/api.git\nlogs:\n  dir: \"{branch_dir}/.logs\"\n").unwrap();
        cfg.validate(Path::new(CONFIG_FILE)).unwrap();
    }

    #[test]
    fn a_log_file_may_not_leave_the_logs_directory() {
        for bad in ["/tmp/x.log", "../x.log", "{branch}/../../x.log"] {
            let cfg = parse(&format!("logs:\n  file: {}\n", quoted(bad))).unwrap();
            assert!(cfg.validate(Path::new(CONFIG_FILE)).is_err(), "{bad}");
        }
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(parse("nope: 1\n").is_err());
    }

    #[test]
    fn both_templates_parse_back() {
        let local = local_template("demo", &["git@github.com:org/api.git".into()]);
        let cfg = parse(&local).unwrap();
        assert_eq!(cfg.name.as_deref(), Some("demo"));
        assert_eq!(cfg.repos.len(), 1);
        assert_eq!(cfg.repos[0].name(), "api");
        assert_eq!(cfg.repos[0].url, "git@github.com:org/api.git");
        cfg.validate(Path::new(CONFIG_FILE)).unwrap();

        let global = parse(&global_template()).unwrap();
        assert!(global.repos.is_empty(), "the global template has no repos");
        global.validate(Path::new("config.yaml")).unwrap();
    }

    #[test]
    fn the_global_template_writes_down_the_defaults_it_documents() {
        // The file claims to show every key with its default filled in; if a
        // default moves, the template has to move with it.
        let global = parse(&global_template()).unwrap();
        let defaults = Config::default();
        assert_eq!(global.default_branch, defaults.default_branch);
        assert_eq!(global.repo_layout, defaults.repo_layout);
        assert_eq!(global.branch_dirs, defaults.branch_dirs);
        assert_eq!(global.bare_dir, defaults.bare_dir);
        assert_eq!(global.worktree_dir, defaults.worktree_dir);
        assert_eq!(global.mux.backend, defaults.mux.backend);
        assert_eq!(global.mux.session, defaults.mux.session);
        assert_eq!(global.mux.root_window, defaults.mux.root_window);
        assert_eq!(global.mux.on_none, defaults.mux.on_none);
        assert_eq!(global.mux.tmux.layout, defaults.mux.tmux.layout);
        assert_eq!(global.mux.tmux.command, defaults.mux.tmux.command);
        assert_eq!(global.mux.herdr.layout, defaults.mux.herdr.layout);
        assert_eq!(
            global.mux.herdr.split_direction,
            defaults.mux.herdr.split_direction
        );
        assert_eq!(global.mux.herdr.command, defaults.mux.herdr.command);
        assert_eq!(global.ui.picker, defaults.ui.picker);
        assert_eq!(global.ui.picker_cmd, defaults.ui.picker_cmd);
        assert_eq!(global.ui.popup.width, defaults.ui.popup.width);
        assert_eq!(global.ui.popup.height, defaults.ui.popup.height);

        assert_eq!(global.tasks.order, defaults.tasks.order);
        assert_eq!(global.tasks.just.enabled, defaults.tasks.just.enabled);
        assert_eq!(global.tasks.task.enabled, defaults.tasks.task.enabled);
        assert_eq!(global.tasks.package.enabled, defaults.tasks.package.enabled);
        assert_eq!(global.tasks.color, defaults.tasks.color);
        assert_eq!(global.logs.enabled, defaults.logs.enabled);
        assert_eq!(global.logs.dir, defaults.logs.dir);
        assert_eq!(global.logs.file, defaults.logs.file);
        assert_eq!(global.logs.max_size, defaults.logs.max_size);
        assert_eq!(global.logs.keep, defaults.logs.keep);
        assert_eq!(global.logs.timestamps, defaults.logs.timestamps);

        let (written, fallback) = (&global.integrations, &defaults.integrations);
        assert_eq!(written.zoxide.enabled, fallback.zoxide.enabled);
        assert_eq!(written.zoxide.register, fallback.zoxide.register);
        assert_eq!(written.zoxide.command, fallback.zoxide.command);
        assert_eq!(written.local.enabled, fallback.local.enabled);
        assert_eq!(written.local.dir, fallback.local.dir);
        let rules = |l: &LocalConfig| -> Vec<(String, LocalMode)> {
            l.paths
                .iter()
                .map(|p| (p.pattern.clone(), p.mode))
                .collect()
        };
        assert_eq!(rules(&written.local), rules(&fallback.local));
        for ((manager, written), (_, fallback)) in written
            .package_managers()
            .into_iter()
            .zip(fallback.package_managers())
        {
            assert_eq!(written.enabled, fallback.enabled, "{}", manager.name());
            assert_eq!(
                written.command(manager),
                fallback.command(manager),
                "{}",
                manager.name()
            );
            assert_eq!(written.install, fallback.install, "{}", manager.name());
            assert_eq!(
                written.link(manager),
                fallback.link(manager),
                "{}",
                manager.name()
            );
        }
    }

    #[test]
    fn uncommenting_the_local_templates_hints_produces_valid_config() {
        // Every commented example is offered as something you can simply
        // uncomment, so each one has to stand on its own — at the indentation
        // the template put it at, which is what carries the nesting in YAML.
        for repos in [vec![], vec!["git@github.com:org/api.git".to_string()]] {
            let text = local_template("demo", &repos);
            let uncommented: String = text
                .lines()
                .map(|line| {
                    let indent = &line[..line.len() - line.trim_start().len()];
                    match line.trim_start().strip_prefix("# ") {
                        // A commented example is a mapping key or a sequence
                        // item; anything else is prose and stays a comment.
                        Some(rest)
                            if rest.contains(": ")
                                || rest.ends_with(':')
                                || rest.starts_with("- ") =>
                        {
                            format!("{indent}{rest}")
                        }
                        _ => line.to_string(),
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            let cfg =
                parse(&uncommented).unwrap_or_else(|e| panic!("{e}\n--- in ---\n{uncommented}"));
            assert_eq!(cfg.branch_dirs, BranchDirs::Nested);
            assert_eq!(cfg.mux.root_window, "ws");
            assert_eq!(cfg.repos.len(), 1);
            assert_eq!(cfg.repos[0].setup_commands(), ["cp .env.example .env"]);
            cfg.validate(Path::new(CONFIG_FILE)).unwrap();
        }
    }

    #[test]
    fn the_local_template_carries_only_identity_and_repos() {
        let text = local_template("demo", &["git@github.com:org/api.git".into()]);
        let value = parse_yaml(&text).unwrap();
        let keys: Vec<&str> = value
            .as_mapping()
            .unwrap()
            .keys()
            .map(|k| k.as_str().unwrap())
            .collect();
        assert_eq!(keys, ["name", "repos"]);
    }

    #[test]
    fn a_name_that_needs_quoting_survives_the_template() {
        let text = local_template(r#"we: "ird""#, &[]);
        let cfg = parse(&text).unwrap();
        assert_eq!(cfg.name.as_deref(), Some(r#"we: "ird""#));
    }

    #[test]
    fn merging_replaces_keys_and_keeps_the_rest_of_a_table() {
        let global = parse_yaml(
            "branch_dirs: nested\n\
             mux:\n  backend: herdr\n  root_window: ws\n\
             ui:\n  picker_cmd: [fzf, --border]\n",
        )
        .unwrap();
        let local = parse_yaml(
            "name: acme\n\
             mux:\n  root_window: acme\n\
             ui:\n  picker_cmd: [sk]\n",
        )
        .unwrap();

        let cfg = from_value(merge_into(global, local)).unwrap();
        assert_eq!(cfg.name.as_deref(), Some("acme"));
        // untouched by the workspace, so the global value survives
        assert_eq!(cfg.branch_dirs, BranchDirs::Nested);
        assert_eq!(cfg.mux.backend, Backend::Herdr);
        // overridden key by key, not section by section
        assert_eq!(cfg.mux.root_window, "acme");
        // sequences are replaced whole, never appended to
        assert_eq!(cfg.ui.picker_cmd, ["sk"]);
    }

    #[test]
    fn merging_an_empty_workspace_file_changes_nothing() {
        let global = parse_yaml("branch_dirs: nested\n").unwrap();
        let local = parse_yaml("").unwrap();
        let cfg = from_value(merge_into(global, local)).unwrap();
        assert_eq!(cfg.branch_dirs, BranchDirs::Nested);
    }
}
