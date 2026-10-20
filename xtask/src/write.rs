//! xtask's one door for changing files. Workspace `clippy.toml` bans every std call that changes
//! the file system (`disallowed-methods`), and this module and its submodules are the only xtask
//! code that may make one. Each operation is named after what it does; add a new one here rather
//! than call std where the change is needed. xtask's tests write through `Scratch`
//! (`scratch.rs`).
//!
//! - `guard`: the tracked-file guard. Any tool that edits a tracked file saves its original,
//!   writes, and puts it back through `Guard`, and recovers an interrupted run's copies with
//!   `Backups`.
//! - `lock`: lock files that keep a second run of a tool out.
//! - `setup`: the staging dirs, run lock and installed binaries of `setup --binaries`.

#![expect(
    clippy::disallowed_methods,
    reason = "xtask changes files only in this module; clippy.toml bans the std calls elsewhere"
)]

mod guard;
mod lock;
mod setup;

pub(crate) use guard::{Backups, Guard, GuardFailure, discard_interrupted_save};
pub(crate) use lock::{create_lock_file, open_lock_file};
pub(crate) use setup::{
    install_executable, open_run_lock, remove_staging_dir, remove_staging_leftovers,
    reset_staging_dir,
};
