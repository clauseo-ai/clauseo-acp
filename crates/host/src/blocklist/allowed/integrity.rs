//! `allowed.json`'s integrity check (decision record 0053, PR 2): the lowercase hex sha256 of
//! the file's bytes as last written, kept in the OS secret store (`clauseo_connectors`'
//! [`SecretStore`]: the macOS Keychain, Windows Credential Manager) under the account
//! [`ACCOUNT`]. A file whose bytes do not hash to it counts as no allows, so editing the file,
//! or putting back an older copy, allows nothing.
//!
//! Everything here runs on one thread of its own ([`run`]), never the actor's, so a Keychain
//! prompt holds up only the allowed sites: the start-up read first, then each save, in order.
//!
//! - The store is asked once, at the start. No store (no seam, Linux's `Unsupported`), a
//!   refusal (a Keychain "Deny") or any other error means no allows for the run, logged once,
//!   and the run writes nothing. No item while the file exists, or a mismatch, means no allows.
//!   No file is no allows, whatever the item; a rejected file is left on disk.
//! - Each save sets the new hash first, then writes the file. A hash that is not set writes
//!   nothing, and the run writes nothing more: the old pair stays as it was. A file that is not
//!   written after its hash was leaves a mismatch, as does a crash between the two: the next
//!   start has no allows, which fails closed.
//! - A revoke that is not saved while the old pair may still match deletes the stored hash, so
//!   the next start honours no allows rather than the revoked one ([`Writer::void`]); in a run
//!   that writes nothing it is tried once.
//! - A file the start rejected (unreadable, over the cap, unparsable, no item, a mismatch) is
//!   set aside at the run's first write, never at the start: moved to
//!   `allowed.json.rejected-<UTC time>` beside it (a counter after a taken name), without ever
//!   replacing a file, and logged with the reason and the new name; only the latest
//!   [`KEEP_REJECTED`] are kept. The temp-file sweep (`.clauseo-*.new`) never takes them, and
//!   the agent's fence denies the whole folder. A move that fails writes nothing, as above.

use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

use clauseo_connectors::SecretStore;
use clauseo_protocol::Timestamp;
use sha2::{Digest as _, Sha256};

use super::file::{self, Body, Stored};
use crate::log_file::Cause;

/// The secret store's account holding the hash.
pub(crate) const ACCOUNT: &str = "blocklist-allowed-sha256";
/// How many set-aside files are kept; older ones are removed at each new one.
pub(crate) const KEEP_REJECTED: usize = 3;
/// The set-aside files' name before their time.
const REJECTED: &str = "allowed.json.rejected-";

/// The lowercase hex sha256 of `body`, the bytes the item holds.
pub(crate) fn digest(body: &[u8]) -> String {
    Sha256::digest(body)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _infallible = write!(hex, "{byte:02x}");
            hex
        })
}

/// What the thread tells the actor.
#[derive(Debug)]
pub(crate) enum Report {
    /// The start-up read.
    Opened(Opened),
    /// One save's outcome, in the order they were asked.
    Wrote(Wrote),
}

/// The allows a run starts with.
#[derive(Debug)]
pub(crate) struct Opened {
    pub(crate) stored: Stored,
    /// What the next start would honour now, when known: `stored` when the file was trusted,
    /// none when there is no file or it was rejected; unknown (`None`) when the store failed.
    pub(crate) persisted: Option<Stored>,
}

/// One save to make: the whole state after an allow or a revoke.
#[derive(Debug)]
pub(crate) struct Job {
    pub(crate) stored: Stored,
    pub(crate) revoke: bool,
    /// When it was asked, for a set-aside file's name.
    pub(crate) now: Timestamp,
}

/// A save's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wrote {
    /// The hash and the file hold the job's state: the next start honours it.
    Saved,
    /// Not saved. `voided`: the stored pair no longer matches (a hash set but not its file, a
    /// deleted hash, a rejected file), so the next start honours no allows.
    NotSaved { voided: bool },
}

/// The thread's body: the start-up read, posted, then each job's save, posted, until `jobs`
/// closes (the host stopped, or gave up waiting for the read). Once `abandoned` (the host's
/// stop gave up on the queue) it writes nothing more, hash or file.
pub(crate) fn run(
    path: &Path,
    secrets: Option<Arc<dyn SecretStore>>,
    abandoned: Arc<AtomicBool>,
    jobs: &mpsc::Receiver<Job>,
    post: impl Fn(Report),
) {
    let (opened, mut writer) = open(path, secrets, abandoned);
    post(Report::Opened(opened));
    for job in jobs {
        post(Report::Wrote(writer.save(&job)));
    }
}

/// The thread's state for saving.
struct Writer {
    path: PathBuf,
    store: Option<Arc<dyn SecretStore>>,
    /// False once the store failed (or there is none): nothing more is written this run.
    writing: bool,
    /// Why the start rejected the file there, which the first write sets aside first.
    set_aside: Option<&'static str>,
    /// Whether a delete of the hash was tried while not writing (once per run).
    void_tried: bool,
    /// Set by the host's stop once it gives up on the queue.
    abandoned: Arc<AtomicBool>,
}

/// Asks `secrets` for the hash once, reads the file at `path`, and keeps its allows only when
/// its bytes hash to the stored value. Every other case is no allows, logged.
fn open(
    path: &Path,
    secrets: Option<Arc<dyn SecretStore>>,
    abandoned: Arc<AtomicBool>,
) -> (Opened, Writer) {
    let mut writer = Writer {
        path: path.to_path_buf(),
        store: secrets,
        writing: false,
        set_aside: None,
        void_tried: false,
        abandoned,
    };
    let unknown = Opened {
        stored: Stored::default(),
        persisted: None,
    };
    let in_memory = |why: &str| {
        tracing::warn!(
            why,
            "blocklist: no secret store for allowed.json's hash; no allows, and allows made \
             now apply until Clauseo stops"
        );
    };
    let Some(store) = &writer.store else {
        in_memory("none");
        // Without a store no start honours an allow, so nothing on disk can come back.
        let nothing = Opened {
            stored: Stored::default(),
            persisted: Some(Stored::default()),
        };
        return (nothing, writer);
    };
    let expected = match store.get(ACCOUNT) {
        Ok(expected) => expected,
        Err(error) => {
            in_memory(&error.to_string());
            return (unknown, writer);
        }
    };
    let (stored, set_aside) = match (file::body(path), expected) {
        (Body::Missing, _) => (Stored::default(), None),
        (Body::Unusable(reason), _) => (Stored::default(), Some(reason)),
        (Body::Read(_), None) => {
            tracing::warn!("blocklist: allowed.json has no stored hash; no allows");
            (Stored::default(), Some("no stored hash"))
        }
        (Body::Read(body), Some(expected)) if expected == digest(&body).as_bytes() => {
            match file::parse(&body) {
                Some(stored) => (stored, None),
                None => (Stored::default(), Some("unparsable")),
            }
        }
        (Body::Read(_), Some(_)) => {
            tracing::warn!("blocklist: allowed.json does not match its stored hash; no allows");
            (Stored::default(), Some("hash mismatch"))
        }
    };
    writer.writing = true;
    writer.set_aside = set_aside;
    let opened = Opened {
        persisted: Some(stored.clone()),
        stored,
    };
    (opened, writer)
}

impl Writer {
    /// Saves `job`, or says why not; a revoke not saved voids the stored pair if it can.
    fn save(&mut self, job: &Job) -> Wrote {
        let voided = match self.try_save(job) {
            Ok(()) => return Wrote::Saved,
            Err(voided) => voided,
        };
        Wrote::NotSaved {
            voided: voided || (job.revoke && self.void()),
        }
    }

    /// The set-aside, the hash, then the file. `Err(voided)` when not saved.
    fn try_save(&mut self, job: &Job) -> Result<(), bool> {
        let Some(store) = self.store.clone().filter(|_| self.writing) else {
            return Err(false);
        };
        let flag = Arc::clone(&self.abandoned);
        let abandoned = || flag.load(Ordering::SeqCst);
        if abandoned() {
            return Err(false);
        }
        if let Some(reason) = self.set_aside {
            match set_aside_file(&self.path, job.now) {
                Ok(name) => {
                    tracing::warn!(reason, name, "blocklist: allowed.json set aside, rejected");
                    self.set_aside = None;
                    prune_rejected(&self.path);
                }
                Err(error) => {
                    tracing::warn!(
                        reason,
                        error = %Cause(&error),
                        "blocklist: rejected allowed.json not set aside, so not overwritten; \
                         allows change in memory only until Clauseo restarts"
                    );
                    self.writing = false;
                    // The file there was rejected already: nothing on disk is honoured.
                    return Err(true);
                }
            }
        }
        let body = file::encode(&job.stored).map_err(|_unsaid| false)?;
        if let Err(error) = store.set(ACCOUNT, digest(&body).as_bytes()) {
            tracing::warn!(
                %error,
                "blocklist: allowed.json's hash not stored, so nothing is written; allows \
                 change in memory only until Clauseo restarts"
            );
            self.writing = false;
            return Err(false);
        }
        // The stop gave up while the hash was being set, and may have deleted it meanwhile. The
        // file stays old, but this state may equal the file's (a repeat allow, a revoke that
        // removed nothing), so the hash just set could match it: delete it again.
        if abandoned() {
            if let Err(error) = store.delete(ACCOUNT) {
                tracing::warn!(
                    %error,
                    "blocklist: allowed.json's hash set after the stop not deleted; a revoked \
                     site may be allowed again at the next start"
                );
            }
            return Err(true);
        }
        if let Err(error) = file::write_bytes(&self.path, &body) {
            tracing::warn!(
                error = %Cause(&error),
                "blocklist: allowed.json not written after its hash; no allows at the next start"
            );
            return Err(true);
        }
        Ok(())
    }

    /// Deletes the stored hash so the next start honours no allows; true when it is gone.
    /// While not writing, tried once per run (a denied store would prompt again).
    fn void(&mut self) -> bool {
        let Some(store) = &self.store else {
            return false;
        };
        if !self.writing {
            if self.void_tried {
                return false;
            }
            self.void_tried = true;
        }
        match store.delete(ACCOUNT) {
            Ok(()) => {
                tracing::warn!(
                    "blocklist: allowed.json's hash deleted; no allows at the next start"
                );
                true
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    "blocklist: allowed.json's hash not deleted; a revoked site may be allowed \
                     again at the next start"
                );
                self.writing = false;
                self.void_tried = true;
                false
            }
        }
    }
}

/// Moves the file at `path` to `<REJECTED><UTC now, YYYYMMDDTHHMMSSZ>` beside it, with `-<n>`
/// after a name already taken, never replacing a file. Returns the new file name.
fn set_aside_file(path: &Path, now: Timestamp) -> io::Result<String> {
    let folder = path.parent().unwrap_or(path);
    let stem = format!("{REJECTED}{}", utc_stamp(now));
    for n in 0..100_u32 {
        let name = if n == 0 {
            stem.clone()
        } else {
            format!("{stem}-{n}")
        };
        match crate::write::move_file_no_replace(path, &folder.join(&name)) {
            Ok(()) => return Ok(name),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "every set-aside name is taken",
    ))
}

/// Removes all but the latest [`KEEP_REJECTED`] set-aside files beside `path` (by name, which
/// starts with the time). One that does not go is logged and left.
fn prune_rejected(path: &Path) {
    let folder = path.parent().unwrap_or(path);
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(REJECTED))
        .collect();
    names.sort_by_key(|name| sort_key(name));
    let old = names.len().saturating_sub(KEEP_REJECTED);
    for name in names.iter().take(old) {
        if let Err(error) = crate::write::remove_old_file(&folder.join(name)) {
            tracing::warn!(name, error = %Cause(&error), "blocklist: old set-aside file kept");
        }
    }
}

/// A set-aside name's order: its time, then its counter as a number.
fn sort_key(name: &str) -> (String, u32) {
    let rest = name.trim_start_matches(REJECTED);
    match rest.split_once('-') {
        Some((time, n)) => (time.to_owned(), n.parse().unwrap_or(0)),
        None => (rest.to_owned(), 0),
    }
}

/// `now` as `YYYYMMDDTHHMMSSZ`, in UTC.
fn utc_stamp(now: Timestamp) -> String {
    let secs = now.as_millis().div_euclid(1000);
    let (year, month, day) = crate::connectors::civil_from_days(secs.div_euclid(86_400));
    let time = secs.rem_euclid(86_400);
    let (hour, minute, second) = (time / 3600, time % 3600 / 60, time % 60);
    format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z")
}
