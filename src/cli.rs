//! Command line surface.

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

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
    /// Write a ws.yaml in the current directory.
    Init {
        /// Workspace name; defaults to the directory name.
        #[arg(long)]
        name: Option<String>,
        /// Repository URL; repeat for several repositories.
        #[arg(long = "repo", value_name = "URL")]
        repos: Vec<String>,
        /// Write the documented global settings file instead, at
        /// ~/.config/ws/config.yaml.
        #[arg(long, conflicts_with_all = ["name", "repos"])]
        global: bool,
        /// Overwrite an existing file.
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
        /// Skip setup commands and the package manager installs.
        #[arg(long)]
        no_setup: bool,
        /// Base for a branch that does not exist yet: a branch, a tag or any
        /// other ref, with or without the `origin/` prefix. Overrides the
        /// configured base_branch.
        #[arg(long, value_name = "REF")]
        from: Option<String>,
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

    /// Run a task from the repositories' justfile, Taskfile or package.json.
    #[command(alias = "r")]
    Run {
        /// `task` runs it in every repository that has it, side by side;
        /// `@repo:task` in that repository only; `@repo` lists its tasks.
        /// Omit to list every task.
        #[arg(value_name = "TASK")]
        targets: Vec<String>,
        /// Branch directory to run in; defaults to the one you are in, then
        /// the default branch.
        #[arg(short, long)]
        branch: Option<String>,
        /// Write the output to the log files, whatever `logs.enabled` says.
        #[arg(long, conflicts_with = "no_logs")]
        logs: bool,
        /// Keep the output out of the log files.
        #[arg(long)]
        no_logs: bool,
        /// Arguments passed on to the task, after `--`.
        #[arg(last = true, value_name = "ARGS")]
        args: Vec<String>,
    },

    /// Show the workspace and branch for the current directory.
    Status,

    /// Print a shell function that makes `ws switch` change directory.
    ShellInit { shell: ShellArg },

    /// Print the completion script for a shell.
    Completions { shell: ShellArg },

    /// Preview text for the fzf picker.
    #[command(hide = true, name = "__preview")]
    Preview { payload: String },

    /// Candidates for the shell completions, one per line, each a value and
    /// an optional description separated by a tab.
    #[command(hide = true, name = "__complete")]
    Complete {
        what: CompleteArg,
        /// The word being completed, when the candidates depend on it.
        word: Option<String>,
    },
}

/// What `ws __complete` is being asked for.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CompleteArg {
    /// The branches this workspace has worktrees for.
    Worktrees,
    /// Every branch of the workspace, worktree or not.
    Branches,
    /// What `--from` takes: the branches and the tags.
    Refs,
    /// What `ws run` takes: task names, or `@repo:task` once `@` is typed.
    Tasks,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ShellArg {
    Zsh,
    Bash,
    Fish,
}

impl From<ShellArg> for Shell {
    fn from(value: ShellArg) -> Shell {
        match value {
            ShellArg::Zsh => Shell::Zsh,
            ShellArg::Bash => Shell::Bash,
            ShellArg::Fish => Shell::Fish,
        }
    }
}

/// Completion script emitted by `ws completions`.
///
/// The script completes the command name `ws`, which is also what the
/// `ws shell-init` wrapper function is called, so the two integrations stack.
pub fn completions(shell: ShellArg, out: &mut dyn std::io::Write) {
    // The generators ignore `hide`, so the command is rebuilt without the
    // hidden `__preview` rather than offering it as a candidate.
    let full = Cli::command();
    let mut command = clap::Command::new("ws")
        .version(env!("CARGO_PKG_VERSION"))
        .about(full.get_about().cloned().unwrap_or_default())
        .long_about(full.get_long_about().cloned().unwrap_or_default())
        .args(full.get_arguments().cloned())
        .subcommands(full.get_subcommands().filter(|c| !c.is_hide_set()).cloned());
    let mut script = Vec::new();
    clap_complete::generate(Shell::from(shell), &mut command, "ws", &mut script);
    let mut script = String::from_utf8(script).expect("the generators write utf-8");
    command.build();
    match shell {
        ShellArg::Bash => script.push_str(&bash_addendum(&command)),
        ShellArg::Zsh => script = zsh_addendum(script),
        ShellArg::Fish => script.push_str(FISH_BRANCHES),
    }
    let _ = out.write_all(script.as_bytes());
}

/// Subcommands whose branch argument names a worktree that already exists.
const WORKTREE_ARG: &[&str] = &["rm", "remove", "path"];

/// Subcommands whose branch argument may also be one that does not exist yet,
/// and so is completed from every branch of the workspace.
const BRANCH_ARG: &[&str] = &["switch", "sw"];

/// Subcommands that take task names.
const RUN_ARG: &[&str] = &["run", "r"];

/// zsh is handed the branch positional as `:branch:_default`, which completes
/// file names. The two required ones — `ws rm` and `ws path` — are rewritten
/// to ask `ws` instead; `ws switch` has an optional branch, written
/// `::branch -- ...`, and is left alone.
fn zsh_addendum(script: String) -> String {
    let hooked: Vec<String> = script
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            // `ws rm` and `ws path` take a worktree that exists; the optional
            // branch of `ws switch`, written `::branch -- <help>`, may be a
            // new one. Both end in the `_default` action, which is files.
            if trimmed.starts_with("':branch:") {
                line.replace(":_default'", ":_ws_worktrees'")
            } else if trimmed.starts_with("'::branch ") {
                line.replace(":_default'", ":_ws_branches'")
            } else if trimmed.starts_with("'--from=") {
                line.replace(":REF:_default'", ":REF:_ws_refs'")
            } else if trimmed.starts_with("'*::targets ") {
                line.replace(":_default'", ":_ws_tasks'")
            } else if trimmed.starts_with("'-b+[Branch directory")
                || trimmed.starts_with("'--branch=[Branch directory")
            {
                line.replace(":BRANCH:_default'", ":BRANCH:_ws_worktrees'")
            } else {
                line.to_string()
            }
        })
        .collect();
    let hooked = hooked.join("\n");
    // The helpers go in before the generated script's last lines, which call
    // `_ws` straight away when the file is autoloaded from `fpath` — the first
    // <Tab> would otherwise ask for helpers that are not defined yet. Only an
    // `eval` of the script, which reaches `compdef` instead, got away with
    // them at the end.
    let (head, tail) = match hooked.find(ZSH_DISPATCH) {
        Some(at) => hooked.split_at(at),
        None => (hooked.as_str(), ""),
    };
    format!(
        r#"{head}
_ws_worktrees() {{
    _ws_candidates worktrees worktree
}}

_ws_branches() {{
    _ws_candidates branches branch
}}

_ws_refs() {{
    # `origin/release/x` is one word to the shell but a path to the completion
    # system; -S '' keeps the slashes out of its hands.
    _ws_candidates refs ref
}}

_ws_tasks() {{
    # `@repo:task` holds a colon, which _describe would take for the start of
    # the description.
    local -a candidates
    local line name
    for line in ${{(f)"$(command ws __complete tasks ${{(q)PREFIX}} 2>/dev/null)"}}; do
        [[ -n "$line" ]] || continue
        name="${{line%%$'\t'*}}"
        candidates+=("${{name//:/\\:}}:${{line#*$'\t'}}")
    done
    (( ${{#candidates}} )) || return 1
    _describe -t tasks task candidates
}}

_ws_candidates() {{
    local -a candidates
    local line
    for line in ${{(f)"$(command ws __complete $1 ${{(q)PREFIX}} 2>/dev/null)"}}; do
        [[ -n "$line" ]] && candidates+=("${{line/$'\t'/:}}")
    done
    (( ${{#candidates}} )) || return 1
    _describe -t "$2s" "$2" candidates
}}

{tail}"#
    )
}

/// Where clap's zsh script starts dispatching: to `_ws` itself when the file
/// was autoloaded as the completion function, to `compdef` when it was
/// sourced or `eval`ed.
const ZSH_DISPATCH: &str = "if [ \"$funcstack[1]\" = \"_ws\" ]; then";

/// fish reads `value<tab>description` straight out of a command.
const FISH_BRANCHES: &str = r#"
complete -c ws -n "__fish_ws_using_subcommand rm" -f -a "(ws __complete worktrees)"
complete -c ws -n "__fish_ws_using_subcommand path" -f -a "(ws __complete worktrees)"
complete -c ws -n "__fish_ws_using_subcommand switch" -f -a "(ws __complete branches)"
complete -c ws -n "__fish_ws_using_subcommand switch" -l from -r -f \
    -a "(ws __complete refs (commandline --current-token))"
complete -c ws -n "__fish_ws_using_subcommand run" -f \
    -a "(ws __complete tasks (commandline --current-token))"
complete -c ws -n "__fish_ws_using_subcommand run" -s b -l branch -r -f \
    -a "(ws __complete worktrees)"
"#;

/// What bash cannot do by itself, bolted onto the generated script.
///
/// zsh and fish are told about a candidate and its description separately, so
/// they list the two side by side and insert only the candidate. bash has one
/// array for both jobs: whatever `COMPREPLY` holds is what is shown *and* what
/// is typed onto the line. The way around it, which is what cobra's bash
/// completion does too, is to decorate the candidates only in the cases where
/// bash is about to list them rather than insert one.
///
/// The same wrapper drops the options unless the word being completed starts
/// with a dash — `ws <Tab>` otherwise buries ten subcommands under seven
/// flags, where zsh and fish show the subcommands alone.
///
/// The last `complete` for a command is the one that counts, so re-registering
/// here replaces what the generated script set up.
fn bash_addendum(command: &clap::Command) -> String {
    let mut arms = String::new();
    let mut value_arms = String::new();
    arms.push_str(&describe_arms(&[""], command));
    value_arms.push_str(&value_arms_for(&[""], command));
    for sub in command.get_subcommands() {
        // `ws sw --<Tab>` is `ws switch --<Tab>`: the aliases name the same
        // context, even though they are not candidates themselves.
        let mut context = vec![sub.get_name()];
        context.extend(sub.get_all_aliases());
        arms.push_str(&describe_arms(&context, sub));
        value_arms.push_str(&value_arms_for(&context, sub));
    }
    let branch_arg = BRANCH_ARG.join(" | ");
    let worktree_arg = WORKTREE_ARG.join(" | ");
    let run_arg = format!("@({})", RUN_ARG.join("|"));
    format!(
        r#"
# Descriptions, by the subcommand already on the line and the candidate.
_ws_describe() {{
    case "$1" in
{arms}    esac
}}

# Options that swallow the next word, so it is not mistaken for an argument.
_ws_takes_value() {{
    case "$1" in
{value_arms}    esac
    return 1
}}

# The subcommand already on the line, as the tables above key them.
_ws_context() {{
    local skip= word i
    for (( i = 1; i < COMP_CWORD; i++ )); do
        word="${{COMP_WORDS[i]}}"
        if [[ -n "$skip" ]]; then
            skip=
            continue
        fi
        if [[ "$word" == -* ]]; then
            # Only the global options can stand before the subcommand.
            [[ "$word" != *=* ]] && _ws_takes_value "" "$word" && skip=yes
            continue
        fi
        printf '%s' "$word"
        return 0
    done
}}

# Which argument of the subcommand the cursor sits on: 1 is the first.
_ws_argument_index() {{
    local index=0 skip= word i
    for (( i = 1; i < COMP_CWORD; i++ )); do
        word="${{COMP_WORDS[i]}}"
        if [[ -n "$skip" ]]; then
            skip=
            continue
        fi
        if [[ "$word" == -* ]]; then
            [[ "$word" != *=* ]] && _ws_takes_value "$1" "$word" && skip=yes
            continue
        fi
        (( index++ ))
    done
    # The subcommand itself is the word this counted first.
    printf '%s' "$index"
}}

# Candidates with the repositories as their description: the worktrees that
# exist, every branch of the workspace, or what `--from` takes. `ws` prints
# nothing outside a workspace.
_ws_candidates() {{
    local what="$1" word="$2" strip="${{3:-}}" line name description
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        name="${{line%%$'\t'*}}"
        description="${{line#*$'\t'}}"
        [[ "$name" == "$word"* ]] || continue
        name="${{name#"$strip"}}"
        [[ -n "$description" && "$description" != "$name" ]] &&
            _ws_branch_description["$name"]="$description"
        COMPREPLY+=("$name")
    done < <(command ws __complete "$what" "$word" 2>/dev/null)
}}

_ws_prefer_commands() {{
    _ws "$@"

    local cur="${{COMP_WORDS[COMP_CWORD]}}"
    local context
    context="$(_ws_context)"

    # A branch argument is something only `ws` knows about: the worktrees that
    # exist for `rm` and `path`, every branch of the workspace for `switch`.
    # Associative arrays want bash 4; without one the branches still complete,
    # just without their repositories.
    unset _ws_branch_description
    declare -gA _ws_branch_description=() 2> /dev/null
    local candidates= prev="${{COMP_WORDS[COMP_CWORD - 1]}}"
    if [[ "$prev" == "--from" ]]; then
        # The value of --from: a branch, a tag, or an `origin/` form once that
        # prefix is typed, which is why `ws` is told the word.
        candidates=refs
    else
        case "$context" in
            {worktree_arg}) candidates=worktrees ;;
            {branch_arg}) candidates=branches ;;
        esac
        [[ "$cur" == -* || "$(_ws_argument_index "$context")" != 1 ]] && candidates=
    fi
    if [[ -n "$candidates" ]]; then
        COMPREPLY=()
        _ws_candidates "$candidates" "$cur"
    fi

    # `ws run`: task names, and `@repo:task`. bash splits words at a colon,
    # so the word is read back from the line, and only what follows its last
    # colon is offered — that is the part bash replaces.
    if [[ "$context" == {run_arg} ]]; then
        local typed="${{COMP_LINE:0:COMP_POINT}}" word i after=
        typed="${{typed##*[[:space:]]}}"
        for (( i = 1; i < COMP_CWORD; i++ )); do
            [[ "${{COMP_WORDS[i]}}" == "--" ]] && after=yes
        done
        if [[ "$prev" == "-b" || "$prev" == "--branch" ]]; then
            COMPREPLY=()
            _ws_candidates worktrees "$cur"
        elif [[ -z "$after" && "$typed" != -* ]]; then
            COMPREPLY=()
            local strip=
            [[ "$typed" == *:* ]] && strip="${{typed%:*}}:"
            _ws_candidates tasks "$typed" "$strip"
        fi
    fi

    if [[ "$cur" != -* ]]; then
        local keep=() candidate
        for candidate in "${{COMPREPLY[@]}}"; do
            [[ "$candidate" == -* ]] || keep+=("$candidate")
        done
        COMPREPLY=("${{keep[@]}}")
    fi

    # menu-complete and insert-completions put the candidate straight on the
    # line, and a lone candidate is inserted whatever the type — a description
    # would be typed along with it.
    case "${{COMP_TYPE:-9}}" in
        37 | 42) return 0 ;;
    esac
    (( ${{#COMPREPLY[@]}} < 2 )) && return 0

    # Without a terminal bash reports a width of 0, which would truncate
    # everything to nothing.
    local width=${{COLUMNS:-80}}
    (( width < 24 )) && width=80
    local column=0 name description line described=() any=
    for name in "${{COMPREPLY[@]}}"; do
        (( ${{#name}} > column )) && column=${{#name}}
    done
    for name in "${{COMPREPLY[@]}}"; do
        description="${{_ws_branch_description[$name]:-}}"
        [[ -z "$description" ]] && description="$(_ws_describe "$context" "$name")"
        if [[ -n "$description" ]]; then
            printf -v line '%-*s  %s' "$column" "$name" "$description"
            any=yes
        else
            line="$name"
        fi
        (( ${{#line}} > width - 2 )) && line="${{line:0:width-5}}..."
        described+=("$line")
    done
    # A described candidate is wide, but not reliably wider than half the
    # terminal — padding to the full width is what keeps bash from laying two
    # of them side by side. Lists that carry no description stay in columns.
    if [[ -n "$any" ]]; then
        for name in "${{!described[@]}}"; do
            printf -v described[name] '%-*s' "$(( width - 2 ))" "${{described[name]}}"
        done
    fi
    COMPREPLY=("${{described[@]}}")
}}
if [[ "${{BASH_VERSINFO[0]}}" -eq 4 && "${{BASH_VERSINFO[1]}}" -ge 4 || "${{BASH_VERSINFO[0]}}" -gt 4 ]]; then
    complete -F _ws_prefer_commands -o nosort -o bashdefault -o default ws
else
    complete -F _ws_prefer_commands -o bashdefault -o default ws
fi
"#
    )
}

/// One `case` arm per candidate of `cmd`, nested under the context it belongs
/// to: the empty string for `ws` itself, the subcommand's name below that.
fn describe_arms(context: &[&str], cmd: &clap::Command) -> String {
    let mut arms = String::new();

    for sub in cmd.get_subcommands().filter(|c| !c.is_hide_set()) {
        let Some(about) = sub.get_about() else {
            continue;
        };
        let mut names = vec![sub.get_name()];
        names.extend(sub.get_visible_aliases());
        arms.push_str(&arm(&names, &about.to_string()));
    }

    for arg in cmd.get_arguments().filter(|a| !a.is_hide_set()) {
        let Some(help) = arg.get_help() else {
            continue;
        };
        let long = arg.get_long().map(|l| format!("--{l}"));
        let short = arg.get_short().map(|s| format!("-{s}"));
        let names: Vec<&str> = long
            .iter()
            .chain(short.iter())
            .map(String::as_str)
            .collect();
        if !names.is_empty() {
            arms.push_str(&arm(&names, &help.to_string()));
        }
    }

    if arms.is_empty() {
        return String::new();
    }
    let context = context
        .iter()
        .map(|c| format!("'{c}'"))
        .collect::<Vec<_>>()
        .join(" | ");
    format!("    {context})\n        case \"$2\" in\n{arms}        esac\n        ;;\n")
}

/// The options of `cmd` that take a value, as one `case` arm per context.
fn value_arms_for(context: &[&str], cmd: &clap::Command) -> String {
    let mut names = Vec::new();
    for arg in cmd.get_arguments() {
        if !arg.get_num_args().is_some_and(|n| n.takes_values()) {
            continue;
        }
        if let Some(long) = arg.get_long() {
            names.push(format!("--{long}"));
        }
        if let Some(short) = arg.get_short() {
            names.push(format!("-{short}"));
        }
    }
    if names.is_empty() {
        return String::new();
    }
    let context = context
        .iter()
        .map(|c| format!("'{c}'"))
        .collect::<Vec<_>>()
        .join(" | ");
    format!(
        "    {context})\n        case \"$2\" in\n        {}) return 0 ;;\n        esac\n        ;;\n",
        names.join(" | ")
    )
}

/// `switch | sw) printf '%s' 'Check out a branch ...' ;;`, quoted for bash.
fn arm(names: &[&str], description: &str) -> String {
    let description = description.lines().next().unwrap_or_default();
    format!(
        "        {}) printf '%s' '{}' ;;\n",
        names.join(" | "),
        description.replace('\'', r"'\''")
    )
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
