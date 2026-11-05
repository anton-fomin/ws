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

```
acme/
  ws.yaml
  .bare/api.git  .bare/web.git     # one bare clone per repository
  main/api       main/web          # a branch directory per branch...
  feature-JIRA-123/api             # ...holding one worktree per repository
  feature-JIRA-123/web
```

Each switch places your untracked local files (`.env`, `.npmrc`), installs
dependencies held to the lockfile, runs each repository's setup and opens a
tmux or herdr session over the branch directory. `ws run dev` then starts
`dev` in every repository at once, with prefixed output.

## Install

On Linux or macOS:

```
curl -fsSL https://raw.githubusercontent.com/anton-fomin/ws/main/install.sh | sh
```

Nix, pinned versions and installing by hand are in
[Getting started](docs/getting-started.md). `git` is the only requirement;
tmux, herdr, fzf, zoxide and the package managers are optional integrations.

## Documentation

**https://ws.fomin.space** has the documentation for every release. The same
pages are in [`docs/`](docs/) here, as of this commit:

- [Getting started](docs/getting-started.md): install, shell wrapper, a first workspace
- [Layout](docs/layout.md): what goes where on disk
- [Commands](docs/commands.md)
- [Configuration](docs/configuration.md): `ws.yaml`, the global config, setup commands
- [Integrations](docs/integrations/index.md): [multiplexers](docs/integrations/multiplexers.md),
  [fzf and zoxide](docs/integrations/fzf-zoxide.md),
  [local files](docs/integrations/local-files.md),
  [package managers](docs/integrations/package-managers.md)
- [Tasks](docs/tasks.md): `ws run` and its logs
- [Shell completion](docs/shell-completion.md)
- [Coding agents](docs/coding-agents.md): the `/ws` launcher and the workspace skill

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option. Unless you explicitly state
otherwise, any contribution intentionally submitted for inclusion in `ws` by
you, as defined in the Apache-2.0 license, shall be dual licensed as above,
without any additional terms or conditions.
