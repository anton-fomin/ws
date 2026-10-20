//! The tools `ws` drives around a workspace, beyond git and the multiplexer.
//!
//! Each one is configured under its own `integrations.<name>` key and decides
//! for itself whether it applies: `enabled: auto` — the default — resolves
//! against whether the program is on `PATH` — for `local`, whether its
//! directory exists — and a package manager then still has to recognise the
//! repository before it does anything.
//!
//! None of them is allowed to fail a switch. A worktree is made by git; what
//! these add is convenience, so a missing tool, an unreadable directory or a
//! failed hard link is a warning and the switch carries on.

pub mod link;
pub mod local;
pub mod pm;
pub mod zoxide;

pub use local::Local;
pub use pm::PackageManagers;
pub use zoxide::Zoxide;
