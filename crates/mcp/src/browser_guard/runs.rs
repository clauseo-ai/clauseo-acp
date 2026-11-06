//! Each guard run's own names under `--cwd`, so one run never deletes or kills another's. An MCP
//! reconnect starts a new guard under the same `--cwd` while the killed guard's reaper may still
//! be cleaning up; with names shared by the session's runs, that reaper deleted the new run's
//! profile and could end its daemon (decision record 0029, "Each run's own names").
//!
//! - The run's id, `<guard pid>-<8 hex>`: the hex is random, and the lock file below, made with
//!   `create_new`, keeps two runs from ever sharing an id.
//! - Its profile `<cwd>/chrome-profile-<id>`, with Chrome's downloads staging folder inside
//!   (`chrome.rs`), and on Unix its agent-browser session name `run-<id>`, which names the
//!   daemon's socket `<socket dir>/run-<id>.sock` (`reaper::kill_daemon`). Windows keeps the
//!   host's session name: the daemon's port derives from it and the host claimed that port. No
//!   reaper runs there; the guard's Job Object ends the daemon.
//! - Its lock file `<cwd>/chrome-profile-<id>.lock`, held shared by the guard and by its reaper
//!   (`File::lock_shared`) for as long as either runs. The OS drops a lock when its holder dies,
//!   so a reused pid never holds it.
//!
//! At its start each guard sweeps. A run whose lock file it can lock exclusively has neither a
//! guard nor a reaper left: its profile is deleted, then its lock file. A run whose lock is held
//! is left alone, whatever its pid. A profile with no lock file is stale too: a run makes its
//! lock file before its profile and deletes it only after. The sweep, and the making and locking
//! of the new run's lock file, happen under one exclusive lock on `<cwd>/chrome-profiles.lock`,
//! so no sweep ever finds a lock file its run does not hold yet.

use std::collections::BTreeSet;
use std::fs::File;
use std::hash::{BuildHasher, RandomState};
use std::io;
use std::path::{Path, PathBuf};

use crate::write;

/// The start of every run's profile and lock file names under `--cwd`.
pub(crate) const PROFILE_PREFIX: &str = "chrome-profile-";
/// The end of a run's lock file name.
const LOCK_SUFFIX: &str = ".lock";
/// The lock file every run under one `--cwd` shares, for the sweep.
pub(crate) const SWEEP_LOCK: &str = "chrome-profiles.lock";
/// How many ids a run tries before it gives up.
const ID_TRIES: u64 = 16;

/// The guard's run: its id, its profile, and its share of its lock, held until it drops.
pub(crate) struct Run {
    /// Unix: the reaper's `--run` and the session name's; Windows needs only the profile.
    #[cfg(any(unix, test))]
    id: String,
    profile: PathBuf,
    lock_path: PathBuf,
    /// Locked shared; dropping it releases the lock.
    _lock: File,
}

impl Run {
    /// Sweeps the stale runs under `cwd`, then makes and locks a new run's lock file.
    pub(crate) fn claim(cwd: &Path) -> io::Result<Run> {
        let sweep_lock = write::open_or_create(&cwd.join(SWEEP_LOCK))?;
        sweep_lock.lock()?;
        sweep(cwd);
        let pid = std::process::id();
        for attempt in 0..ID_TRIES {
            // The high half of a randomly keyed hash: 8 hex digits.
            let id = format!("{pid}-{:08x}", RandomState::new().hash_one(attempt) >> 32);
            let lock_path = lock_of(cwd, &id);
            let lock = match write::create_new(&lock_path) {
                Ok(lock) => lock,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            };
            if let Err(error) = lock.lock_shared() {
                drop(write::remove_file(&lock_path));
                return Err(error);
            }
            return Ok(Run {
                profile: profile_of(cwd, &id),
                #[cfg(any(unix, test))]
                id,
                lock_path,
                _lock: lock,
            });
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "no free run id under --cwd",
        ))
    }

    /// `<pid>-<8 hex>`.
    #[cfg(any(unix, test))]
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    /// `<cwd>/chrome-profile-<id>`, which Chrome's start makes.
    pub(crate) fn profile(&self) -> &Path {
        &self.profile
    }
}

impl Drop for Run {
    /// Deletes the lock file once the profile is gone. A profile still there (on Windows, a file
    /// Chrome's last helper still held) keeps it, so a later sweep deletes both.
    fn drop(&mut self) {
        if std::fs::symlink_metadata(&self.profile).is_err() {
            drop(write::remove_file(&self.lock_path));
        }
    }
}

/// The longest id: the largest pid and nonce. The socket folder's check (`socket.rs`) measures
/// the socket path with its session name, before the run is claimed.
#[cfg(unix)]
pub(crate) const LONGEST_ID: &str = "4294967295-ffffffff";

/// Run `id`'s agent-browser session name on Unix: `run-<id>`, at most 23 bytes.
#[cfg(any(unix, test))]
pub(crate) fn session_of(id: &str) -> String {
    format!("run-{id}")
}

/// `<cwd>/chrome-profile-<id>`.
pub(crate) fn profile_of(cwd: &Path, id: &str) -> PathBuf {
    cwd.join(format!("{PROFILE_PREFIX}{id}"))
}

/// `<cwd>/chrome-profile-<id>.lock`.
fn lock_of(cwd: &Path, id: &str) -> PathBuf {
    cwd.join(format!("{PROFILE_PREFIX}{id}{LOCK_SUFFIX}"))
}

/// Exactly `<pid>-<8 hex>`: 1 to 10 digits, a dash, 8 of `0-9 a-f`.
pub(crate) fn is_id(text: &str) -> bool {
    let Some((pid, nonce)) = text.split_once('-') else {
        return false;
    };
    (1..=10).contains(&pid.len())
        && pid.bytes().all(|b| b.is_ascii_digit())
        && nonce.len() == 8
        && nonce
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The reaper's share of run `id`'s lock, held until the file drops. `None` when the lock file
/// is gone, or cannot be locked: the reaper still cleans up its own run then.
#[cfg(unix)]
pub(crate) fn hold(cwd: &Path, id: &str) -> Option<File> {
    let file = File::open(lock_of(cwd, id)).ok()?;
    file.lock_shared().ok()?;
    Some(file)
}

/// Deletes run `id`'s lock file if its profile is gone: the reaper's last step.
#[cfg(unix)]
pub(crate) fn forget(cwd: &Path, id: &str) {
    if std::fs::symlink_metadata(profile_of(cwd, id)).is_err() {
        drop(write::remove_file(&lock_of(cwd, id)));
    }
}

/// Deletes every stale run under `cwd`: a lock file nobody holds, with its profile; a profile
/// with no lock file. Called under the sweep lock.
fn sweep(cwd: &Path) {
    let Ok(entries) = std::fs::read_dir(cwd) else {
        return;
    };
    let (mut locks, mut profiles) = (BTreeSet::new(), Vec::new());
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(rest) = name.to_str().and_then(|n| n.strip_prefix(PROFILE_PREFIX)) else {
            continue;
        };
        match rest.strip_suffix(LOCK_SUFFIX) {
            Some(id) if is_id(id) => drop(locks.insert(id.to_owned())),
            None if is_id(rest) => profiles.push(rest.to_owned()),
            _ => {}
        }
    }
    for id in &locks {
        sweep_run(cwd, id);
    }
    for id in profiles.iter().filter(|id| !locks.contains(*id)) {
        if remove_profile(&profile_of(cwd, id)).is_err() {}
    }
}

/// Deletes run `id`'s profile and then its lock file, if its lock is free; else nothing.
fn sweep_run(cwd: &Path, id: &str) {
    let path = lock_of(cwd, id);
    let Ok(file) = File::open(&path) else { return };
    // Held: its guard or its reaper still runs. Any other failure: leave it too.
    if file.try_lock().is_err() {
        return;
    }
    if remove_profile(&profile_of(cwd, id)).is_ok() {
        drop(write::remove_file(&path));
    }
}

/// Deletes `profile`; one already gone counts as deleted.
fn remove_profile(profile: &Path) -> io::Result<()> {
    match write::remove_dir_all(profile) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
