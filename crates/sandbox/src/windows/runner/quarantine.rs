//! The unclean-slot mark: `<data_dir>\sandbox\run\<pool user's SID>.unclean`. A session whose
//! outcome is `RunExit::Escapee` (a sweep found a process of the pool user's it couldn't end, a
//! survivor of its kill among them) leaves its pool user marked, and the mark holds the slot
//! until a fresh sweep verifies it clean:
//!
//! - **The next launch on the slot** ([`admit`]): before anything of the new run is granted to
//!   the pool user (its pipes, its window station and desktop, which name the pool user and the
//!   matter), the outer runs a verify-only pass: `--verify`, the end sweeper's two passes as the
//!   pool user with no job, nothing kept but itself and no removal (`sweeper_flow::verify`). Only
//!   its exit 0 clears the mark, and only then does the launch go on. Any other outcome refuses
//!   the launch with `RunExit::Quarantined` and the lawyer's first action, to restart the
//!   computer ([`RESTART`]); the mark stays. A mark that can't be read counts as one.
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
    /// Removes the mark.
    fn clear(&mut self) -> io::Result<()>;
}

/// Why [`admit`] refused: the verify-only pass on a marked slot failed.
#[cfg(any(windows, test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Quarantined {
    /// What the pass said went wrong.
    pub(crate) why: String,
}

/// Runs `launch` (everything that grants the new run anything) only once the slot is clean: a
/// slot that isn't marked at once; a marked one only after its verify-only pass passed, which
/// clears the mark. A pass that failed refuses: `launch` never runs, and the mark stays. A mark
/// that can't be removed after a pass that passed is logged; the launch goes on, and the next
/// launch verifies again.
#[cfg(any(windows, test))]
pub(crate) fn admit<S: Slot, T>(
    slot: &mut S,
    log: &mut dyn FnMut(&str),
    launch: impl FnOnce() -> T,
) -> Result<T, Quarantined> {
    if slot.unclean() {
        log(
            "clauseo-sandbox-run: the sandbox user was left unclean by an earlier session: \
             verifying no process of it is left before the launch",
        );
        slot.verify().map_err(|why| Quarantined { why })?;
        if let Err(error) = slot.clear() {
            log(&format!(
                "clauseo-sandbox-run: the unclean mark's removal failed ({error})"
            ));
        }
        log("clauseo-sandbox-run: the sandbox user is clean again");
    }
    Ok(launch())
}

#[cfg(test)]
mod tests;
