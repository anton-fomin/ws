# Layout

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
