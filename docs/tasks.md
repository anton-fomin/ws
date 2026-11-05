# Tasks

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

## Logs

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
`dir` is resolved like [`bare_dir`](configuration.md), which keeps every branch's logs in one
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
