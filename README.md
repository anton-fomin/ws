# ws

[![CI](https://github.com/anton-fomin/ws/actions/workflows/ci.yml/badge.svg)](https://github.com/anton-fomin/ws/actions/workflows/ci.yml)

Multi-repo git worktree workspaces. One command checks a branch out in every
repository of a project, runs each repo's setup, and lays the result out in
tmux or [herdr](https://herdr.dev).

```
ws init --repo git@github.com:org/api.git --repo git@github.com:org/web.git
ws fetch
ws switch feature/JIRA-123
```

## Install on NixOS

The repository ships a flake, so `ws` installs straight from it. Flakes must be
enabled: `nix.settings.experimental-features = [ "nix-command" "flakes" ];`

**Try it without installing**

```
nix run github:anton-fomin/ws -- --help
```

**Per user**

```
nix profile add github:anton-fomin/ws
```

Older Nix spells that `nix profile install`. Afterwards `nix profile upgrade ws`
(or `--all`) updates it and `nix profile remove ws` drops it.

**System-wide, as a flake input**

```nix
# flake.nix of your NixOS configuration
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    ws = {
      url = "github:anton-fomin/ws";
      inputs.nixpkgs.follows = "nixpkgs";   # one nixpkgs, not two
    };
  };

  outputs = { nixpkgs, ws, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./configuration.nix
        ({ pkgs, ... }: {
          environment.systemPackages = [
            ws.packages.${pkgs.system}.default
            pkgs.git                              # required at runtime
            pkgs.tmux pkgs.fzf pkgs.zoxide        # optional, see below
          ];
        })
      ];
    };
  };
}
```

Then `nixos-rebuild switch --flake .#myhost`, and `nix flake update ws` to pull
a newer version.

Under Home Manager it is the same expression in `home.packages`.

**Pin to a release** by putting the tag in the URL:

```
github:anton-fomin/ws/v0.2.0
```

**Runtime dependencies.** `ws` shells out rather than linking libraries, so the
closure is just glibc — the tools it drives have to be on `PATH`:

| | |
|---|---|
| `git` | required |
| `tmux` or `herdr` | whichever multiplexer you use; without either, `ws` prints the branch directory |
| `fzf` | only for `ws pick` |
| `zoxide` | only for the directory registration and `ws pick --global` |

## Layout

The workspace root is the directory holding `ws.toml`:

```
<root>/
  ws.toml
  .ws/state.toml          # which setup commands already ran
  .bare/api.git           # one bare clone per repository
  .bare/web.git
  main/api  main/web      # a branch directory per branch...
  feature-JIRA-123/api    # ...holding one worktree per repository
  feature-JIRA-123/web
```

With a single repository the branch directory *is* the worktree
(`<root>/main/`). `repo_layout = "single" | "grouped"` forces either shape.

Branch names with slashes are folded into one directory by default
(`feature/x` → `feature-x`); `branch_dirs = "nested"` keeps the hierarchy.

Both base directories are configurable. `worktree_dir` moves the branch
directories somewhere else — a subdirectory, or an absolute path (`~` is
expanded) if the worktrees belong on another disk entirely. Each `[[repo]]`
can set `path` to say where it sits inside a branch directory, which is how a
worktree layout can mirror a monorepo's own structure:

```toml
worktree_dir = "~/trees/acme"   # default: "." — next to ws.toml

[[repo]]
name = "web"
url = "git@github.com:org/web.git"
path = "services/web"           # default: the repo name
```

With the worktrees outside the root there is no `ws.toml` above them, so `ws`
follows the worktree's `.git` file back to the bare repo and finds the
workspace from there. Every command keeps working from inside a worktree.

Which branch lives in which directory is always read back from git, so it
cannot drift. `.ws/state.toml` only records which `setup` commands have run.

## Commands

| Command | |
|---|---|
| `ws init` | write a `ws.toml` |
| `ws fetch` | clone missing bare repos, fetch all of them |
| `ws switch <branch>` | worktrees + setup + session (alias `sw`) |
| `ws pick [--global]` | choose a branch or workspace with fzf (alias `p`) |
| `ws list` | branch directories and their repos (alias `ls`) |
| `ws path <branch> [repo]` | print a path |
| `ws rm <branch> [--force]` | remove worktrees and close the session |
| `ws status` | where am I |
| `ws shell-init <shell>` | shell wrapper so `ws switch` also `cd`s |

Global flags: `--dry-run` prints the git/tmux/herdr commands instead of running
the mutating ones, `--mux <backend>` overrides the multiplexer, `-v` echoes
every command.

Human-readable output goes to stderr and paths go to stdout, so
`cd "$(ws path feature/x)"` works.

## Multiplexers

* **tmux** — one window per repository named after it, plus a shared window for
  the branch directory when there are several repos. Inside tmux the current
  client is switched; outside, `ws` hands the terminal to `attach-session`.
* **herdr** — one workspace per branch labelled like the tmux session, one pane
  per repository. Command shapes are verified against herdr 0.9.0; ids are read
  back from the JSON `.result` rather than constructed.
* **none** — prints the branch directory instead.

`backend = "auto"` picks whichever one `ws` is running inside, then whichever is
installed.

## fzf and zoxide

`ws pick` offers the branches already checked out, the `origin/*` branches that
are not, and — with `--global`, or outside a workspace — other workspace roots
from the zoxide database. Inside tmux the filter opens in a popup; the popup
runs `ws` itself, so the selection is acted on from inside the session.

Every switch registers the branch directory and its worktrees with zoxide, so
`z feature-JIRA` jumps straight there; `ws rm` removes them again. Set
`[zoxide] enabled = false` to opt out.

## Configuration

`ws init` writes a commented `ws.toml`. Every key has a default, so the minimum
is one `[[repo]]` block:

```toml
[[repo]]
url = "git@github.com:org/api.git"
setup = "pnpm install"        # or a list of commands; runs once per worktree
```

`bare_dir` and `worktree_dir` accept the same forms: `.` for the workspace
root, a relative path, an absolute one, or `~`.

Launcher commands are configurable, which is also how the tools are tested
without installing them:

```toml
[mux.herdr]
command = ["nix", "run", "nixpkgs#herdr", "--"]
```

`WS_TMUX_CMD`, `WS_HERDR_CMD`, `WS_ZOXIDE_CMD` and `WS_PICKER_CMD` override
those at runtime.

## Development

`rustc` is not in the system profile on NixOS; the flake provides it:

```
nix develop              # rustc, cargo, clippy, rustfmt, git, tmux, fzf, zoxide
cargo test               # 48 unit + 11 end-to-end tests, no network
cargo clippy --all-targets -- -D warnings
```

The end-to-end tests build local fixture repositories and stub out fzf and
zoxide with small scripts, so nothing external is required.
