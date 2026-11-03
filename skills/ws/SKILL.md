---
name: ws
description: Launch a task into a new ws branch directory with a fresh agent of the same kind working on it.
argument-hint: "[task]"
disable-model-invocation: true
---

# /ws — launch a task into its own workspace

The task is everything the user wrote after the skill name:

$ARGUMENTS

(In pi the placeholder above stays literal and the task follows after this skill text.)

You are the **launcher**: classify the task, create its branch directory, hand the task over verbatim and start a new agent there. The new agent does the work. Read only what classification needs: conventions and branch names, never the code. When a step says *stop*, report why and end; when it says *ask*, wait for the user's answer and continue. All asking happens in steps 1–6, before anything is created.

## 1. Workspace

Run `ws status`. If it fails, stop: "`/ws` only works inside a ws workspace (a directory with `ws.yaml` above it)." Note `root`, and read the files its `config` lines list, in order; a later file overrides an earlier one key by key. From them take the repos (a repo's name is its `name`, else the last segment of its `url` without `.git`), `default_branch` (default `main`), `bare_dir` (default `.bare`) and `mux.backend` (default `auto`).

## 2. Fetch

Run `ws fetch`. It also clones any missing bare repo. Each repo's bare clone is `<bare-dir>/<repo>.git`, where `<bare-dir>` is `bare_dir` resolved like ws does: `~` or `~/…` from `$HOME`, an absolute path as is, `.` the root, anything else relative to `root`. Below, `git --git-dir=<bare>` runs in it. Check each `<bare>` once with `git --git-dir=<bare> rev-parse --is-bare-repository`; unless it prints `true`, stop and show what it printed.

## 3. Agent kind

The new agent is the same kind as you. Run:

```sh
kinds=
[ "${CLAUDECODE:-}" = 1 ] && kinds="$kinds claude"
[ "${OPENCODE:-}" = 1 ] && kinds="$kinds opencode"
[ "${PI_CODING_AGENT:-}" = true ] && kinds="$kinds pi"
agent=''
case $kinds in " claude" | " opencode" | " pi") agent=${kinds# } ;; esac
pid=$$
while [ -z "$agent" ] && [ -n "$pid" ] && [ "$pid" -gt 1 ]; do
  comm=$(ps -o comm= -p "$pid" 2>/dev/null) || break
  arg0=$(ps -o args= -p "$pid" 2>/dev/null | sed 's/ .*//')
  for n in "${comm##*/}" "${arg0##*/}"; do
    case $n in
      claude | .claude-wrapped) agent=claude ;;
      opencode | .opencode-wrapped) agent=opencode ;;
      pi | .pi-wrapped) agent=pi ;;
    esac
  done
  pid=$(ps -o ppid= -p "$pid" | tr -d ' ')
done
echo "${agent:-unknown}"
```

Several markers at once means they leaked through a multiplexer, so the parent-process walk decides. On `unknown`, ask which agent to start.

| agent | launch command (`<pointer>` in single quotes) | user invokes `/ws` as |
|---|---|---|
| Claude Code | `env -u CLAUDECODE -u CLAUDE_CODE_CHILD_SESSION claude '<pointer>'` | `/ws <task>` |
| opencode | `opencode --prompt '<pointer>'` | `/ws <task>` |
| pi | `pi -- '<pointer>'` | `/skill:ws <task>` |

The pointer is exactly `Load the ws-workspace skill, then carry out the task in <branch-dir>/.ws/task.md.` with the absolute branch directory from step 7 filled in (inside the single quotes write a `'` in it as `'\''`). Plain agent defaults: copy no model, effort or permission flags.

## 4. Branch name

Derive it from what the task is about, by the first source that applies:

1. Explicit branch rules in the repos' `AGENTS.md`, `CLAUDE.md` or `CONTRIBUTING.md` (read them from a worktree, or `git --git-dir=<bare> show origin/<default_branch>:AGENTS.md`).
2. The pattern of existing remote branches: `git --git-dir=<bare> for-each-ref --format='%(refname:short)' refs/remotes/origin`.
3. `<type>/<slug>`: type `feat`, `fix`, `chore`, `docs` or `refactor`; slug a few lowercase hyphenated words.

When the convention requires a ticket ID and the task contains none, ask for it. Take IDs only from the task or the user.

## 5. Base

- The task names a base ref: that ref is `<base>`.
- Otherwise `<base>` is `origin/<default_branch>`. For each repo check the remote's own default: `git --git-dir=<bare> ls-remote --symref origin HEAD` prints `ref: refs/heads/<name>`. If any `<name>` differs from `default_branch`, stop and report which repo says what. If the command fails, stop and show its error.

## 6. Branch must be new

If `ws list` shows the branch, or any repo has it, stop and ask: the user decides whether to reuse it or pick another name. Per repo:

- `git --git-dir=<bare> show-ref --verify --quiet refs/heads/<branch>`: exit 0 means it exists, 1 means it does not.
- `git --git-dir=<bare> ls-remote --exit-code --heads origin <branch>`: exit 0 means it exists, 2 means it does not.

Any other exit status is an error, not an answer: stop and show it.

## 7. Create the branch directory

Inside a tmux client (`$TMUX` is set) with `mux.backend` `tmux` or `auto`, keep ws from switching the client yet; the switch comes last, in step 10. Use `mux.tmux.command` in place of `tmux` if the config sets one:

```sh
env -u TMUX WS_TMUX_CMD="tmux -S '${TMUX%%,*}'" ws switch '<branch>' --from '<base>' --mux tmux </dev/null
```

Otherwise:

```sh
ws switch '<branch>' --from '<base>'
```

No `--cd`: ws creates the worktrees, runs setup and opens the herdr or tmux session (herdr focuses it right away). A non-zero exit after `created worktrees` can still have opened the session (outside a tmux client ws ends with `open terminal failed: not a terminal`; some tmux layouts fail on a later option): carry on when `ws path '<branch>'` prints the branch directory, otherwise stop and show the error.

`<branch-dir>` is the absolute path `ws path '<branch>'` prints (the worktree itself in a single-repo workspace). Run `cd '<branch-dir>' && ws status`; its `session` line is the session name. herdr uses it as the workspace label as is. For tmux replace each `.`, `:`, `\` and control character with `-`; an empty name becomes `ws`.

## 8. Task file

Write the task verbatim to `<branch-dir>/.ws/task.md`: every character as the user gave it, no summary, no added heading. Use your file-writing tool, or a quoted heredoc with a delimiter that does not occur in the task:

```sh
mkdir -p '<branch-dir>/.ws'
cat > '<branch-dir>/.ws/task.md' <<'WS_TASK_END'
<task>
WS_TASK_END
```

Leave `.gitignore` and `.git/info/exclude` as they are. ws does not count this file as an uncommitted change, and `ws rm` deletes it with the branch directory.

## 9. Launch

Start the agent in the session's first pane: the first pane of the first tab or window. The pointer names the task file by absolute path, so the pane's directory does not matter.

**herdr** (`herdr workspace list` has a workspace labelled `<session>`):

```sh
W=$(herdr workspace list | jq -r --arg l '<session>' '.result.workspaces[] | select(.label == $l) | .workspace_id')
T=$(herdr tab list --workspace "$W" | jq -r '.result.tabs | min_by(.number) | .tab_id')
P=$(herdr pane list --workspace "$W" | jq -r --arg t "$T" --arg d '<branch-dir>' '[.result.panes[] | select(.tab_id == $t)] | (map(select(.cwd == $d)) + .)[0].pane_id')
herdr pane run "$P" "<launch command>"
```

That picks the tab's pane in the branch directory, else its first pane. Without `jq`, read the JSON yourself.

**tmux** (`tmux has-session -t '=<session>'` succeeds):

```sh
tmux send-keys -t '=<session>:^.{top-left}' -l -- "<launch command>"
tmux send-keys -t '=<session>:^.{top-left}' Enter
```

**No session** (`mux: none`): launch nothing. Print the branch directory and the command to run there, `cd '<branch-dir>' && <launch command>`, and skip step 10.

## 10. Switch

- herdr: `herdr workspace focus "$W"` (ws already focused it; this does it again).
- tmux inside a client: `tmux switch-client -t '=<session>'`.
- tmux outside a client: tell the user to run `tmux attach -t '=<session>'`.

Finish with one short report: branch, base, branch directory, agent started. A new folder may first show the agent's trust prompt in the new session; tell the user to confirm it there.
