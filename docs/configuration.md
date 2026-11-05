# Configuration

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
once per worktree, after the [local files](integrations/local-files.md) and the
[package managers](integrations/package-managers.md); `--rerun-setup` runs them again.

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
