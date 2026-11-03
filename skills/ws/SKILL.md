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

You are the **launcher**: classify the task, create its branch directory, hand the task over verbatim and start a new agent there. The new agent does the work. Read only what classification needs: conventions and branch names, never the code. When a step says *stop*, report why and end; when it says *ask*, wait for the user's answer and continue.

## 1. Workspace

Run `ws status`. If it fails, stop: "`/ws` only works inside a ws workspace (a directory with `ws.yaml` above it)." Note `root`, and read `<root>/ws.yaml` (layered over `~/.config/ws/config.yaml`) for the repos, `default_branch` (default `main`) and `bare_dir` (default `.bare`). Each repo's bare clone is `<root>/<bare_dir>/<repo>.git`; below, `git --git-dir=<bare>` runs in it.

## 2. Branch name

Derive it from what the task is about, by the first source that applies:

1. Explicit branch rules in the repos' `AGENTS.md`, `CLAUDE.md` or `CONTRIBUTING.md` (read them from a worktree, or `git --git-dir=<bare> show origin/<default_branch>:AGENTS.md`).
2. The pattern of existing remote branches: `git --git-dir=<bare> for-each-ref --format='%(refname:short)' refs/remotes/origin`.
3. `<type>/<slug>`: type `feat`, `fix`, `chore`, `docs` or `refactor`; slug a few lowercase hyphenated words.

When the convention requires a ticket ID and the task contains none, ask for it. Take IDs only from the task or the user.

## 3. Base

- The task names a base ref: that ref is `<base>`.
- Otherwise run `ws fetch`; `<base>` is `origin/<default_branch>`. For each repo check the remote's own default: `git --git-dir=<bare> ls-remote --symref origin HEAD` prints `ref: refs/heads/<name>`. If any `<name>` differs from `default_branch`, stop and report which repo says what.

## 4. Branch must be new

If `ws list` shows the branch, or any repo has it (`git --git-dir=<bare> show-ref --verify refs/heads/<branch>`, or `git --git-dir=<bare> ls-remote --exit-code --heads origin <branch>` succeeds), stop and ask: the user decides whether to reuse it or pick another name.

## 5. Create the branch directory

```sh
ws switch '<branch>' --from '<base>'
```

No `--cd`: ws creates the worktrees, runs setup and opens the herdr or tmux session. A non-zero exit after `created worktrees` can still have opened the session (outside a tmux client ws ends with `open terminal failed: not a terminal`; some tmux layouts fail on a later option): carry on when `ws path '<branch>'` prints the branch directory, otherwise stop and show the error.

Then, from the branch directory: `cd "$(ws path '<branch>')" && ws status`. Its `session` line is the session name: the herdr workspace label as is; for tmux replace `.` and `:` with `-`.

## 6. Task file

Write the task verbatim to `<branch-dir>/.ws/task.md`: every character as the user gave it, no summary, no added heading. Use your file-writing tool, or a quoted heredoc with a delimiter that does not occur in the task:

```sh
mkdir -p '<branch-dir>/.ws'
cat > '<branch-dir>/.ws/task.md' <<'WS_TASK_END'
<task>
WS_TASK_END
```

Leave `.gitignore` and `.git/info/exclude` as they are.

## 7. Agent kind

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

The pointer is exactly: `Load the ws-workspace skill, then carry out the task in .ws/task.md.` Plain agent defaults: copy no model, effort or permission flags.

## 8. Launch and switch

Start the agent in the session's root pane, whose working directory is the branch directory (the worktree in a single-repo workspace), so the relative `.ws/task.md` resolves.

**herdr** (`herdr workspace list` has a workspace labelled `<session>`):

```sh
W=$(herdr workspace list | jq -r --arg l '<session>' '.result.workspaces[] | select(.label == $l) | .workspace_id')
T=$(herdr tab list --workspace "$W" | jq -r '.result.tabs | min_by(.number) | .tab_id')
P=$(herdr pane list --workspace "$W" | jq -r --arg t "$T" --arg d '<root-dir>' '[.result.panes[] | select(.tab_id == $t and .cwd == $d)][0].pane_id')
herdr pane run "$P" "<launch command>"
```

Without `jq`, read the JSON yourself. ws already focused that workspace; `herdr workspace focus "$W"` does it again if needed.

**tmux** (`tmux has-session -t '=<session>'` succeeds):

```sh
tmux send-keys -t '=<session>:^.{top-left}' -l -- "<launch command>"
tmux send-keys -t '=<session>:^.{top-left}' Enter
```

Inside a tmux client ws already switched it; otherwise tell the user to run `tmux attach -t '=<session>'`.

**No session** (`mux: none`): launch nothing. Print the branch directory and the command to run there, `cd '<branch-dir>' && <launch command>`.

Finish with one short report: branch, base, branch directory, agent started. A new folder may first show the agent's trust prompt in the new session; tell the user to confirm it there.
