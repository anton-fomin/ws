{
  description = "ws — multi-repo git worktree workspaces";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAll = f: nixpkgs.lib.genAttrs systems (s: f nixpkgs.legacyPackages.${s});
    in {
      packages = forAll (pkgs: {
        default = pkgs.rustPlatform.buildRustPackage {
          pname = "ws";
          version = "0.2.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          # The end-to-end tests build fixture repositories with git.
          nativeCheckInputs = [ pkgs.git ];

          nativeBuildInputs = [ pkgs.installShellFiles ];
          # `ws completions` prints the script, so the binary itself is the
          # source of truth — skipped when it cannot run on the build machine.
          postInstall = pkgs.lib.optionalString
            (pkgs.stdenv.buildPlatform.canExecute pkgs.stdenv.hostPlatform) ''
              installShellCompletion --cmd ws \
                --bash <($out/bin/ws completions bash) \
                --zsh  <($out/bin/ws completions zsh) \
                --fish <($out/bin/ws completions fish)
            '';
        };
      });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [ rustc cargo clippy rustfmt git tmux fzf zoxide ];

          # Completion for the `ws` in the working tree, built on demand so
          # that entering the shell stays cheap:
          #   eval "$(ws-completions zsh)"
          shellHook = ''
            ws-completions() {
              cargo run --quiet -- completions "''${1:-bash}"
            }
          '';
        };
      });
    };
}
