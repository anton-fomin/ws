//! Fallback when no multiplexer is in play.

use anyhow::Result;

use crate::config::OnNone;
use crate::exec::Runner;
use crate::mux::{Mux, SessionPlan};

pub struct None_ {
    on_none: OnNone,
}

impl None_ {
    pub fn new(on_none: OnNone) -> None_ {
        None_ { on_none }
    }
}

impl Mux for None_ {
    fn name(&self) -> &'static str {
        "none"
    }

    fn open(&self, _runner: &Runner, plan: &SessionPlan) -> Result<()> {
        if self.on_none == OnNone::PrintPath {
            // stdout is the machine-readable channel: `cd "$(ws switch x --cd)"`
            println!("{}", plan.root_dir.display());
        }
        Ok(())
    }

    fn close(&self, _runner: &Runner, _session: &str) -> Result<()> {
        Ok(())
    }
}
