# Shell completion

`ws completions <shell>` prints the completion script for `bash`, `zsh` or
`fish` — subcommands, their aliases and every flag. It completes the command
name `ws`, which is also the name of the `ws shell-init` wrapper function, so
the two integrations stack rather than shadowing each other.

**From the Nix package** nothing has to be done: the derivation runs
`ws completions` at build time and installs the scripts where each shell looks
for them.

| | |
|---|---|
| bash | `share/bash-completion/completions/ws.bash` |
| zsh | `share/zsh/site-functions/_ws` |
| fish | `share/fish/vendor_completions.d/ws.fish` |

In a user or system profile those are picked up once completion is enabled —
`programs.bash.completion.enable` (NixOS) or `programs.zsh.enable` with
`enableCompletion`, Home Manager's equivalents, or whatever your distribution
uses. bash finds its file lazily from `PATH`, so a `ws` on `PATH` is enough;
zsh needs the directory on `fpath` *before* `compinit` runs.

**Without Nix**, write the script where your shell will find it:

```bash
# bash
ws completions bash > ~/.local/share/bash-completion/completions/ws

# zsh — any directory on $fpath, created before compinit runs
ws completions zsh > ~/.local/share/zsh/site-functions/_ws

# fish
ws completions fish > ~/.config/fish/completions/ws.fish
```

Or load it for the current shell only, which is also the quickest way to try a
change: `eval "$(ws completions zsh)"`.

**What it offers.** `ws <Tab>` lists the subcommands with their descriptions,
one per line; the options stay out of the way until the word starts with a
dash, where `ws --<Tab>` lists those the same way.

```
$ ws <Tab>
init         Write a ws.yaml in the current directory
fetch        Clone missing bare repositories and fetch all of them
switch       Check out a branch in every repository and open the session
...
```

zsh and fish are told the candidate and its description separately, so they do
all of this themselves. bash has one array for both jobs — whatever it is
handed is both what it shows and what it types onto the line — so the script
`ws completions bash` prints carries a description table and attaches it only
where bash is about to *list* the candidates: never for a lone match, and
never under `menu-complete`, where each candidate goes straight onto the line.
It is the same trick cobra plays, and about the only one available: the limit
is bash's, not the completion library's.

**Branch arguments** are completed by asking `ws`, rather than with file
names. `ws rm` and `ws path` name a worktree that exists, described by the
directory `ws rm` would delete and the repositories inside it:

```
$ ws rm <Tab>
feature/JIRA-123  feature-JIRA-123 — api, web
main              api, web
```

`ws switch` takes any branch of the workspace, so it offers all of them —
deduplicated across the repositories, the ones with worktrees first, then the
`origin/*` branches without, which is what the fzf picker shows too. The
description names the repositories the branch was found in:

```
$ ws switch <Tab>
main              checked out — api, web
feature/JIRA-123  checked out — api, web
feature/only-api  api
shared/thing      api, web
```

A branch that is still only on `origin` completes the same as one already
checked out, and `ws switch` with no argument still opens the picker.

`--from` is completed from the branches *and* the tags, each described by the
repositories it was found in. The `origin/` forms are the same branches again,
so they appear once that prefix is typed rather than doubling the list:

```
$ ws switch hotfix/x --from <Tab>
main              checked out — api, web
shared/thing      api, web
release/v0.4.5    tag — api
v1.0.0            tag — api, web

$ ws switch hotfix/x --from origin/<Tab>
origin/main          api, web
origin/shared/thing  api, web
```

Outside a workspace every one of these lists is empty rather than an error —
a shell asks on every <kbd>Tab</kbd>, wherever the prompt happens to be.

**In a dev shell.** Add `ws` to the shell's `packages` as above and bash is
done: bash-completion locates `share/bash-completion/completions/ws.bash` from
`PATH`, so the completion appears with the package and leaves with it.

zsh needs the directory on `fpath`, and a dev shell cannot put it there. A
`shellHook` runs under bash, so a zsh script generated and `eval`ed in one only
produces bash errors. `FPATH` is worse than useless: it is tied to `fpath`, and
assigning it in a *running* zsh — which is what direnv does when it exports the
environment — **replaces** `fpath` with that one directory instead of adding to
it, so every standard completion function disappears at once. Unsetting it on
the way out empties `fpath` altogether.

What a dev shell *can* do is say where its completions are: `mkShell` puts
every package's `share` directory on `XDG_DATA_DIRS`, the `ws` package's
`share/zsh/site-functions/_ws` included, and direnv exports that like any other
variable. [zsh-completion-sync](https://github.com/BronzeDeer/zsh-completion-sync)
watches it before every prompt, adds the new `site-functions` directories to
`fpath` and registers what they hold — so the completion of every package in
the shell appears on entering it and goes again on leaving. One line in
`~/.zshrc`, after `compinit` and the direnv hook, and nothing per project:

```zsh
znap source BronzeDeer/zsh-completion-sync     # or source its .plugin.zsh
```

Without a plugin, a completion that loads itself the first time you ask for
one does the same for `ws` alone:

```zsh
# ~/.zshrc, after compinit
_ws() {
  local script
  script="$(command ws completions zsh 2>/dev/null)" || return 1
  unfunction _ws
  eval "$script"
  _ws "$@"
}
compdef _ws ws
```

`ws` then has to exist only at the moment you press <kbd>Tab</kbd> — outside
a dev shell the stub simply yields no matches. It costs one
`ws completions zsh` per shell and covers `nix develop`, `nix develop -c zsh`
and direnv alike.

And for the shell you are in, by hand: `eval "$(ws completions zsh)"`, once
`compinit` has run.
