# Commands

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
