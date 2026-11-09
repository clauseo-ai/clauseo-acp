//! The unclean-slot mark: `<data_dir>\sandbox\run\<pool user's SID>.unclean`. It is written
//! first and removed last: the outer writes it before anything of a session runs as the pool user
//! ([`admit`]; a mark that can't be written refuses the launch), and removes it only at a clean
//! end ([`ended`]: the inner exited clean, the end sweeper exited 0 while the outer waited, and no
//! sweep found an escapee). Any other end leaves it: a sweep that ran out of rounds or time, an
//! escapee or a survivor, a failure, a cancel, an outer that was killed, an end sweeper still
//! running when the outer stopped waiting. The mark holds the slot until a fresh sweep verifies
//! it clean:
//!
//! - **The next launch on the slot** ([`admit`]): before anything of the new run is granted to
//!   the pool user (its pipes, its window station and desktop, which name the pool user and the
//!   matter), the outer runs a verify-only pass: `--verify`, the end sweeper's two passes as the
//!   pool user with no job, nothing kept but itself and no removal (`sweeper_flow::verify`),
//!   started suspended and shut as the end sweeper is (`token::runner_process_sddl`). Only its
//!   exit 0 lets the launch go on, the mark kept for this session. Any other outcome refuses the
//!   launch with `RunExit::Quarantined` and the lawyer's first action, to restart the computer
//!   ([`RESTART`]); the mark stays. A mark that can't be read counts as one. The pass runs as the
//!   pool user, so its exit code is the pool user's word: a Medium survivor that opened it before
//!   its shut could end it with 0. Only a check from `SYSTEM`, which reads every process's SID,
//!   closes that (0049, C5).
//! - **Another matter** (`pool::assign`): a marked pool user is held back (`Why::Unclean`), so a
//!   matter never gets a slot with a process that outlived its sweep; only the matter that holds
//!   it launches on it, and its next launch's verify-only pass clears it.
//!
//! The mark is written as the session marker is (`marker.rs`): in the run folder, which only the
//! desktop user, `SYSTEM` and `Administrators` may open, atomically, then given the folder's
//! three entries as its own protected DACL and the desktop user as its owner. The pool user can
//! neither write nor remove it. It holds the pool user's SID, which its name gives too.
//!
//! The names, the paths and the listing hold no Windows call, so their tests run on every OS.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use super::RUN_DIR;
use crate::windows::matters::SANDBOX_DIR;
use crate::windows::sids::Sid;
#[cfg(any(windows, test))]
use crate::windows::write;

/// The mark's suffix, after the pool user's SID.
pub const UNCLEAN_SUFFIX: &str = ".unclean";
/// What the outer says when it refuses a launch on a marked slot whose verify-only pass failed:
/// the ordinary message, then the lawyer's first action.
#[cfg(any(windows, test))]
pub(crate) const RESTART: &str = "the sandbox user still has a process from an earlier session \
                                  that couldn't be ended; restart the computer, then try again";

/// The mark's path for `pool_user`: `<data_dir>\sandbox\run\<SID>.unclean`.
pub fn unclean_path(data_dir: &Path, pool_user: &Sid) -> PathBuf {
    data_dir
        .join(SANDBOX_DIR)
        .join(RUN_DIR)
        .join(format!("{}{UNCLEAN_SUFFIX}", pool_user.as_str()))
}

/// The pool users marked unclean in `<data_dir>\sandbox\run\`, by the marks' names; none when
/// the folder is missing. A folder that can't be read is an error, which the caller takes as
/// every user marked.
pub fn unclean_users(data_dir: &Path) -> io::Result<BTreeSet<Sid>> {
    let run = data_dir.join(SANDBOX_DIR).join(RUN_DIR);
    let entries = match std::fs::read_dir(&run) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => return Err(error),
    };
    let mut users = BTreeSet::new();
    for entry in entries {
        let name = entry?.file_name();
        if let Some(sid) = name
            .to_str()
            .and_then(|name| name.strip_suffix(UNCLEAN_SUFFIX))
            .and_then(|sid| Sid::parse(sid).ok())
        {
            users.insert(sid);
        }
    }
    Ok(users)
}

/// Marks `pool_user` unclean; its path.
#[cfg(any(windows, test))]
pub(crate) fn mark_unclean(data_dir: &Path, pool_user: &Sid) -> io::Result<PathBuf> {
    let path = unclean_path(data_dir, pool_user);
    write::write_atomic(&path, pool_user.as_str().as_bytes())?;
    write::check_owned_file(&path)?;
    #[cfg(windows)]
    crate::windows::private_dir::own_file(&path)?;
    Ok(path)
}

/// Whether `pool_user` is marked; one whose mark can't be looked at is (failing closed).
#[cfg(any(windows, test))]
pub(crate) fn is_unclean(data_dir: &Path, pool_user: &Sid) -> bool {
    match std::fs::symlink_metadata(unclean_path(data_dir, pool_user)) {
        Ok(_) => true,
        Err(error) => error.kind() != io::ErrorKind::NotFound,
    }
}

/// Removes `pool_user`'s mark; one already gone is fine.
#[cfg(any(windows, test))]
pub(crate) fn clear_unclean(data_dir: &Path, pool_user: &Sid) -> io::Result<()> {
    write::remove_owned_file(&unclean_path(data_dir, pool_user))
}

/// The slot's mark and its verify-only pass, behind a trait so the tests can follow the order.
#[cfg(any(windows, test))]
pub(crate) trait Slot {
    /// Whether the slot is marked unclean.
    fn unclean(&mut self) -> bool;
    /// The verify-only pass: `Ok` once a fresh sweep of the pool user's processes found none
    /// left, else what went wrong.
    fn verify(&mut self) -> Result<(), String>;
    /// Writes the mark.
    fn mark(&mut self) -> io::Result<()>;
    /// Removes the mark.
    fn clear(&mut self) -> io::Result<()>;
}

/// Why [`admit`] refused.
#[cfg(any(windows, test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refused {
    /// The verify-only pass on a marked slot failed: what it said went wrong.
    Quarantined(String),
    /// The mark couldn't be written: why.
    Unmarked(String),
}

/// Runs `launch` (everything of the session: what grants the pool user anything, and the agent)
/// only once the slot is clean and marked: a marked slot only after its verify-only pass passed;
/// then the mark is written (again), so any end but a clean one ([`ended`]) leaves it, a killed
/// outer's too. A pass that failed, or a mark that can't be written, refuses: `launch` never
/// runs.
#[cfg(any(windows, test))]
pub(crate) fn admit<S: Slot, T>(
    slot: &mut S,
    log: &mut dyn FnMut(&str),
    launch: impl FnOnce() -> T,
) -> Result<T, Refused> {
    if slot.unclean() {
        log(
            "clauseo-sandbox-run: the sandbox user was left unclean by an earlier session: \
             verifying no process of it is left before the launch",
        );
        slot.verify().map_err(Refused::Quarantined)?;
        log("clauseo-sandbox-run: the verify pass found no process of the sandbox user");
    }
    slot.mark()
        .map_err(|error| Refused::Unmarked(error.to_string()))?;
    Ok(launch())
}

/// The session's end: a `clean` one removes the mark (one that can't be removed is logged, and
/// the next launch verifies); any other leaves it.
#[cfg(any(windows, test))]
pub(crate) fn ended<S: Slot>(slot: &mut S, clean: bool, log: &mut dyn FnMut(&str)) {
    if clean && let Err(error) = slot.clear() {
        log(&format!(
            "clauseo-sandbox-run: the unclean mark's removal failed ({error})"
        ));
    }
}

#[cfg(test)]
mod tests;
