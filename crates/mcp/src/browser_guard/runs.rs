//! Each guard run's own names under `--cwd`, so one run never deletes or kills another's. An MCP
//! reconnect starts a new guard under the same `--cwd` while the killed guard's reaper may still
//! be cleaning up; with names shared by the session's runs, that reaper deleted the new run's
//! profile and could end its daemon (decision record 0029, "Each run's own names").
//!
//! - The run's id, `<guard pid>-<8 hex>`: the hex is random, and the lock file below, made with
//!   `create_new`, keeps two runs from ever sharing an id.
//! - Its profile `<cwd>/chrome-profile-<id>`, with Chrome's downloads staging folder inside
//!   (`chrome.rs`), and on Unix its agent-browser session name `run-<id>`, which names the
//!   daemon's socket `<socket dir>/run-<id>.sock` (`reaper::kill_daemon`). On Windows the name
//!   is `run-<id>-<n>`, with the `n` whose derived daemon port is the one the host claimed
//!   ([`windows_session`]), so the daemon's port files are the run's own too. No reaper runs
//!   there; the guard's Job Object ends the daemon.
//! - Its Chrome record `<cwd>/chrome-profile-<id>.chrome` (`record.rs`), which the host reads.
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
//!
//! [`settled`] tells the host when every run under `--cwd` has finished its cleanup.

use std::collections::BTreeSet;
use std::fs::File;
use std::hash::{BuildHasher, RandomState};
use std::io;
use std::path::{Path, PathBuf};

use crate::write;

/// The start of every run's profile and lock file names under `--cwd`.
pub(crate) const PROFILE_PREFIX: &str = "chrome-profile-";
/// The end of a run's lock file name.
pub(crate) const LOCK_SUFFIX: &str = ".lock";
/// The end of a run's Chrome record's name (`record.rs`).
pub(crate) const RECORD_SUFFIX: &str = ".chrome";
/// The lock file every run under one `--cwd` shares, for the sweep.
pub(crate) const SWEEP_LOCK: &str = "chrome-profiles.lock";
/// How many ids a run tries before it gives up.
const ID_TRIES: u64 = 16;

/// The guard's run: its id, its profile, and its share of its lock, held until it drops.
pub(crate) struct Run {
    /// The reaper's `--run` (Unix) and the session name's.
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

/// How many suffixes [`windows_session`] tries: about 16 400 are needed on average.
#[cfg(any(windows, test))]
const SESSION_TRIES: u32 = 1_000_000;

/// Run `id`'s agent-browser session name on Windows: `run-<id>-<n>`, with the smallest `n` whose
/// daemon port ([`daemon_port_of`]) is `port`, the one the host claimed for the host's own
/// name. So the daemon's port, and its `.port` and `.stream` files in `<cwd>\run`, are the
/// run's own too: a new run's CLI never reaches a dying run's daemon. `None` if no suffix up to
/// [`SESSION_TRIES`] fits, which a port from 49152 to 65534 never meets in practice.
#[cfg(any(windows, test))]
pub(crate) fn windows_session(id: &str, port: u16) -> Option<String> {
    (0..SESSION_TRIES)
        .map(|n| format!("run-{id}-{n}"))
        .find(|name| daemon_port_of(name) == port)
}

/// The daemon port agent-browser derives from a session name on Windows (v0.41.1
/// `connection.rs:423-431`, no namespace set): a Java string hash, folded into 49152 to 65534.
#[cfg(any(windows, test))]
pub(crate) fn daemon_port_of(session: &str) -> u16 {
    let mut hash: i32 = 0;
    for c in session.chars() {
        let c = i32::try_from(u32::from(c)).unwrap_or(i32::MAX);
        hash = (hash << 5).wrapping_sub(hash).wrapping_add(c);
    }
    let folded = u16::try_from(hash.unsigned_abs() % 16_383).unwrap_or(0);
    49_152 + folded
}

/// `<cwd>/chrome-profile-<id>`.
pub(crate) fn profile_of(cwd: &Path, id: &str) -> PathBuf {
    cwd.join(format!("{PROFILE_PREFIX}{id}"))
}

/// `<cwd>/chrome-profile-<id>.chrome`, the run's Chrome record (`record.rs`).
pub(crate) fn record_of(cwd: &Path, id: &str) -> PathBuf {
    cwd.join(format!("{PROFILE_PREFIX}{id}{RECORD_SUFFIX}"))
}

/// `<cwd>/chrome-profile-<id>.lock`.
pub(crate) fn lock_of(cwd: &Path, id: &str) -> PathBuf {
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

/// Deletes every stale run under `cwd`: a lock file nobody holds, with its profile and its Chrome
/// record; a profile with no lock file; a record with no lock file. A record that may name a
/// live Chrome stays (`record.rs`). Called under the sweep lock.
fn sweep(cwd: &Path) {
    let Ok(entries) = std::fs::read_dir(cwd) else {
        return;
    };
    let (mut locks, mut profiles, mut records) = (BTreeSet::new(), Vec::new(), Vec::new());
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(rest) = name.to_str().and_then(|n| n.strip_prefix(PROFILE_PREFIX)) else {
            continue;
        };
        match (
            rest.strip_suffix(LOCK_SUFFIX),
            rest.strip_suffix(RECORD_SUFFIX),
        ) {
            (Some(id), _) if is_id(id) => drop(locks.insert(id.to_owned())),
            (_, Some(id)) if is_id(id) => records.push(id.to_owned()),
            (None, None) if is_id(rest) => profiles.push(rest.to_owned()),
            _ => {}
        }
    }
    for id in &locks {
        sweep_run(cwd, id);
    }
    for id in profiles.iter().filter(|id| !locks.contains(*id)) {
        if remove_profile(&profile_of(cwd, id)).is_err() {}
    }
    // A record whose run is gone, unless it may name a live Chrome (`record.rs`).
    for id in records.iter().filter(|id| !lock_of(cwd, id).exists()) {
        super::record::forget(cwd, id);
    }
}

/// If run `id`'s lock is free: its Chrome record (`record.rs`; one left saying `starting` ends
/// what runs with the profile first), then its profile and then its lock file; else nothing.
fn sweep_run(cwd: &Path, id: &str) {
    let path = lock_of(cwd, id);
    let Ok(file) = File::open(&path) else { return };
    // Held: its guard or its reaper still runs. Any other failure: leave it too.
    if file.try_lock().is_err() {
        return;
    }
    super::record::forget(cwd, id);
    if remove_profile(&profile_of(cwd, id)).is_ok() {
        drop(write::remove_file(&path));
    }
}

/// Whether every run under `cwd` has finished its cleanup: no run's profile folder is left, and
/// no run's lock file is held (by its guard or its reaper). A free lock file with no profile
/// (a sweep's leftover) counts as settled; the sweep lock and the Chrome records are not looked
/// at. Each lock file is probed with [`held`]: a live run holds its share, so the probe fails and
/// takes nothing from it; a guard or reaper taking its share at that instant waits the probe out.
/// A run between making its lock file and taking its share has started nothing yet and reads as
/// settled. "Cleanup finished", not "Chrome dead": decision record 0029, "Settled runs".
pub(crate) fn settled(cwd: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(cwd) else {
        return true;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(rest) = name.to_str().and_then(|n| n.strip_prefix(PROFILE_PREFIX)) else {
            continue;
        };
        let live = match rest.strip_suffix(LOCK_SUFFIX) {
            Some(id) if is_id(id) => {
                File::open(entry.path()).map_or(true, |file| file.try_lock().is_err())
            }
            // A profile, or anything else at a profile's name: its cleanup has not finished.
            None if is_id(rest) => true,
            _ => false,
        };
        if live {
            return false;
        }
    }
    true
}

/// Whether the lock file at `path` is held: an exclusive `try_lock`, released at once, fails. A
/// lock file that cannot be opened or probed counts as held; one that is gone, as free.
pub(crate) fn held(path: &Path) -> bool {
    match File::open(path) {
        Ok(file) => file.try_lock().is_err(),
        Err(error) => error.kind() != io::ErrorKind::NotFound,
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
