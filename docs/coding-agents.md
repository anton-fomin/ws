# Coding agents

The repository ships two skills for Claude Code, OpenCode and pi:

- [`skills/ws/SKILL.md`](../skills/ws/SKILL.md), **`/ws <task>`**, launches a
  task into its own workspace. Type it in an agent session anywhere inside a
  workspace (in pi: `/skill:ws <task>`). The agent picks a branch name from
  the repositories' conventions (`AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md`,
  then the pattern of the remote branches, then `<type>/<slug>`), branches
  from the ref the task names or else from `origin/<default_branch>` after
  `ws fetch`, and runs `ws switch`. It saves the task verbatim to
  `<branch-dir>/.ws/task.md`, starts the same kind of agent in the session's
  first pane with a pointer to that file, and leaves you in the new session.
  It stops and asks when the branch already exists or the branch convention
  needs a ticket ID the task lacks; without a multiplexer it prints the
  directory and the command to run. Only you invoke it.
- [`skills/ws-workspace/SKILL.md`](../skills/ws-workspace/SKILL.md) teaches an
  agent working inside a workspace how to find its way around: `ws status` to
  orient, each repository's own `AGENTS.md` or `CLAUDE.md`, `ws switch --cd`
  instead of `git switch`, `ws run` for tasks, and which local files are
  shared symlinks. Agents load it on their own, and the agent `/ws` starts is
  told to. `ws init` points here until an agent has it.

`/ws` adds `.ws/task.md` to no ignore file. In a single-repo workspace the
file sits inside the worktree, so `git status` lists it as untracked unless
you ignore `.ws/` yourself. `ws` treats it like its own marker: `ws list` and
`ws rm` do not count it (or a `.ws/` holding only it) as an uncommitted
change, and `ws rm` deletes it, and `.ws/` once empty, with the branch
directory. Anything else you put in `.ws/` still counts.

Install both once per user with the [`skills`](https://skills.sh) CLI,
naming the agents you use:

```
npx skills add anton-fomin/ws -g -a claude-code opencode
```

That puts the skills in `~/.agents/skills/`, which OpenCode and pi read, and
links them into `~/.claude/skills/` for Claude Code. `-a` takes any agent the
CLI knows (`codex`, `cursor`, `gemini-cli`, …). It installs from `main`;
`npx skills update -g` fetches the current version.

**Installed the skill before `/ws` existed?** The skill called `ws` was the
in-workspace guide, now `ws-workspace`. `npx skills update -g` only refreshes
skills you already have, so it replaces the guide with the launcher and never
adds `ws-workspace`. Run the `npx skills add` line above again to get both; with a
manual copy, add the `skills/ws-workspace` directory too.

**Without Node**, copy the `skills/ws` and `skills/ws-workspace` directories
of a checkout into the agent's skill directory, or symlink them so that
`git pull` keeps them current:

| agent | skill directory |
|---|---|
| Claude Code | `~/.claude/skills/` (`$CLAUDE_CONFIG_DIR/skills/` when that is set) |
| OpenCode | `~/.config/opencode/skills/`; it also reads `~/.claude/skills` and `~/.agents/skills` |
| pi | `~/.pi/agent/skills/` (`$PI_CODING_AGENT_DIR/skills/` when that is set); it also reads `~/.agents/skills` |
