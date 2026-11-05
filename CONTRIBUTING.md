# Contributing

`rustc` is not in the system profile on NixOS; the flake provides it:

```
nix develop              # rustc, cargo, clippy, rustfmt, git, tmux, fzf, zoxide
cargo test               # 108 unit + 46 end-to-end tests, no network
cargo clippy --all-targets -- -D warnings
```

The dev shell defines `ws-completions <shell>`, which builds the working tree
and prints its completion script — `eval "$(ws-completions zsh)"` tries a
change without installing anything.

The end-to-end tests build local fixture repositories and stub out fzf,
zoxide, the package managers and `just` with small scripts, so nothing external is required.


## Documentation

The user documentation is `docs/*.md` and `docs/integrations/*.md`, one file
per page; https://ws.fomin.space is built from them at each release tag. A
change in behaviour comes with its documentation change in the same pull
request. Link between pages with relative `.md` links, which work both on
GitHub and on the site. A new page also has to be added to the site's page
list in [ws-site](https://github.com/anton-fomin/ws-site), whose build fails
on a page it does not know. `docs/agents/` is for coding agents and is not
published.
