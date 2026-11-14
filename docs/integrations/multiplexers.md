# Multiplexers

* **tmux** — one window per repository named after it, plus a shared window for
  the branch directory when there are several repos. Inside tmux the current
  client is switched; outside, `ws` hands the terminal to `attach-session`.
* **herdr** — one workspace per branch labelled like the tmux session, one tab
  per repository plus one for the branch directory, mirroring the tmux
  windows. Tab labels start with their one-based position and a middle dot:
  `1 · ws`, `2 · api`, `3 · web`. A single-repo workspace has `1 · <repo>`.
  `mux.herdr.tab_name` customizes the label (see below).
  These labels are set when creating a workspace; existing workspaces are only
  focused, not renamed. `[mux.herdr] layout = "panes"` puts them side by side in a single
  tab instead, and `split_direction` then says where each one goes. Command
  shapes are verified against herdr 0.9; ids are read back from the JSON
  `.result` rather than constructed.
* **none** — prints the branch directory instead.

`backend = "auto"` picks whichever one `ws` is running inside, then whichever is
installed.

## herdr tab names

Set `mux.herdr.tab_name` in `ws.yaml` or your global config:

```yaml
mux:
  herdr:
    tab_name: "{number} · {name}"  # default
```

`{number}` is the one-based position at creation, with the shared branch tab
first when present. `{name}` is the repository name or `mux.root_window` for
the shared tab. Other text is kept literally. For unnumbered labels, use
`tab_name: "{name}"`; for another separator, use `"{number} - {name}"`.

This only affects newly created workspaces in the `tabs` layout. Existing
workspaces keep their labels, and labels are not renumbered after tabs are
reordered or closed. The `panes` layout and tmux names are unchanged.
