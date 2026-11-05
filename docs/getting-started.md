# Getting started

`ws` checks one branch out in every repository of a project, runs each repo's
setup, and lays the result out in tmux or herdr. This page installs it, hooks
it into your shell and builds a first workspace.

## Install

On any Linux distribution or macOS, the install script downloads a prebuilt
binary from the latest [release](https://github.com/anton-fomin/ws/releases):

```
curl -fsSL https://raw.githubusercontent.com/anton-fomin/ws/main/install.sh | sh
```

It puts `ws` in `~/.local/bin`, without sudo, and says so if that directory is
not on `PATH`. It touches no rc files. Options go after `sh -s --`:

```
curl -fsSL https://raw.githubusercontent.com/anton-fomin/ws/main/install.sh | sh -s -- --version v0.7.0
```

| | |
|---|---|
| `--version <tag>` | a specific release instead of the latest |
| `--bin-dir <dir>` | install somewhere else, e.g. `/usr/local/bin`; also `WS_INSTALL_DIR`. Uses sudo only when the directory is not writable |
| `--libc musl\|gnu` | Linux only. The default is the static musl build, which runs on every distribution; `gnu` is the glibc build and needs glibc 2.39 or newer |

The script picks the build for the machine — `x86_64` or `aarch64` Linux,
Apple Silicon or Intel macOS — checks it against the release's `.sha256` file
and refuses a mismatch, and tries the new binary before it replaces the old
one. It needs `curl` or `wget`, `tar`, and `sha256sum` or `shasum`. `git` has
to be installed for `ws` to be any use; everything else it drives is an
optional [integration](integrations/index.md).

**Upgrade** by running the same line again. **Uninstall** with
`rm ~/.local/bin/ws`.

**To read the script first**, download it rather than piping it — every
release also carries the `install.sh` it was published with:

```
curl -fsSLO https://raw.githubusercontent.com/anton-fomin/ws/main/install.sh
less install.sh
sh install.sh
```

**By hand**, download `ws-<tag>-<target>.tar.gz` and its `.sha256` from the
release page, then:

```
sha256sum -c ws-v0.7.0-x86_64-unknown-linux-musl.tar.gz.sha256   # shasum -a 256 -c on macOS
tar -xzf ws-v0.7.0-x86_64-unknown-linux-musl.tar.gz
install -m 755 ws ~/.local/bin/ws
```

Releases before 0.7.0 have only an `x86_64-unknown-linux-gnu` build and no
`.sha256` files, so the script cannot install them.

### On NixOS

The repository ships a flake, so `ws` installs straight from it. Flakes must be
enabled: `nix.settings.experimental-features = [ "nix-command" "flakes" ];`

**Try it without installing**

```
nix run github:anton-fomin/ws -- --help
```

**Per user**

```
nix profile add github:anton-fomin/ws
```

Older Nix spells that `nix profile install`. Afterwards `nix profile upgrade ws`
(or `--all`) updates it and `nix profile remove ws` drops it.

**System-wide, as a flake input**

```nix
# flake.nix of your NixOS configuration
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    ws = {
      url = "github:anton-fomin/ws";
      inputs.nixpkgs.follows = "nixpkgs";   # one nixpkgs, not two
    };
  };

  outputs = { nixpkgs, ws, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./configuration.nix
        ({ pkgs, ... }: {
          environment.systemPackages = [
            ws.packages.${pkgs.system}.default
            pkgs.git                              # required at runtime
            pkgs.tmux pkgs.fzf pkgs.zoxide        # optional, see Integrations
          ];
        })
      ];
    };
  };
}
```

Then `nixos-rebuild switch --flake .#myhost`, and `nix flake update ws` to pull
a newer version.

Under Home Manager it is the same expression in `home.packages`.

**Pin to a release** by putting the tag in the URL:

```
github:anton-fomin/ws/v0.7.0
```

**In a project's dev shell**

`ws` is normally a per-user tool, but putting it in a `devShell` pins one
version for everyone working on the project:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    ws = {
      url = "github:anton-fomin/ws";   # or github:anton-fomin/ws/v0.7.0
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, ws, ... }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      devShells.${system}.default = pkgs.mkShell {
        packages = [
          ws.packages.${system}.default
          pkgs.git                              # required at runtime
          pkgs.tmux pkgs.fzf pkgs.zoxide        # optional, see Integrations
        ];
      };
    };
}
```

`nix develop` then has `ws` on `PATH`, and so do the tmux windows it opens
from there. Completion comes along with the package — see
[Shell completion](shell-completion.md) for the one line zsh needs. For several
systems wrap the outputs in `nixpkgs.lib.genAttrs` — this repository's own
`flake.nix` does exactly that.

The shell is only as wide as the project, though: outside it `ws` is gone, and
it cannot be what finds the *other* workspaces for `ws pick --global`. Install
it per user too if you want it everywhere.

## Shell wrapper and completion

A program cannot change its parent shell's directory, so `ws switch` needs a
small shell function to `cd` into the branch directory it opens. Add it to
your rc file:

```
eval "$(ws shell-init zsh)"     # ~/.zshrc; the same with bash in ~/.bashrc
ws shell-init fish | source     # ~/.config/fish/config.fish
```

The function is called `ws` too and passes everything else through to the
binary. Then set up [shell completion](shell-completion.md), which completes
subcommands, flags and branch names.

## Your first workspace

A workspace is a directory holding a `ws.yaml`. Write your personal defaults
once, then create a workspace for a project by naming its repositories:

```
ws init --global                                   # once: your settings
mkdir acme && cd acme
ws init --repo git@github.com:org/api.git --repo git@github.com:org/web.git
ws fetch                                           # bare clones of both
ws switch feature/JIRA-123                         # a worktree of each, side by side
```

`ws switch` creates `feature-JIRA-123/api` and `feature-JIRA-123/web`, places
your [local files](integrations/local-files.md), installs dependencies, runs
each repository's `setup`, and opens a tmux or herdr session over the branch
directory. `ws status` tells you where you are; `ws list` shows every branch
directory.

Next: how the directories are arranged in [Layout](layout.md), every command
in [Commands](commands.md), and what goes in `ws.yaml` in
[Configuration](configuration.md).
