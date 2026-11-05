//! `clauseo-process` against the real `fake_agent` binary over pipes: the shutdown ladder, the
//! tree kill, the exit report and the descendant sweep, on every OS CI runs. No test sleeps: each
//! step waits for a stderr line, a pipe or an exit, and `ManualTimer` decides when the ladder's
//! waits end. The one exception is in `sweep.rs`: a child the sweep kills is a zombie no exit
//! event reports, so it asks the OS whether the child still runs, under a deadline.
//!
//! A test crate root resolves modules beside itself, so each submodule names its path. Each
//! carries `#[cfg(test)]` so clippy treats its helpers as test code.

#[cfg(test)]
#[path = "pipes/harness.rs"]
mod harness;

#[cfg(test)]
#[path = "pipes/write.rs"]
mod write;

#[cfg(test)]
#[path = "support/exit_watch.rs"]
mod exit_watch;

#[cfg(test)]
#[path = "pipes/shutdown.rs"]
mod shutdown;

#[cfg(test)]
#[path = "pipes/tree.rs"]
mod tree;

#[cfg(test)]
#[path = "pipes/sweep.rs"]
mod sweep;
