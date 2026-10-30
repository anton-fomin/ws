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
github:anton-fomin/ws/v0.6.0
```

**In a project's dev shell**

`ws` is normally a per-user tool, but putting it in a `devShell` pins one
version for everyone working on the project:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    ws = {
      url = "github:anton-fomin/ws";   # or github:anton-fomin/ws/v0.6.0
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
from there. Completion comes along with the package — see
[Shell completion](#shell-completion) for the one line zsh needs. For several
systems wrap the outputs in `nixpkgs.lib.genAttrs` — this repository's own
`flake.nix` does exactly that.

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
| `yarn`, `pnpm`, `npm`, `uv` | only for the [package manager](#package-managers) installs, and package.json [tasks](#tasks) |
| `just`, `task` | only for [`ws run`](#tasks) |

## Layout

The workspace root is the directory holding `ws.yaml`:

```
<root>/
  ws.yaml
  .ws/state.yaml          # which setup commands already ran
  local/api/.env          # untracked files placed into every worktree
  .bare/api.git           # one bare clone per repository
  .bare/web.git
  main/api  main/web      # a branch directory per branch...
  feature-JIRA-123/api    # ...holding one worktree per repository
  feature-JIRA-123/web
```

With a single repository the branch directory *is* the worktree
(`<root>/main/`). `repo_layout: single | grouped` forces either shape.

Branch names with slashes are folded into one directory by default
(`feature/x` → `feature-x`); `branch_dirs: nested` keeps the hierarchy.

Both base directories are configurable. `worktree_dir` moves the branch
directories somewhere else — a subdirectory, or an absolute path (`~` is
expanded) if the worktrees belong on another disk entirely. Each entry under
`repos` can set `path` to say where it sits inside a branch directory, which is
how a worktree layout can mirror a monorepo's own structure:

```yaml
worktree_dir: "~/trees/acme"    # default: "." — next to ws.yaml

repos:
  - name: web
    url: "git@github.com:org/web.git"
    path: services/web          # default: the repo name
```

With the worktrees outside the root there is no `ws.yaml` above them, so `ws`
follows the worktree's `.git` file back to the bare repo and finds the
workspace from there. Every command keeps working from inside a worktree.

Which branch lives in which directory is always read back from git, so it
cannot drift. `.ws/state.yaml` only records which `setup` commands have run.

## Commands

| Command | |
|---|---|
| `ws init` | write a `ws.yaml` |
| `ws fetch` | clone missing bare repos, fetch all of them |
| `ws switch <branch>` | worktrees + setup + session (alias `sw`) |
| `ws switch <branch> --from <ref>` | …starting a new branch at a branch, tag or commit |
| `ws pick [--global]` | choose a branch or workspace with fzf (alias `p`) |
| `ws list` | branch directories and their repos (alias `ls`) |
| `ws path <branch> [repo]` | print a path |
| `ws rm <branch> [--force]` | remove worktrees and close the session |
| `ws run <task>` | run a task in every repo that has it, side by side (alias `r`) |
| `ws run @repo:task` | …in that one repo only; `ws run` alone lists the tasks |
| `ws status` | where am I |
| `ws shell-init <shell>` | shell wrapper so `ws switch` also `cd`s |
| `ws completions <shell>` | completion script for bash, zsh or fish |

Global flags: `--dry-run` prints the git/tmux/herdr commands instead of running
the mutating ones, `--mux <backend>` overrides the multiplexer, `-v` echoes
every command.

`ws switch --from <ref>` says where a branch that does not exist yet starts,
overriding the configured `base_branch` for that one call:

```
ws switch hotfix/x --from release/v0.4.5        # a tag, or a branch
ws switch hotfix/x --from origin/release/v0.4.5 # the remote one, explicitly
```

The ref is resolved per repository, in this order: `origin/x` exactly as
written, a bare name as the remote branch when there is one, then the local
branch, then anything else git can reach — a tag, a commit, `HEAD~2`. **Every
repository has to have it**, and that is checked before the first worktree is
created, so a tag that is only in one of them fails the switch outright rather
than leaving the branch meaning two different things:

```
error: --from release/v0.4.5 is not in web — fetch it, or name a ref every repository has
```

A branch that exists already is checked out as it is, and `--from` says it
went unused rather than quietly starting it somewhere else.

**Without `--from`**, a new branch continues the branch directory you are
standing in: run `ws switch feature/child` from anywhere inside
`…/feature-parent` and it forks from `feature/parent` — the local branch as
you have it, not `origin/`'s idea of it. Outside a branch directory, the
configured `base_branch` is the base as before. A repository that has never
seen that branch falls back to the base too, with a warning; unlike `--from`,
this one is a default rather than an instruction, so it bends instead of
breaking.

**Which branch directory you are in** is answered by walking up from wherever
you stand, not by listing every repository's worktrees. A branch directory
that *holds* repositories carries a `.ws.yaml` naming itself:

```yaml
branch: feature/child
from: feature/parent
```

`ws status` prints that `from` as `started`, and git has the last word on the
branch itself — a worktree moved with `git switch` afterwards is reported
where it actually is, not where the marker remembers it.

With a single repository there is no such file, because the branch directory
*is* that repository's working tree: anything `ws` wrote there would be a file
in your project, picked up by `git add .` and reported by `git status`, and
keeping it quiet would mean writing into git's own directory or into your
`.gitignore`. Neither is `ws`'s to touch — and nothing is needed, since a
worktree already says which branch it is on. The walk up finds the `.git` and
asks it.

Human-readable output goes to stderr and paths go to stdout, so
`cd "$(ws path feature/x)"` works.

## Shell completion

`ws completions <shell>` prints the completion script for `bash`, `zsh` or
`fish` — subcommands, their aliases and every flag. It completes the command
name `ws`, which is also the name of the `ws shell-init` wrapper function, so
the two integrations stack rather than shadowing each other.

**From the Nix package** nothing has to be done: the derivation runs
`ws completions` at build time and installs the scripts where each shell looks
for them.

| | |
|---|---|
| bash | `share/bash-completion/completions/ws.bash` |
| zsh | `share/zsh/site-functions/_ws` |
| fish | `share/fish/vendor_completions.d/ws.fish` |

In a user or system profile those are picked up once completion is enabled —
`programs.bash.completion.enable` (NixOS) or `programs.zsh.enable` with
`enableCompletion`, Home Manager's equivalents, or whatever your distribution
uses. bash finds its file lazily from `PATH`, so a `ws` on `PATH` is enough;
zsh needs the directory on `fpath` *before* `compinit` runs.

**Without Nix**, write the script where your shell will find it:

```bash
# bash
ws completions bash > ~/.local/share/bash-completion/completions/ws

# zsh — any directory on $fpath, created before compinit runs
ws completions zsh > ~/.local/share/zsh/site-functions/_ws

# fish
ws completions fish > ~/.config/fish/completions/ws.fish
```

Or load it for the current shell only, which is also the quickest way to try a
change: `eval "$(ws completions zsh)"`.

**What it offers.** `ws <Tab>` lists the subcommands with their descriptions,
one per line; the options stay out of the way until the word starts with a
dash, where `ws --<Tab>` lists those the same way.

```
$ ws <Tab>
init         Write a ws.yaml in the current directory
fetch        Clone missing bare repositories and fetch all of them
switch       Check out a branch in every repository and open the session
...
```

zsh and fish are told the candidate and its description separately, so they do
all of this themselves. bash has one array for both jobs — whatever it is
handed is both what it shows and what it types onto the line — so the script
`ws completions bash` prints carries a description table and attaches it only
where bash is about to *list* the candidates: never for a lone match, and
never under `menu-complete`, where each candidate goes straight onto the line.
It is the same trick cobra plays, and about the only one available: the limit
is bash's, not the completion library's.

**Branch arguments** are completed by asking `ws`, rather than with file
names. `ws rm` and `ws path` name a worktree that exists, described by the
directory `ws rm` would delete and the repositories inside it:

```
$ ws rm <Tab>
feature/JIRA-123  feature-JIRA-123 — api, web
main              api, web
```

`ws switch` takes any branch of the workspace, so it offers all of them —
deduplicated across the repositories, the ones with worktrees first, then the
`origin/*` branches without, which is what the fzf picker shows too. The
description names the repositories the branch was found in:

```
$ ws switch <Tab>
main              checked out — api, web
feature/JIRA-123  checked out — api, web
feature/only-api  api
shared/thing      api, web
```

A branch that is still only on `origin` completes the same as one already
checked out, and `ws switch` with no argument still opens the picker.

`--from` is completed from the branches *and* the tags, each described by the
repositories it was found in. The `origin/` forms are the same branches again,
so they appear once that prefix is typed rather than doubling the list:

```
$ ws switch hotfix/x --from <Tab>
main              checked out — api, web
shared/thing      api, web
release/v0.4.5    tag — api
v1.0.0            tag — api, web

$ ws switch hotfix/x --from origin/<Tab>
origin/main          api, web
origin/shared/thing  api, web
```

Outside a workspace every one of these lists is empty rather than an error —
a shell asks on every <kbd>Tab</kbd>, wherever the prompt happens to be.

**In a dev shell.** Add `ws` to the shell's `packages` as above and bash is
done: bash-completion locates `share/bash-completion/completions/ws.bash` from
`PATH`, so the completion appears with the package and leaves with it.

zsh needs the directory on `fpath`, and a dev shell cannot put it there. A
`shellHook` runs under bash, so a zsh script generated and `eval`ed in one only
produces bash errors. `FPATH` is worse than useless: it is tied to `fpath`, and
assigning it in a *running* zsh — which is what direnv does when it exports the
environment — **replaces** `fpath` with that one directory instead of adding to
it, so every standard completion function disappears at once. Unsetting it on
the way out empties `fpath` altogether.

What a dev shell *can* do is say where its completions are: `mkShell` puts
every package's `share` directory on `XDG_DATA_DIRS`, the `ws` package's
`share/zsh/site-functions/_ws` included, and direnv exports that like any other
variable. [zsh-completion-sync](https://github.com/BronzeDeer/zsh-completion-sync)
watches it before every prompt, adds the new `site-functions` directories to
`fpath` and registers what they hold — so the completion of every package in
the shell appears on entering it and goes again on leaving. One line in
`~/.zshrc`, after `compinit` and the direnv hook, and nothing per project:

```zsh
znap source BronzeDeer/zsh-completion-sync     # or source its .plugin.zsh
```

Without a plugin, a completion that loads itself the first time you ask for
one does the same for `ws` alone:

```zsh
# ~/.zshrc, after compinit
_ws() {
  local script
  script="$(command ws completions zsh 2>/dev/null)" || return 1
  unfunction _ws
  eval "$script"
  _ws "$@"
}
compdef _ws ws
```

`ws` then has to exist only at the moment you press <kbd>Tab</kbd> — outside
a dev shell the stub simply yields no matches. It costs one
`ws completions zsh` per shell and covers `nix develop`, `nix develop -c zsh`
and direnv alike.

And for the shell you are in, by hand: `eval "$(ws completions zsh)"`, once
`compinit` has run.

## Multiplexers

* **tmux** — one window per repository named after it, plus a shared window for
  the branch directory when there are several repos. Inside tmux the current
  client is switched; outside, `ws` hands the terminal to `attach-session`.
* **herdr** — one workspace per branch labelled like the tmux session, one tab
  per repository plus one for the branch directory, mirroring the tmux
  windows. `[mux.herdr] layout = "panes"` puts them side by side in a single
  tab instead, and `split_direction` then says where each one goes. Command
  shapes are verified against herdr 0.9; ids are read back from the JSON
  `.result` rather than constructed.
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
`integrations.zoxide.enabled: false` to opt out.

## Local files

Untracked files you want in every worktree — a `.env` with credentials,
editor settings, a personal `Makefile.local`, an `.npmrc` with a token — live
in one directory beside the workspace, `local/` by default
(`integrations.local.dir`, resolved like `bare_dir`). The integration is on
whenever that directory exists.

With a single repository everything in `local/` is placed into the worktree.
With several, `local/` holds one subdirectory per repository, named after it:
`local/api/.env` goes to `<branch>/api/.env`.

Each entry's path relative to that directory is matched against the `paths`
rules in order — the whole path, as a regular expression, with `/` between the
components — and the first rule that matches decides how it is placed:

| mode | |
|---|---|
| `link` | a symlink to the file in `local/`: one copy, shared by every worktree (the default) |
| `copy` | an independent copy per worktree |
| `hardlink` | shared like `link`, but a real file to tools that will not follow a symlink |

```yaml
integrations:
  local:
    dir: local
    paths:
      - { pattern: '\.env.*', mode: copy }   # each branch edits its own
      - { pattern: '.*', mode: link }         # everything else is shared
```

An entry no rule matches is left out. A directory is placed whole when the
worktree has nothing at that path; when the repository tracks that directory,
the walk goes inside it instead, so `local/config/dev.yaml` lands next to the
repository's own `config/base.yaml`. What the worktree already has is never
replaced — a tracked file wins, with a warning on the switch that creates the
worktree — and the placing runs on every switch, so a file added to `local/`
later reaches the existing worktrees too.

Local files are placed before the package managers install (an `.npmrc` may be
what lets them reach a registry) and before `setup`; `--no-setup` skips them.
They are not added to any ignore file: keep them in the repository's
`.gitignore`, or your global one. A branch whose directory would be `local/`
itself is refused.

## Package managers

yarn, pnpm, npm and uv are integrations too. Each is on by default when its
program is on `PATH` (`enabled: auto`), and then acts only on repositories that
are recognisably its:

| | recognised by |
|---|---|
| yarn | `yarn.lock` |
| pnpm | `pnpm-lock.yaml` |
| npm | `package-lock.json` or `npm-shrinkwrap.json` |
| uv | `uv.lock` or `pyproject.toml` |

For the three node managers the `packageManager` field of `package.json` —
what corepack reads — beats the lockfile; a repository with two lockfiles and
no such field gets a warning and the first of yarn, pnpm, npm. uv is detected
independently, so a repository with a Python service and a web front end gets
both.

In every worktree a switch creates, the manager installs **held to the
lockfile**, so two worktrees of the same commit get the same dependencies and
an out-of-date lockfile is an error rather than a silent rewrite:

| | runs |
|---|---|
| yarn | `yarn install --immutable` — `--frozen-lockfile` on yarn 1.x, asked per worktree since corepack makes the version the project's choice |
| pnpm | `pnpm install --frozen-lockfile` |
| npm | `npm ci` |
| uv | `uv sync` |

**Before installing**, yarn and pnpm seed the new worktree's `node_modules` by
hard-linking the one from the branch you switched *from*, or from the default
branch when that is not a branch directory. A hard link costs an inode instead
of the bytes, and the frozen install then reconciles the seed against what the
branch's lockfile actually says — the result is the same as installing from
nothing, minus most of the work. Symlinks are recreated as symlinks, so pnpm's
relative links into `node_modules/.pnpm` point into the copy, not back at the
original. A monorepo's package-level `node_modules` (up to three levels deep)
come along from the same source worktree; across filesystems the files are
copied instead.

npm does not seed by default: `npm ci` deletes `node_modules` before it starts,
which is what makes it reproducible. uv never seeds — a virtualenv carries its
own absolute path and is not relocatable — but its global cache already
hard-links packages into place.

The integrations run before a repository's own `setup` commands, which can then
assume the dependencies are there. An existing worktree whose dependency
directory is present is left alone; `--rerun-setup` installs again and
`--no-setup` skips the managers along with `setup`. An install that fails is a
warning, not a failed switch — the session still opens, and you fix it there.

```yaml
integrations:
  pnpm:
    install: [install, --offline]   # replaces the arguments entirely
  npm:
    install: [install]              # trade npm ci for seeding
    link: true
  yarn:
    enabled: false
```

`ws status` lists which integrations are active on this machine.

## Tasks

`ws run` runs what the repositories declare they can run — the recipes of a
`justfile`, the tasks of a `Taskfile.yml`, the `scripts` of `package.json` —
in the branch directory you are standing in:

```
ws run                         # every task, per repository
ws run @api                    # the tasks of api
ws run dev                     # dev in every repository that has one, at once
ws run @api:test               # test in api only
ws run @api:db:migrate @web:build -- --verbose   # several; args after --
ws run dev --branch feature/x  # in another branch directory
```

A bare name runs in **every** repository that has it, all at the same time;
one that no repository has is an error. `@repo:task` names a single
repository — everything after the first colon is the task, so `@api:db:migrate`
is go-task's `db:migrate` and `@api:db::seed` is just's `seed` in module `db`.
Arguments after `--` go to every task named: `just test --x`,
`task test -- --x` (which go-task hands over as `CLI_ARGS`), and
`npm run test -- --x` or `pnpm run test --x`.

Outside a branch directory `ws run` uses the default branch's; `--branch`
picks any other that has worktrees.

**Output.** Every line is printed with a prefix naming where it came from —
the repository, or `repo:task` once the tasks differ — padded to line up and
coloured, one colour per task. stdout stays stdout and stderr stays stderr, so
both can be redirected as usual.

```
api | listening on :8080
web | VITE v5.4.0  ready in 312 ms
api | GET /health 200
```

The tasks run in pipes, where most tools stop colouring; while `ws` colours,
it sets `FORCE_COLOR=1` and `CLICOLOR_FORCE=1` so they carry on.
`tasks.color: never` turns both off, and `NO_COLOR` is honoured. The tasks
also get the variables setup commands get — `WS_ROOT`, `WS_BRANCH`,
`WS_BRANCH_DIR`, `WS_WORKTREE`, `WS_REPO`.

A task that fails does not stop the others: `ws run` waits for all of them,
says which failed, and exits non-zero. <kbd>Ctrl-C</kbd> reaches every task
directly, and `ws` stays until they are gone, so what they print on the way
out is still shown; a `SIGTERM` or `SIGHUP` sent to `ws` alone is passed on.

**Where a task comes from.** A repository can declare the same name in more
than one place; `tasks.order` decides which one runs it, and each source can
be switched off:

```yaml
tasks:
  order: [just, task, package]  # the first source with the name wins
  just: { enabled: auto, command: [just] }
  task: { enabled: auto, command: [task] }   # go-task
  package: { enabled: auto }    # auto: whenever there is a package.json
  color: auto                   # auto | always | never
```

`just` and `task` are on when they are on `PATH`, and only read a file in the
worktree itself, never one further up. Scripts run with the repository's own
package manager — the `packageManager` field, then the lockfile, then npm —
through the launcher configured under `integrations`, whether or not that
integration installs.

### Logs

With logging on, everything `ws run` prints also goes to a file per task,
without the colours:

```yaml
logs:
  enabled: true           # or per run: --logs / --no-logs
  dir: "{branch_dir}/logs"
  file: "{repo}.{task}.log"
  max_size: 10MB          # 512K, 1GB, or bytes; 0 never rotates
  keep: 5
  timestamps: true
```

```
feature-JIRA-123/
  api/  web/
  logs/api.dev.log
  logs/api.dev.log.1     # the previous 10MB
  logs/web.dev.log
```

By default the logs live in the branch directory they were written for, next
to its repositories, and `ws rm` removes them with it. `{branch_dir}` may only
start `dir`, and what follows must not be — or contain — a repository's
`path`. With a single repository the branch directory *is* the worktree, so
`logs/` is an untracked directory there: `ws list` and `ws rm` do not count it
as a change, but git does, so put `logs/` in your global gitignore. Any other
`dir` is resolved like `bare_dir`, which keeps every branch's logs in one
place:

```yaml
logs:
  dir: logs                           # <root>/logs
  file: "{branch}/{repo}.{task}.log"  # <root>/logs/feature-JIRA-123/api.dev.log
```

`file` takes `{branch}` (with slashes folded, like a flat branch directory),
`{repo}` and `{task}`, and may not leave `dir`. Tasks whose files coincide —
`file: "{branch}.log"` puts a whole branch in one — share it, each line marked
`repo:task |`. Every run is framed by the command it ran and how it ended.

A file is rotated by size, not by date: before a line would take it past
`max_size`, it moves to `<file>.1`, the `.1` before it to `.2`, and so on up to
`keep`; the oldest is deleted. A dev server left running for a week costs at
most `max_size × (keep + 1)` per task. With logs kept outside the branch
directories, a branch whose directory would be the logs directory is refused,
as with `local/`.

## Configuration

Settings live in one place for all your workspaces, at
`~/.config/ws/config.yaml` (`$XDG_CONFIG_HOME` is honoured; `WS_CONFIG` points
somewhere else, and `WS_CONFIG=""` turns the layer off). `ws init --global`
writes it with every key documented and its default filled in.

A workspace's `ws.yaml` then stays small — what it actually owns:

```yaml
name: acme
branch_dirs: nested             # an override, when this project differs

repos:
  - url: "git@github.com:org/api.git"
    setup: cp .env.example .env # or a list; once per worktree, after the install

  - url: "git@github.com:org/web.git"
    path: services/web          # where it sits inside a branch directory
    setup: |                    # a script: every line runs, a failing one stops it
      cp {root}/shared/web.env .env
      echo "API_TOKEN={env:API_TOKEN}" >> .env
      ./scripts/codegen "$WS_WORKTREE"
```

**Setup commands** are a string or a list of strings, and any one of them may
span several lines. Each runs through `sh -e` in the repository's worktree, so
a failing line stops the setup just as a failing list entry does. They run
once per worktree, after the [local files](#local-files) and the
[package managers](#package-managers); `--rerun-setup` runs them again.

Before running, these are replaced:

| | |
|---|---|
| `{root}` | the workspace root, where `ws.yaml` is |
| `{branch}` | the branch, as git names it (`feature/x`) |
| `{branch_dir}` | the branch directory |
| `{worktree}` | this repository's worktree — also the working directory |
| `{repo}` | the repository's name |
| `{env:NAME}` | the environment variable `NAME`; an unset one fails the setup |
| `{env:NAME:-default}` | `NAME`, or `default` when it is unset or empty — like the shell's `${NAME:-default}` |

```yaml
setup: |
  echo "API_URL={env:API_URL:-http://localhost:8080}" > .env
  echo "LOG_LEVEL={env:LOG_LEVEL:-}" >> .env      # an empty default is fine
```

A default runs to the closing brace, so it may hold spaces, colons or `:-`,
but not a `}`.

**Escaping.** To hand a placeholder to the command as written — a template of
your own that uses the same syntax, say — double its braces: `{{env:TOKEN}}`
becomes the literal `{env:TOKEN}`, and `{{repo}}` becomes `{repo}`.

```yaml
setup: envsubst-like-tool --template '{{env:TOKEN}}'   # receives {env:TOKEN}
```

Nothing else needs escaping. Braces around anything that is not a placeholder
are left alone, so `${HOME}`, awk's `'{print $1}'`, `'{if(x){print}}'` and
brace expansions like `cp x.{a,b} y` reach the shell as written — and a
doubled pair only counts as an escape when what it encloses is a placeholder.

Values are inserted verbatim — quote the placeholder if a path may hold a
space. The same values are exported as `WS_ROOT`, `WS_BRANCH`,
`WS_BRANCH_DIR`, `WS_WORKTREE` and `WS_REPO`, which the shell quotes for you
as `"$WS_WORKTREE"`.

Merging is key by key, not file by file: a workspace that sets one key of
`mux` keeps the rest of your global `mux`. Sequences are replaced whole, never
appended to. Repositories belong to the workspace — `repos` in the global file
is an error rather than a silently ignored block. `ws status` prints the files
a workspace was built from, in the order they were applied.

Overrides go anywhere in the file, in any order; the one `ws init` writes
offers them commented out. A URL holding a colon is safe either way, but the
file `ws init` writes quotes the values that came from you.

`bare_dir` and `worktree_dir` accept `.` for the workspace root, a relative
path, an absolute one, or `~`. They resolve per workspace, so an absolute
`worktree_dir` in the *global* file would point every workspace at the same
directory — set that one per workspace.

Launcher commands are configurable, which is also how the tools are tested
without installing them:

```yaml
mux:
  herdr:
    command: [nix, run, "nixpkgs#herdr", --]
```

`WS_TMUX_CMD`, `WS_HERDR_CMD`, `WS_ZOXIDE_CMD`, `WS_YARN_CMD`, `WS_PNPM_CMD`,
`WS_NPM_CMD`, `WS_UV_CMD`, `WS_JUST_CMD`, `WS_TASK_CMD` and `WS_PICKER_CMD`
override those at runtime.

## Coding agents

[`skills/ws/SKILL.md`](skills/ws/SKILL.md) is a skill that teaches an agent
working inside a workspace how to find its way around: `ws status` to orient,
each repository's own `AGENTS.md` or `CLAUDE.md`, `ws switch --cd` instead of
`git switch`, `ws run` for tasks, and which local files are shared symlinks.
`ws init` points here until an agent has it.

Install it once per user with the [`skills`](https://skills.sh) CLI, naming
the agents you use:

```
npx skills add anton-fomin/ws -g -a claude-code opencode
```

That puts the skill in `~/.agents/skills/ws`, which OpenCode reads, and links
it into `~/.claude/skills/ws` for Claude Code. `-a` takes any agent the CLI
knows (`codex`, `cursor`, `gemini-cli`, …). It installs from `main`;
`npx skills update -g` fetches the current version.

**Without Node**, copy the `skills/ws` directory of a checkout into the
agent's skill directory, or symlink it so that `git pull` keeps it current:

| agent | skill directory |
|---|---|
| Claude Code | `~/.claude/skills/ws` (`$CLAUDE_CONFIG_DIR/skills/ws` when that is set) |
| OpenCode | `~/.config/opencode/skills/ws`; it also reads `~/.claude/skills` and `~/.agents/skills` |

## Development

`rustc` is not in the system profile on NixOS; the flake provides it:

```
nix develop              # rustc, cargo, clippy, rustfmt, git, tmux, fzf, zoxide
cargo test               # 108 unit + 42 end-to-end tests, no network
cargo clippy --all-targets -- -D warnings
```

The dev shell defines `ws-completions <shell>`, which builds the working tree
and prints its completion script — `eval "$(ws-completions zsh)"` tries a
change without installing anything.

The end-to-end tests build local fixture repositories and stub out fzf,
zoxide, the package managers and `just` with small scripts, so nothing external is required.
