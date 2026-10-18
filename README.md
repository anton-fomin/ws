# ws

[![CI](https://github.com/anton-fomin/ws/actions/workflows/ci.yml/badge.svg)](https://github.com/anton-fomin/ws/actions/workflows/ci.yml)

Multi-repo git worktree workspaces. One command checks a branch out in every
repository of a project, runs each repo's setup, and lays the result out in
tmux or [herdr](https://herdr.dev).

```
ws init --global                                   # once: your settings
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

**In a project's dev shell**

`ws` is normally a per-user tool, but putting it in a `devShell` pins one
version for everyone working on the project:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    ws = {
      url = "github:anton-fomin/ws";   # or github:anton-fomin/ws/v0.2.0
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, ws, ... }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      devShells.${system}.default = pkgs.mkShell {
        packages = [
          ws.packages.${system}.default
          pkgs.git                              # required at runtime
          pkgs.tmux pkgs.fzf pkgs.zoxide        # optional, see below
        ];
      };
    };
}
```

`nix develop` then has `ws` on `PATH`, and so do the tmux windows it opens
from there. For several systems wrap the outputs in `nixpkgs.lib.genAttrs` —
this repository's own `flake.nix` does exactly that.

The shell is only as wide as the project, though: outside it `ws` is gone, and
it cannot be what finds the *other* workspaces for `ws pick --global`. Install
it per user too if you want it everywhere.

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

Settings live in one place for all your workspaces, at
`~/.config/ws/config.toml` (`$XDG_CONFIG_HOME` is honoured; `WS_CONFIG` points
somewhere else, and `WS_CONFIG=""` turns the layer off). `ws init --global`
writes it with every key documented and its default filled in.

A workspace's `ws.toml` then stays small — what it actually owns:

```toml
name = "acme"
branch_dirs = "nested"          # an override, when this project differs

[[repo]]
url = "git@github.com:org/api.git"
setup = "pnpm install"          # or a list of commands; runs once per worktree

[[repo]]
url = "git@github.com:org/web.git"
path = "services/web"           # where it sits inside a branch directory
```

Merging is key by key, not file by file: a workspace that sets one key of
`[mux]` keeps the rest of your global `[mux]`. Arrays are replaced whole, never
appended to. Repositories belong to the workspace — `[[repo]]` in the global
file is an error rather than a silently ignored block. `ws status` prints the
files a workspace was built from, in the order they were applied.

TOML reads a bare key as part of the table above it, so single-key overrides
go above the first `[[repo]]` and whole sections below the last one. The file
`ws init` writes already offers them in the right places, commented out.

`bare_dir` and `worktree_dir` accept `.` for the workspace root, a relative
path, an absolute one, or `~`. They resolve per workspace, so an absolute
`worktree_dir` in the *global* file would point every workspace at the same
directory — set that one per workspace.

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
cargo test               # 52 unit + 15 end-to-end tests, no network
cargo clippy --all-targets -- -D warnings
```

The end-to-end tests build local fixture repositories and stub out fzf and
zoxide with small scripts, so nothing external is required.
