//! `<data_dir>\sandbox\secrets.bin`: each pool user's password, protected with user-scope DPAPI
//! for the set-up's owner (S15 `PR4g`, D-B3). The elevated set-up writes it while acting as the
//! owner (`setup::owner`), so the blobs, the file and its DACL are the owner's; the owner's
//! unelevated app reads it ([`secret_for`], for `PR5`'s runner). Machine-scope DPAPI
//! (`CRYPTPROTECT_LOCAL_MACHINE`) is never used: any local process, pool users included, could
//! undo it.
//!
//! The format, built and tested on every OS, is JSON:
//! `{ "version": 1, "users": [{ "sid": "S-1-5-21-…", "blob_b64": "…" }] }`, one entry per pool
//! user, each blob the standard base64 of what `CryptProtectData` gave. A newer version is
//! refused, never read in part.
//!
//! The file is replaced in one step: a new file beside it, then a rename
//! (`write::write_atomic`), with `write::check_owned_file` on the old file before and on the new
//! one after. A failed write leaves the old file as it was, byte for byte; set-up's rollback
//! relies on that (S15 `PR4d`).
//!
//! # A SID with no blob
//!
//! The record keeps a pool SID whose account is gone while a ledger entry still names it, so
//! removal can still trust those entries (D-B6). Set-up hands [`store`] only the live users, so
//! the file holds no blob for such a SID: it has no account, so it has no password. Asked for
//! it, [`secret_for`] gives [`SecretError::NoSecret`]. That is the whole answer: no other
//! user's blob, no older file and no other scope is tried. The caller (`PR5`'s runner) must not
//! launch as that SID. The same result comes for any SID the file lacks, a missing file
//! included; for a live user that means `secrets.bin` was lost, and running set-up again resets
//! the password and writes the blob (D-B6).
//!
//! No password, blob or token appears in any error or `Debug` text here.

use std::fmt;
#[cfg(any(windows, test))]
use std::fs::File;
use std::io;
#[cfg(any(windows, test))]
use std::io::Write;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
#[cfg(any(windows, test))]
use zeroize::Zeroizing;

use super::matters::SANDBOX_DIR;
use super::sids::Sid;
use super::write;

#[cfg(windows)]
mod dpapi;

/// The file's name, in the sandbox folder.
pub const SECRETS_FILE: &str = "secrets.bin";
/// The format version this build writes and reads.
pub const SECRETS_VERSION: u32 = 1;

/// Why `secrets.bin` couldn't give or take a password. No variant's text holds a password or a
/// blob.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    /// The file holds no blob for this SID: a recorded SID whose account is gone, or a file that
    /// was lost (the module's "A SID with no blob").
    #[error("secrets.bin holds no password for {sid}")]
    NoSecret {
        /// The SID asked for.
        sid: Sid,
    },
    /// Written by a newer Clauseo; this build can't read it.
    #[error("secrets.bin was written by a newer Clauseo (version {found})")]
    Newer {
        /// Its version.
        found: u32,
    },
    /// Not a secrets file this build can read; why, without the file's content.
    #[error("secrets.bin can't be read: {0}")]
    Malformed(String),
    /// Opening, checking or writing the file failed; what, with its path.
    #[error("{0}")]
    Io(String),
    /// A DPAPI call failed with this error code.
    #[error("{call} failed (error {code})")]
    Dpapi {
        /// `CryptProtectData` or `CryptUnprotectData`.
        call: &'static str,
        /// Its error code.
        code: u32,
    },
    /// A password was given for a SID outside the pool being written, so it would be lost.
    #[error("a password was given for {sid}, which is not in the pool being written")]
    NotInPool {
        /// The SID.
        sid: Sid,
    },
}

impl From<SecretError> for crate::SetupError {
    fn from(error: SecretError) -> Self {
        crate::SetupError::Failed(error.to_string())
    }
}

/// What `secrets.bin` holds: one DPAPI blob per pool SID, in the order written. Its `Debug`
/// shows the SIDs and each blob's length, never a blob.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SecretsFile {
    /// Each SID with its blob; no SID twice.
    users: Vec<(Sid, Vec<u8>)>,
}

/// The file on disk.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    version: u32,
    users: Vec<WireUser>,
}

/// One user on disk.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireUser {
    sid: Sid,
    blob_b64: String,
}

/// The field every version has, read before the version decides the rest.
#[derive(Deserialize)]
struct Head {
    version: u32,
}

impl SecretsFile {
    /// A file with no users.
    pub fn new() -> SecretsFile {
        SecretsFile::default()
    }

    /// The blob for `sid`; `None` when the file has none.
    pub fn blob_for(&self, sid: &Sid) -> Option<&[u8]> {
        self.users
            .iter()
            .find(|(user, _)| user == sid)
            .map(|(_, blob)| blob.as_slice())
    }

    /// Sets `sid`'s blob, replacing one there.
    pub fn insert(&mut self, sid: Sid, blob: Vec<u8>) {
        match self.users.iter_mut().find(|(user, _)| *user == sid) {
            Some(entry) => entry.1 = blob,
            None => self.users.push((sid, blob)),
        }
    }

    /// The SIDs with a blob, in file order.
    pub fn sids(&self) -> impl Iterator<Item = &Sid> {
        self.users.iter().map(|(sid, _)| sid)
    }

    /// Parses the file's bytes. The version is read first: a higher one is
    /// [`SecretError::Newer`]; this one must parse whole, with every SID once and every blob
    /// valid base64 and not empty.
    pub fn from_bytes(bytes: &[u8]) -> Result<SecretsFile, SecretError> {
        let head: Head = serde_json::from_slice(bytes).map_err(|e| malformed(&e))?;
        match head.version {
            SECRETS_VERSION => {}
            found if found > SECRETS_VERSION => return Err(SecretError::Newer { found }),
            found => {
                return Err(SecretError::Malformed(format!("unknown version {found}")));
            }
        }
        let wire: Wire = serde_json::from_slice(bytes).map_err(|e| malformed(&e))?;
        let mut file = SecretsFile::new();
        for user in wire.users {
            if file.blob_for(&user.sid).is_some() {
                return Err(SecretError::Malformed(format!(
                    "{} is listed twice",
                    user.sid
                )));
            }
            let blob = STANDARD.decode(user.blob_b64.as_bytes()).map_err(|error| {
                SecretError::Malformed(format!(
                    "the blob for {} is not base64 ({})",
                    user.sid,
                    not_base64(&error)
                ))
            })?;
            if blob.is_empty() {
                return Err(SecretError::Malformed(format!(
                    "the blob for {} is empty",
                    user.sid
                )));
            }
            file.users.push((user.sid, blob));
        }
        Ok(file)
    }

    /// The file's bytes at [`SECRETS_VERSION`]: pretty JSON and a final newline.
    pub fn to_bytes(&self) -> Result<Vec<u8>, SecretError> {
        let wire = Wire {
            version: SECRETS_VERSION,
            users: self
                .users
                .iter()
                .map(|(sid, blob)| WireUser {
                    sid: sid.clone(),
                    blob_b64: STANDARD.encode(blob),
                })
                .collect(),
        };
        let mut bytes = serde_json::to_vec_pretty(&wire).map_err(|e| malformed(&e))?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

impl fmt::Debug for SecretsFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut list = f.debug_list();
        for (sid, blob) in &self.users {
            list.entry(&format_args!("{sid}: {} bytes", blob.len()));
        }
        list.finish()
    }
}

/// A parse error as its kind and place, never the text it stopped at (which could be a blob).
fn malformed(error: &serde_json::Error) -> SecretError {
    SecretError::Malformed(format!(
        "not a secrets file ({:?} error at line {}, column {})",
        error.classify(),
        error.line(),
        error.column()
    ))
}

/// What is wrong with a blob that isn't base64, without the byte that stopped the decode.
fn not_base64(error: &base64::DecodeError) -> &'static str {
    match error {
        base64::DecodeError::InvalidByte(..) => "a character outside base64",
        base64::DecodeError::InvalidLength(_) => "a length base64 can't have",
        base64::DecodeError::InvalidLastSymbol(..) => "a last character base64 can't end with",
        base64::DecodeError::InvalidPadding => "wrong padding",
    }
}

/// The file's path under `data_dir`.
pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(SANDBOX_DIR).join(SECRETS_FILE)
}

/// Reads `<data_dir>\sandbox\secrets.bin`, after `write::check_owned_file` (a link, a second
/// hard link or an owner that can't be taken back is refused). A missing file reads as one with
/// no users, so every SID is then [`SecretError::NoSecret`].
pub fn read(data_dir: &Path) -> Result<SecretsFile, SecretError> {
    let file = path(data_dir);
    write::check_owned_file(&file).map_err(|error| io_error(&error))?;
    match std::fs::read(&file) {
        Ok(bytes) => SecretsFile::from_bytes(&bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(SecretsFile::new()),
        Err(error) => Err(SecretError::Io(format!("{}: {error}", file.display()))),
    }
}

/// An `io::Error` from the write module, whose text names the path where it matters.
fn io_error(error: &io::Error) -> SecretError {
    SecretError::Io(format!("secrets.bin: {error}"))
}

/// Writes `secrets.bin` for `pool`, as `Machine::protect_for_owner` asks: each SID of `pool` in
/// order gets the blob `protect` makes of its password in `secrets`, or else the blob the file
/// holds now; a SID with neither is [`SecretError::NoSecret`] and nothing is written. A
/// password for a SID outside `pool` is [`SecretError::NotInPool`]. The old file is read only
/// when some SID keeps its blob. Then the file is replaced as the module says; on any failure
/// the old file stays as it was. The caller is acting as the owner (Windows), so the folder,
/// the file and their DACLs are the owner's.
#[cfg(any(windows, test))]
pub(crate) fn store(
    data_dir: &Path,
    secrets: &[(Sid, Zeroizing<String>)],
    pool: &[Sid],
    protect: impl FnMut(&[u8]) -> Result<Vec<u8>, SecretError>,
) -> Result<(), SecretError> {
    store_with(data_dir, secrets, pool, protect, Write::write_all)
}

/// [`store`] with the file write as a parameter, so a test can fail it partway.
#[cfg(any(windows, test))]
fn store_with(
    data_dir: &Path,
    secrets: &[(Sid, Zeroizing<String>)],
    pool: &[Sid],
    protect: impl FnMut(&[u8]) -> Result<Vec<u8>, SecretError>,
    write_bytes: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
) -> Result<(), SecretError> {
    let file = path(data_dir);
    // The folder first: a link or junction there is refused before anything lists it.
    if let Some(folder) = file.parent() {
        write::make_sandbox_dir(folder).map_err(|error| io_error(&error))?;
    }
    remove_stale_temps(&file)?;
    let new = merge(|| read(data_dir), secrets, pool, protect)?;
    write::check_owned_file(&file).map_err(|error| io_error(&error))?;
    write::write_atomic_with(&file, &new.to_bytes()?, write_bytes)
        .map_err(|error| SecretError::Io(format!("{}: {error}", file.display())))?;
    // The new file in place gets the same explicit DACL.
    write::check_owned_file(&file).map_err(|error| io_error(&error))
}

/// Removes the new files an earlier [`store`] left beside `file` when it stopped between
/// writing and renaming one (a crash): each entry of the folder whose name is exactly
/// `write::write_atomic`'s temp name for `file` (`write::is_atomic_temp`). Each goes through
/// `write::remove_owned_file`, so a link or a file with a second hard link is never followed
/// or removed: it stays, and does no harm, since every write makes a new name. Nothing else in
/// the folder is touched. A missing folder has nothing to remove. Called after
/// `write::make_sandbox_dir`, which refuses a folder that is a link or a junction, so the
/// listing never reads through one.
#[cfg(any(windows, test))]
fn remove_stale_temps(file: &Path) -> Result<(), SecretError> {
    let (Some(folder), Some(name)) = (file.parent(), file.file_name()) else {
        return Ok(());
    };
    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(SecretError::Io(format!("{}: {error}", folder.display()))),
    };
    for entry in entries {
        let entry = entry.map_err(|error| io_error(&error))?;
        if !write::is_atomic_temp(name, &entry.file_name()) {
            continue;
        }
        // A link or a hard-linked file is refused by the check and left where it is.
        drop(write::remove_owned_file(&entry.path()));
    }
    Ok(())
}

/// The file [`store`] writes, from the old file (read through `old` only when needed).
#[cfg(any(windows, test))]
fn merge(
    old: impl FnOnce() -> Result<SecretsFile, SecretError>,
    secrets: &[(Sid, Zeroizing<String>)],
    pool: &[Sid],
    mut protect: impl FnMut(&[u8]) -> Result<Vec<u8>, SecretError>,
) -> Result<SecretsFile, SecretError> {
    if let Some((sid, _)) = secrets.iter().find(|(sid, _)| !pool.contains(sid)) {
        return Err(SecretError::NotInPool { sid: sid.clone() });
    }
    let password_of = |sid: &Sid| secrets.iter().find(|(s, _)| s == sid).map(|(_, p)| p);
    let old = if pool.iter().all(|sid| password_of(sid).is_some()) {
        SecretsFile::new()
    } else {
        old()?
    };
    let mut new = SecretsFile::new();
    for sid in pool {
        if new.blob_for(sid).is_some() {
            continue;
        }
        let blob = match password_of(sid) {
            Some(password) => protect(password.as_bytes())?,
            None => old
                .blob_for(sid)
                .ok_or_else(|| SecretError::NoSecret { sid: sid.clone() })?
                .to_vec(),
        };
        new.users.push((sid.clone(), blob));
    }
    Ok(new)
}

/// `sid`'s password from the file under `data_dir`, through `unprotect`: the file read as
/// [`read`] reads it, the SID's blob ([`SecretError::NoSecret`] when there is none), then
/// `unprotect`. A password that isn't UTF-8 is [`SecretError::Malformed`], its bytes wiped.
#[cfg(any(windows, test))]
fn secret_with(
    data_dir: &Path,
    sid: &Sid,
    unprotect: impl FnOnce(&[u8]) -> Result<Zeroizing<Vec<u8>>, SecretError>,
) -> Result<Zeroizing<String>, SecretError> {
    let file = read(data_dir)?;
    let blob = file
        .blob_for(sid)
        .ok_or_else(|| SecretError::NoSecret { sid: sid.clone() })?;
    let mut plain = unprotect(blob)?;
    // Moves the buffer out without a copy; `plain` keeps an empty one.
    let bytes = std::mem::take(&mut *plain);
    match String::from_utf8(bytes) {
        Ok(text) => Ok(Zeroizing::new(text)),
        Err(error) => {
            drop(Zeroizing::new(error.into_bytes()));
            Err(SecretError::Malformed(format!(
                "the password for {sid} is not text"
            )))
        }
    }
}

/// `sid`'s password, unprotected with user-scope DPAPI as this thread's user: for the owner's
/// unelevated app (`PR5`'s runner). A SID the file lacks is [`SecretError::NoSecret`] (the
/// module's "A SID with no blob"); a blob the user can't unprotect (another user's, or another
/// entropy) is [`SecretError::Dpapi`].
#[cfg(windows)]
pub fn secret_for(data_dir: &Path, sid: &Sid) -> Result<Zeroizing<String>, SecretError> {
    secret_with(data_dir, sid, dpapi::unprotect)
}

/// `Machine::protect_for_owner` for `WindowsMachine`: [`store`] under `data_dir` while this
/// thread acts as `owner` (`Owner::run_as`), so the DPAPI blobs are the owner's (user scope,
/// D-B3) and the file and its DACL are too. The thread stops acting as the owner on every
/// return.
#[cfg(windows)]
pub fn protect_for_owner(
    owner: &super::setup::owner::Owner,
    data_dir: &Path,
    secrets: &[(Sid, super::setup::Password)],
    pool: &[Sid],
) -> Result<(), super::setup::MachineError> {
    use super::setup::MachineError;

    let stored = owner.run_as(|| store(data_dir, secrets, pool, dpapi::protect))?;
    stored.map_err(|error| {
        if let SecretError::Dpapi { call, code } = error {
            MachineError::Os { call, code }
        } else {
            MachineError::Other(error.to_string())
        }
    })
}

#[cfg(test)]
mod tests;
