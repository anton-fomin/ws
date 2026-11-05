# Multiplexers

* **tmux** — one window per repository named after it, plus a shared window for
  the branch directory when there are several repos. Inside tmux the current
  client is switched; outside, `ws` hands the terminal to `attach-session`.
* **herdr** — one workspace per branch labelled like the tmux session, one tab
  per repository plus one for the branch directory, mirroring the tmux
  windows. `[mux.herdr] layout = "panes"` puts them side by side in a single
  tab instead, and `split_direction` then says where each one goes. Command
  shapes are verified against herdr 0.9; ids are read back from the JSON
  `.result` rather than constructed.
* **none** — prints the branch directory instead.

`backend = "auto"` picks whichever one `ws` is running inside, then whichever is
installed.
