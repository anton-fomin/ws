# fzf and zoxide

`ws pick` offers the branches already checked out, the `origin/*` branches that
are not, and — with `--global`, or outside a workspace — other workspace roots
from the zoxide database. Inside tmux the filter opens in a popup; the popup
runs `ws` itself, so the selection is acted on from inside the session.

Every switch registers the branch directory and its worktrees with zoxide, so
`z feature-JIRA` jumps straight there; `ws rm` removes them again. Set
`integrations.zoxide.enabled: false` to opt out.
