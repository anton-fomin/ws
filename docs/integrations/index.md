# Integrations

`git` is the only thing `ws` needs. Everything else it drives is an
integration: each one comes into play when its program is on `PATH` and is
left out otherwise, and none of them can fail a switch — a problem is a
warning. `ws` shells out rather than linking libraries — the Nix closure is
just glibc, and the musl build links nothing at all — so these are the
programs it looks for:

| | |
|---|---|
| `tmux` or `herdr` | the session over the branch directory, see [Multiplexers](multiplexers.md); without either, `ws` prints the branch directory |
| `fzf` | the picker of `ws pick`, see [fzf and zoxide](fzf-zoxide.md) |
| `zoxide` | directory registration and `ws pick --global` |
| `yarn`, `pnpm`, `npm`, `uv` | [package manager](package-managers.md) installs, and package.json [tasks](../tasks.md) |
| `just`, `task` | the task sources of [`ws run`](../tasks.md) |

[Local files](local-files.md) need no program, only a directory. `ws status`
lists which integrations are active on this machine.
