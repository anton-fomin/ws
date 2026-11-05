# Package managers

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
