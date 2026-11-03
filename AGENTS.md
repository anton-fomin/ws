# AGENTS.md

Guidance for coding agents working in this repository.

`ws` is a Rust CLI for multi-repo git worktree workspaces: one bare clone per
repository, one branch directory per branch holding a worktree of each repo,
per-repo setup, and a tmux/herdr session laid over the result. The README is
the user-facing spec and is kept detailed; behaviour changes usually need a
README change too.

## Commands

On NixOS `rustc` is not in the system profile — run `nix develop` first (it
also provides git, tmux, fzf, zoxide).

```
cargo build
cargo test                                   # unit + e2e, no network
cargo test <name_substring>                  # one test, e.g. cargo test nested_branch_dirs
cargo test --test e2e <name>                 # only the end-to-end suite
cargo test --bin ws <name>                   # only unit tests (they live in src/)
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
shellcheck -s sh install.sh                  # install.sh is plain POSIX sh
```

CI runs fmt, clippy and tests with `RUSTFLAGS=-D warnings`, so warnings fail
the build. Tests also run on macOS (temp dirs are canonicalised because `/var`
is a symlink there).

The dev shell's `ws-completions <shell>` builds the working tree and prints its
completion script.

## Architecture

- `main.rs` dispatches the clap commands from `cli.rs`; `workspace.rs` holds
  the orchestration (fetch, switch, list, rm).
- **Every external process goes through `exec.rs`**: build a `Cmd`, run it with
  a `Runner`. Mutating calls (`Runner::run`, `capture_mut`) are suppressed by
  `--dry-run`; read-only queries (`capture`, `try_capture`, `ok`) always run so
  a dry run reports against real state. Command construction is kept separate
  from execution so unit tests can assert on command shapes — keep that split
  when adding git/mux/integration calls.
- `mux/`: a `Mux` trait (`tmux`, `herdr`, `none`) turning a `SessionPlan` into
  commands; `mux::select` picks one from config or `--mux`.
- `integration/`: zoxide, local files (`local/<repo>/` placed into worktrees),
  package managers, and task sources for `ws run`. Each resolves `enabled:
  auto` itself, and **none may fail a switch** — problems are warnings.
- `config.rs`: the global `~/.config/ws/config.yaml` (or `WS_CONFIG`; empty
  string disables it) is layered under the workspace `ws.yaml` by
  `merge_into` — mappings merge key by key, sequences and scalars replace.
  Structs use `deny_unknown_fields`. Workspace discovery walks up to `ws.yaml`,
  or follows a worktree's `.git` file back to the bare repo when worktrees live
  outside the root.
- State: which branch is where is always read from git, never stored.
  `.ws/state.yaml` (`state.rs`) only records which setup commands ran; the
  `.ws.yaml` marker (`marker.rs`) in a multi-repo branch directory records the
  branch and the ref it started `from`. Single-repo workspaces write no marker
  into the worktree.
- `setup.rs` expands `{root}`, `{branch}`, `{env:NAME}` etc. and runs each
  setup entry through `sh -e`; `run.rs` + `logs.rs` run tasks in parallel with
  prefixed, coloured output and size-rotated log files.
- Human-readable output goes to stderr, paths to stdout (`cd "$(ws path x)"`).

## Tests

Unit tests sit in `#[cfg(test)]` modules next to the code. `tests/e2e.rs`
builds local fixture git repos in temp dirs and stubs fzf, zoxide, package
managers and `just` with small shell scripts. Its `ws()` helper sets
`WS_CONFIG=""` and `--mux none` so the developer's own config and multiplexer
never leak in — use it (or `ws_with_global`) for new e2e tests.

## Releases

The version must agree in `Cargo.toml`, `Cargo.lock` and `flake.nix`, plus the
version pin examples in `README.md`. Pushing a `v*` tag triggers
`.github/workflows/release.yml`, which builds gnu and musl Linux and macOS
targets with `.sha256` files that `install.sh` verifies.

`skills/ws-workspace/SKILL.md` is the agent skill shipped for users of `ws`; keep it in
sync when commands change.

## Agent skills

### Issue tracker

Issues and specs live as local markdown under `.scratch/<feature>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Default five roles (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`), recorded as a `Status:` line. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `GLOSSARY.md` and `docs/adr/` at the repo root, created lazily. See `docs/agents/domain.md`.
