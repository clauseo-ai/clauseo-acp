//! The OS half of an agent's process tree: a process group on Unix, a Job Object on Windows.
//! Every OS call of the crate sits here.
//!
//! Each half has `prepare`, which sets up the command before it starts, and a `Tree`, which
//! `adopt` builds from the started child and which ends the tree with `kill` (and, on Unix only,
//! asks it to stop with `terminate`). Neither waits.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{Tree, prepare};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{Tree, prepare};
