//! Command line surface.

use clap::{Parser, Subcommand, ValueEnum};

use crate::config::Backend;

#[derive(Debug, Parser)]
#[command(
    name = "ws",
    version,
    about = "Multi-repo git worktree workspaces",
    long_about = "Create one worktree per repository under a shared branch directory, \
                  run per-repo setup, and lay the result out in tmux or herdr.",
    subcommand_negates_reqs = true
)]
pub struct Cli {
    /// Print the git/tmux/herdr commands instead of running the mutating ones.
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Echo every command as it runs.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Override the multiplexer backend from the config.
    #[arg(long, global = true, value_enum)]
    pub mux: Option<BackendArg>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum BackendArg {
    Auto,
    Tmux,
    Herdr,
    None,
}

impl From<BackendArg> for Backend {
    fn from(value: BackendArg) -> Backend {
        match value {
            BackendArg::Auto => Backend::Auto,
            BackendArg::Tmux => Backend::Tmux,
            BackendArg::Herdr => Backend::Herdr,
            BackendArg::None => Backend::None,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Write a ws.toml in the current directory.
    Init {
        /// Workspace name; defaults to the directory name.
        #[arg(long)]
        name: Option<String>,
        /// Repository URL; repeat for several repositories.
        #[arg(long = "repo", value_name = "URL")]
        repos: Vec<String>,
        /// Overwrite an existing ws.toml.
        #[arg(long)]
        force: bool,
    },

    /// Clone missing bare repositories and fetch all of them.
    Fetch,

    /// Check out a branch in every repository and open the session.
    #[command(alias = "sw")]
    Switch {
        /// Branch name; omit to pick one interactively.
        branch: Option<String>,
        /// Run each repo's setup command again.
        #[arg(long)]
        rerun_setup: bool,
        /// Skip setup commands entirely.
        #[arg(long)]
        no_setup: bool,
        /// Print the branch directory and skip the multiplexer.
        #[arg(long)]
        cd: bool,
    },

    /// Pick a branch or another workspace with fzf.
    #[command(alias = "p")]
    Pick {
        /// Include workspaces from the zoxide database.
        #[arg(long)]
        global: bool,
        /// Run the filter here instead of opening a popup.
        #[arg(long)]
        inline: bool,
        /// Extra keywords passed to `zoxide query` with --global.
        #[arg(value_name = "QUERY")]
        query: Vec<String>,
    },

    /// Show the branch directories of this workspace.
    #[command(alias = "ls")]
    List,

    /// Print the path of a branch directory or one repo's worktree.
    Path {
        branch: String,
        /// Repository name; defaults to the branch directory.
        repo: Option<String>,
    },

    /// Remove the worktrees of a branch and close its session.
    #[command(alias = "remove")]
    Rm {
        branch: String,
        /// Remove even with uncommitted changes.
        #[arg(short, long)]
        force: bool,
    },

    /// Show the workspace and branch for the current directory.
    Status,

    /// Print a shell function that makes `ws switch` change directory.
    ShellInit { shell: ShellArg },

    /// Preview text for the fzf picker.
    #[command(hide = true, name = "__preview")]
    Preview { payload: String },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ShellArg {
    Zsh,
    Bash,
    Fish,
}

/// Shell wrapper emitted by `ws shell-init`.
pub fn shell_init(shell: ShellArg) -> &'static str {
    match shell {
        ShellArg::Zsh | ShellArg::Bash => {
            r#"# ws shell integration — add to your rc file:
#   eval "$(ws shell-init zsh)"
# `ws switch` then changes the shell's directory; every other subcommand is
# passed straight through.
ws() {
  case "$1" in
    switch|sw)
      local __ws_dir
      __ws_dir="$(command ws "$@" --cd)" || return $?
      [ -n "$__ws_dir" ] && cd "$__ws_dir"
      ;;
    *)
      command ws "$@"
      ;;
  esac
}
"#
        }
        ShellArg::Fish => {
            r#"# ws shell integration — add to config.fish:
#   ws shell-init fish | source
function ws
    switch "$argv[1]"
        case switch sw
            set -l __ws_dir (command ws $argv --cd)
            or return $status
            test -n "$__ws_dir"; and cd "$__ws_dir"
        case '*'
            command ws $argv
    end
end
"#
        }
    }
}
