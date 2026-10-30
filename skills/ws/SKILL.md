---
name: ws
description: Working inside a ws workspace — a multi-repo git worktree layout with a `ws.yaml` at its root, a `.ws.yaml` branch marker, or worktrees whose `.git` points into `.bare/`. Use when changing branches, running tests or dev servers, reaching a sibling repository, or touching `.env` and other local files there.
---

# Working in a ws workspace

`ws` checks one branch out in every repository of a project, side by side. You are standing in a **worktree** of a **branch directory**:

```
<root>/ws.yaml              # repos, setup commands, overrides
<root>/.bare/<repo>.git     # bare clones; git's real home
<root>/local/<repo>/...     # untracked files placed into every worktree
<root>/<branch-dir>/<repo>  # one worktree per repository
```

With a single repository the branch directory *is* the worktree. `worktree_dir` and `path` in `ws.yaml` may move things; `ws path` is the source of truth.

## Orient first

Run `ws status` before anything else. It names the workspace, the branch, the base it `started` from, and the config files applied. `ws list` shows every branch directory and its repos; `ws path <branch> [repo]` prints one path on stdout (messages go to stderr, so `$(ws path …)` is safe).

The sibling repositories of this branch are at `ws path <branch> <repo>`, all on the same branch name. A change that spans repos is one commit per repo, each pushed from its own worktree.

Each repository brings its own agent instructions: `AGENTS.md`, `CLAUDE.md`, `.claude/` at its worktree root. Started in the branch directory, you sit above all of them and none is loaded for you. Before working in a repo, read its instruction files and follow them for everything in that repo: its build, test and style conventions. Repos differ, so re-read when you move to the next one. This skill covers `ws`; the repo's own files cover the code.

## Branches belong to `ws`

Each branch lives in its own branch directory, and git lets a branch be checked out in only one worktree. To work on another branch, give it its own directory:

```
ws switch <branch> --cd                 # creates worktrees, installs, runs setup; prints the dir
ws switch <branch> --cd --from <ref>    # new branch starting at a ref every repo has
```

Always pass `--cd`: without it `ws` switches the user's tmux client or takes over the terminal. A new branch forks from the branch directory you stand in; pass `--from` when it should start elsewhere. Then `cd` into the printed directory.

`git switch`/`git checkout` inside a worktree moves only that one repo and leaves the branch directory named for a branch it no longer holds — keep the worktree on its branch. Likewise leave `git worktree add/remove` to `ws`.

`ws rm <branch>` deletes worktrees, logs and the session; `--force` discards uncommitted work. Both are the user's call — ask first.

`ws fetch` fetches every repository. `git fetch` in one worktree updates that repo's bare clone, so every branch directory sees it.

## Running things

`ws run` lists the tasks each repo declares (justfile, Taskfile.yml, package.json scripts). Prefer it to guessing the package manager:

```
ws run @api:test -- --filter foo    # one repo
ws run lint                         # every repo that has `lint`, in parallel
ws run dev --branch feature/x       # another branch directory
```

Output lines are prefixed `repo |` / `repo:task |`; the exit code is non-zero if any task failed. Long-running tasks (`dev`, `watch`) never exit — run them in the background. With `logs.enabled` (or `--logs`) each task also writes `<branch-dir>/logs/<repo>.<task>.log`; read those to see what a dev server is doing.

Dependencies were installed held to the lockfile when the worktree was created. After changing a lockfile or a setup step, rerun with `ws switch <branch> --cd --rerun-setup`.

## Local files

Untracked files such as `.env` or `.npmrc` may be placed from `<root>/local/<repo>/`. In the default `link` mode the file in the worktree is a **symlink**: editing it edits the shared original and changes every branch. Check with `ls -l` before writing to one; to change it for this branch only, ask the user — `copy` mode is set in `integrations.local.paths`. These files are never committed.

## Previewing

`--dry-run` prints the git/tmux/herdr commands instead of running the mutating ones; `-v` echoes every command. Use them before any `ws` command whose effect you are unsure of.
