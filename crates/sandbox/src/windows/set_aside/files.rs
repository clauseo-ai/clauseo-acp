//! The set-aside's files in `<data_dir>\sandbox\set-aside\<matter>\`: the record of the
//! protected names at launch (`record.json`), the manifest of what was set aside
//! (`manifest.json`), and the lock every read and write of either holds (`set-aside.lock`,
//! taken as the ledger takes `acl-ledger.lock`: the file checked, then `write::lock_exclusive`
//! with its bounded wait). Each folder on the way
//! is made, or checked when there, as the sandbox folder is (`write::make_sandbox_dir`: refused
//! if a link, the protected DACL set again); each file is checked before it is read and after
//! it is written (`write::check_owned_file`), and replaced in one step (`write::write_atomic`).
//!
//! `record.json`: `{"version": 1, "matter": "<id>", "sessions": ["<session>", ...], "names":
//! [{"path": [".git"], "id": {"volume": 1234, "id": "<32 hex digits>"}}, {"path":
//! ["CLAUDE.md"], "id": null}, {"path": ["sub", ".git"], "id": {...}}, ...], "links": [{"path":
//! ["Docs", "Precedents"], "id": {...}}, ...]}`: each name's path below the matter's root, one
//! name per part, and `null` for a name at the root that was absent; and each link (junction
//! or symbolic link) in the matter tree by its own path and file ID (R2). Each part, like each
//! name and path in the manifest, is kept as the file system gave it (`names.rs`): a string
//! when it is text, `{"utf16": [...]}` when it isn't.
//! `manifest.json`: `{"version": 1, "next": 3, "entries": [<Entry>, ...], "intents": [<Entry>,
//! ...]}`: `intents` holds the moves begun and not yet finished (N10), and is left out when
//! empty.

use std::collections::BTreeSet;
#[cfg(any(windows, test))]
use std::ffi::{OsStr, OsString};
#[cfg(any(windows, test))]
use std::fs::File;
#[cfg(any(windows, test))]
use std::io;
#[cfg(any(windows, test))]
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::Entry;
use super::SET_ASIDE_BOOKKEEPING;
use super::names::{Links, Names, keyed};
#[cfg(any(windows, test))]
use super::{SET_ASIDE_DIR, SetAsideFailure};
#[cfg(any(windows, test))]
use crate::windows::matters::SANDBOX_DIR;
#[cfg(any(windows, test))]
use crate::windows::write;

#[cfg(any(windows, test))]
const RECORD_FILE: &str = SET_ASIDE_BOOKKEEPING[0];
#[cfg(any(windows, test))]
const MANIFEST_FILE: &str = SET_ASIDE_BOOKKEEPING[2];
/// The matter's lock file: every read and write of the record or the manifest holds it.
pub(super) const LOCK_FILE: &str = SET_ASIDE_BOOKKEEPING[1];
/// The key, in [`Record::names`], that says the record holds the `HEAD` files
/// ([`Kept::Head`](super::scan::Kept::Head)): a record an older version wrote lacks it, and
/// its `HEAD`s are then recorded, never set aside, so none is taken for new. No file can have
/// the name: `:` separates a stream on NTFS. Older versions ignore the key.
#[cfg(any(windows, test))]
pub(super) const HEADS_RECORDED: &str = ":heads";
/// The version both files are written in and the only one read.
const VERSION: u32 = 1;

/// A file's ID on its volume, read from a handle: the volume's serial and the 128-bit id
/// (`FILE_ID_INFO`). The same ID means the same file or folder, whatever its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileId {
    /// The volume's serial number.
    pub(super) volume: u64,
    /// The id on that volume, as 32 hex digits in the record.
    #[serde(serialize_with = "to_hex", deserialize_with = "from_hex")]
    pub(super) id: [u8; 16],
}

/// What the last pass recorded: each protected name in the matter and its file ID, and the
/// sessions launched whose after-session pass hasn't run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    version: u32,
    matter: MatterId,
    /// The sessions launched with no after-session pass yet: running, or ended without one.
    /// The record is clean only when this is empty.
    pub(super) sessions: BTreeSet<String>,
    /// Each protected name by its path below the matter's root, one name per part, and its
    /// file ID, or `None` for a name at the root that was absent.
    #[serde(with = "keyed")]
    pub(super) names: Names,
    /// Each link (a junction or symbolic link, whatever its name) in the matter tree by its
    /// path below the root, and the link's own file ID (R2).
    #[serde(default, with = "keyed")]
    pub(super) links: Links,
}

/// Whether `names` holds [`HEADS_RECORDED`].
#[cfg(any(windows, test))]
pub(super) fn heads_recorded(names: &Names) -> bool {
    names
        .keys()
        .any(|key| key.len() == 1 && key.first().is_some_and(|k| k == HEADS_RECORDED))
}

/// What a pass records of the matter: [`Record::names`] and [`Record::links`].
#[cfg(any(windows, test))]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub(super) names: Names,
    pub(super) links: Links,
}

impl Record {
    /// Whether the record holds the `HEAD` files ([`HEADS_RECORDED`]).
    #[cfg(any(windows, test))]
    pub(super) fn holds_heads(&self) -> bool {
        heads_recorded(&self.names)
    }

    /// Whether `bytes` read as a record of [`VERSION`] with no open session; anything the typed
    /// read refuses is `false`.
    pub(super) fn closed(bytes: &[u8]) -> bool {
        serde_json::from_slice::<Record>(bytes)
            .is_ok_and(|record| record.version == VERSION && record.sessions.is_empty())
    }

    #[cfg(any(windows, test))]
    pub(super) fn new(matter: MatterId, sessions: BTreeSet<String>, seen: Snapshot) -> Record {
        Record {
            version: VERSION,
            matter,
            sessions,
            names: seen.names,
            links: seen.links,
        }
    }
}

/// Every item set aside from a matter and not restored.
#[cfg(any(windows, test, feature = "test-support"))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Manifest {
    version: u32,
    /// The next entry's number.
    pub(super) next: u64,
    /// The items set aside, each move finished.
    pub(super) entries: Vec<Entry>,
    /// The moves begun: each written before its move and dropped after it, so a crash in
    /// between leaves it here for the next load to settle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) intents: Vec<Entry>,
}

impl Manifest {
    /// Whether `bytes` read as a manifest of [`VERSION`] with nothing outstanding: no item set
    /// aside and not restored, no move begun. Anything the typed read refuses is `false`.
    pub(super) fn settled_empty(bytes: &[u8]) -> bool {
        serde_json::from_slice::<Manifest>(bytes).is_ok_and(|manifest| {
            manifest.version == VERSION
                && manifest.entries.is_empty()
                && manifest.intents.is_empty()
        })
    }
}

#[cfg(any(windows, test))]
impl Default for Manifest {
    fn default() -> Manifest {
        Manifest {
            version: VERSION,
            next: 1,
            entries: Vec::new(),
            intents: Vec::new(),
        }
    }
}

/// Where one matter's set-aside lives.
#[cfg(any(windows, test))]
#[derive(Debug, Clone)]
pub(super) struct Place {
    /// `<data_dir>\sandbox`, where each move's folder chain starts.
    pub(super) sandbox: PathBuf,
    /// `<data_dir>\sandbox\set-aside\<matter>`.
    base: PathBuf,
    matter: MatterId,
}

#[cfg(any(windows, test))]
impl Place {
    pub(super) fn new(data_dir: &Path, matter: &MatterId) -> Place {
        let sandbox = data_dir.join(SANDBOX_DIR);
        let base = sandbox.join(SET_ASIDE_DIR).join(matter.to_string());
        Place {
            sandbox,
            base,
            matter: *matter,
        }
    }

    /// The matter.
    pub(super) fn matter(&self) -> MatterId {
        self.matter
    }

    /// The parts below the sandbox folder of the folder `within` in the pass's folder
    /// `folder`.
    pub(super) fn parts(&self, folder: &str, within: &[OsString]) -> Vec<OsString> {
        [
            OsString::from(SET_ASIDE_DIR),
            OsString::from(self.matter.to_string()),
            OsString::from(folder),
        ]
        .into_iter()
        .chain(within.iter().cloned())
        .collect()
    }

    /// Where `name` in the folder `within` in the pass's folder `folder` is.
    pub(super) fn to(&self, folder: &str, within: &[OsString], name: &OsStr) -> PathBuf {
        within
            .iter()
            .fold(self.base.join(folder), |path, part| path.join(part))
            .join(name)
    }

    /// Makes (or checks) the sandbox folder, `set-aside` and the matter's folder in turn, then
    /// takes the matter's lock, which is held until the file is dropped. Under the lock, the temp
    /// files a crash left in the folder are cleared ([`clear_temps`]).
    pub(super) fn lock(&self) -> Result<File, SetAsideFailure> {
        write::make_sandbox_dir(&self.sandbox).map_err(|e| io_failure(&e))?;
        write::make_sandbox_dir(&self.sandbox.join(SET_ASIDE_DIR)).map_err(|e| io_failure(&e))?;
        write::make_sandbox_dir(&self.base).map_err(|e| io_failure(&e))?;
        let lock = self.base.join(LOCK_FILE);
        write::check_owned_file(&lock).map_err(|e| io_failure(&e))?;
        let held = write::lock_exclusive(&lock).map_err(|e| io_failure(&e))?;
        clear_temps(&self.base);
        Ok(held)
    }

    /// The record, or `None` when there is none. One that can't be read, or of another
    /// version or matter, is [`SetAsideFailure::Record`].
    pub(super) fn read_record(&self) -> Result<Option<Record>, SetAsideFailure> {
        let Some(bytes) = read(&self.base.join(RECORD_FILE))? else {
            return Ok(None);
        };
        match serde_json::from_slice::<Record>(&bytes) {
            Ok(record) if record.version == VERSION && record.matter == self.matter => {
                Ok(Some(record))
            }
            Ok(_) | Err(_) => {
                tracing::warn!("the set-aside record can't be read or is another's");
                Err(SetAsideFailure::Record)
            }
        }
    }

    pub(super) fn write_record(&self, record: &Record) -> Result<(), SetAsideFailure> {
        save(&self.base.join(RECORD_FILE), record)
    }

    /// The manifest; an empty one when there is none.
    pub(super) fn read_manifest(&self) -> Result<Manifest, SetAsideFailure> {
        let Some(bytes) = read(&self.base.join(MANIFEST_FILE))? else {
            return Ok(Manifest::default());
        };
        match serde_json::from_slice::<Manifest>(&bytes) {
            Ok(manifest) if manifest.version == VERSION => Ok(manifest),
            Ok(_) | Err(_) => {
                tracing::warn!("the set-aside manifest can't be read");
                Err(SetAsideFailure::Record)
            }
        }
    }

    pub(super) fn write_manifest(&self, manifest: &Manifest) -> Result<(), SetAsideFailure> {
        save(&self.base.join(MANIFEST_FILE), manifest)
    }
}

/// Removes the temp files a crash left in `base` (the matter's folder) beside the record and the
/// manifest: each name is exactly the temp `write::write_atomic` gives (`write::is_atomic_temp`).
/// Called under the lock, which every writer holds while it writes its temp, so a live write's
/// temp is never removed. Nothing else is touched: another name, a folder, a link, or a file with
/// another hard link stays. A failure is logged and left: the next lock tries again, and no
/// reader ever opens a temp.
#[cfg(any(windows, test))]
fn clear_temps(base: &Path) {
    let targets = [OsStr::new(RECORD_FILE), OsStr::new(MANIFEST_FILE)];
    #[cfg(windows)]
    let cleared = super::ntfs::remove_temps(base, &targets);
    #[cfg(not(windows))]
    let cleared = remove_temps_by_path(base, &targets);
    if let Err(error) = cleared {
        tracing::warn!(%error, "a set-aside temp file could not be cleared");
    }
}

/// The stand-in for [`clear_temps`] off Windows: each name in `base` that is a temp of one of
/// `targets`, removed by `write::remove_owned_file`, which refuses a link, a folder or a second
/// hard link (those stay).
#[cfg(all(test, not(windows)))]
fn remove_temps_by_path(base: &Path, targets: &[&OsStr]) -> Result<(), SetAsideFailure> {
    for entry in std::fs::read_dir(base).map_err(|e| io_failure(&e))? {
        let entry = entry.map_err(|e| io_failure(&e))?;
        let name = entry.file_name();
        if !targets
            .iter()
            .any(|target| write::is_atomic_temp(target, &name))
        {
            continue;
        }
        if let Err(error) = write::remove_owned_file(&entry.path()) {
            tracing::debug!(%error, "a set-aside temp file stays");
        }
    }
    Ok(())
}

/// The file at `path`, checked first; `None` when it is missing.
#[cfg(any(windows, test))]
fn read(path: &Path) -> Result<Option<Vec<u8>>, SetAsideFailure> {
    write::check_owned_file(path).map_err(|e| io_failure(&e))?;
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_failure(&error)),
    }
}

/// Replaces the file at `path` with `value` as JSON, checking the old file and the new one.
#[cfg(any(windows, test))]
fn save(path: &Path, value: &impl Serialize) -> Result<(), SetAsideFailure> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|_unwritable| SetAsideFailure::Io(io::ErrorKind::InvalidData))?;
    bytes.push(b'\n');
    write::check_owned_file(path).map_err(|e| io_failure(&e))?;
    write::write_atomic(path, &bytes).map_err(|e| io_failure(&e))?;
    write::check_owned_file(path).map_err(|e| io_failure(&e))
}

/// `error` as a failure of its kind; the message, which may name a path, goes to the debug log.
#[cfg(any(windows, test))]
pub(super) fn io_failure(error: &io::Error) -> SetAsideFailure {
    tracing::debug!(%error, "a set-aside file-system call failed");
    SetAsideFailure::Io(error.kind())
}

fn to_hex<S: Serializer>(id: &[u8; 16], serializer: S) -> Result<S::Ok, S::Error> {
    let text = id.iter().fold(String::new(), |mut text, byte| {
        text.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        text.push(char::from_digit(u32::from(byte & 0xf), 16).unwrap_or('0'));
        text
    });
    serializer.serialize_str(&text)
}

fn from_hex<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u8; 16], D::Error> {
    let text = String::deserialize(deserializer)?;
    let bad = || serde::de::Error::custom("a file id is 32 hex digits");
    if text.len() != 32 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let mut id = [0_u8; 16];
    for (slot, pair) in id.iter_mut().zip(text.as_bytes().chunks(2)) {
        let pair = std::str::from_utf8(pair).map_err(|_not_text| bad())?;
        *slot = u8::from_str_radix(pair, 16).map_err(|_not_hex| bad())?;
    }
    Ok(id)
}
