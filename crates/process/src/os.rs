//! The OS half of an agent's process tree: a process group on Unix, a Job Object on Windows.
//! Every OS call of the crate sits here.
//!
//! Each half has `prepare`, which sets up the command before it starts, and a `Tree`, which
//! `adopt` builds from the started child and which ends the tree with `kill` (and, on Unix only,
//! asks it to stop with `terminate`). Neither waits. Each half also has a `Root`, which lists
//! the agent's descendants (`linux.rs` and `macos.rs` read them for Unix; Windows reads the job's
//! process list), and the call that ends one listed process once its start is checked again.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{Ask, Root, Tree, now, prepare, signal};
#[cfg(all(unix, feature = "test-support"))]
pub(crate) use unix::{kill_pid, running, started_before_now};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{Root, Tree, now, prepare, terminate};
#[cfg(all(windows, feature = "test-support"))]
pub(crate) use windows::{kill_pid, running, started_before_now};
