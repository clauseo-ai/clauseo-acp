//! `ProcessReaders` end to end against the real `fake_agent` binary: a test launcher swaps the
//! helper's program and arguments for `fake_agent` on a scenario (or `--hold`), keeping the env
//! and the working folder the reader built, and records each spec. Each test sets up a fake
//! install and a data folder of its own, loads settings from it, and removes it at the end. No
//! test sleeps: a read ends with its process, and `ManualTimer` decides the deadline.
//!
//! Run `cargo build -p clauseo-fake_agent` first: `-p clauseo-limits` does not build it.
//!
//! A test crate root resolves modules beside itself, so each submodule names its path. Each
//! carries `#[cfg(test)]` so clippy treats its helpers as test code.

#[cfg(test)]
#[path = "readers/write.rs"]
mod write;

#[cfg(test)]
#[path = "readers/harness.rs"]
mod harness;

#[cfg(test)]
#[path = "readers/claude.rs"]
mod claude;

#[cfg(test)]
#[path = "readers/refusals.rs"]
mod refusals;

#[cfg(test)]
#[path = "readers/codex.rs"]
mod codex;

#[cfg(test)]
#[path = "readers/agents_layout.rs"]
mod agents_layout;
