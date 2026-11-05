# Local files

Untracked files you want in every worktree — a `.env` with credentials,
editor settings, a personal `Makefile.local`, an `.npmrc` with a token — live
in one directory beside the workspace, `local/` by default
(`integrations.local.dir`, resolved like [`bare_dir`](../configuration.md)). The integration is on
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
