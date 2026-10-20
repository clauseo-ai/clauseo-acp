//! xtask's one door for changing files. Workspace `clippy.toml` bans every std call that changes
//! the file system (`disallowed-methods`), and this module and its submodules are the only xtask
//! code that may make one. Each operation is named after what it does and refuses paths outside
//! the place it belongs; it hands out no `File` to write through. Add a new operation here rather
//! than call std where the change is needed. xtask's tests write through `Scratch`
//! (`scratch.rs`).
//!
//! - `guard`: the tracked-file guard. Any tool that edits a tracked file saves its original,
//!   writes, and puts it back through `Guard`, and recovers an interrupted run's copies with
//!   `Backups`.
//! - `lock`: lock files under `target/xtask` that keep a second run of a tool out.
//! - `setup`: the staging dirs, run lock and installed binaries of `setup --binaries` in cargo's
//!   bin.

#![expect(
    clippy::disallowed_methods,
    reason = "xtask changes files only in this module; clippy.toml bans the std calls elsewhere"
)]

mod guard;
mod lock;
mod replace;
mod setup;

pub(crate) use guard::{Backups, Guard, GuardFailure};
#[cfg(test)]
pub(crate) use lock::LOCK_DIR;
pub(crate) use lock::{LockFile, create_lock_file, open_lock_file};
pub(crate) use setup::{
    RUN_LOCK, STAGING_PREFIX, Staging, open_run_lock, remove_staging_leftovers,
};
