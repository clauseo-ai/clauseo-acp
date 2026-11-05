//! The set-aside's files in `<data_dir>\sandbox\set-aside\<matter>\`: the record of the
//! protected names at launch (`record.json`), the manifest of what was set aside
//! (`manifest.json`), and the lock every change holds (`set-aside.lock`). Each folder on the way
//! is made, or checked when there, as the sandbox folder is (`write::make_sandbox_dir`: refused
//! if a link, the protected DACL set again); each file is checked before it is read and after
//! it is written (`write::check_owned_file`), and replaced in one step (`write::write_atomic`).
//!
//! `record.json`: `{"version": 1, "matter": "<id>", "open": true, "names": {".git": {"volume":
//! 1234, "id": "<32 hex digits>"}, "CLAUDE.md": null, ...}}`, `null` for a name absent.
//! `manifest.json`: `{"version": 1, "next": 3, "entries": [<Entry>, ...]}`.

use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{Entry, SetAsideFailure};
use crate::windows::matters::SANDBOX_DIR;
use crate::windows::write;

/// The set-aside's folder in the sandbox folder.
pub(super) const SET_ASIDE_DIR: &str = "set-aside";
const RECORD_FILE: &str = "record.json";
const MANIFEST_FILE: &str = "manifest.json";
const LOCK_FILE: &str = "set-aside.lock";
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

/// What a launch recorded: each protected name at the matter's root and its file ID, and
/// whether the session is still open (no after-session pass yet).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    version: u32,
    matter: MatterId,
    /// No after-session pass has run since the launch that wrote it.
    pub(super) open: bool,
    /// Each protected name, and its file ID at launch, or `None` when it was absent.
    pub(super) names: BTreeMap<String, Option<FileId>>,
}

impl Record {
    pub(super) fn new(
        matter: MatterId,
        open: bool,
        names: BTreeMap<String, Option<FileId>>,
    ) -> Record {
        Record {
            version: VERSION,
            matter,
            open,
            names,
        }
    }
}

/// Every item set aside from a matter and not restored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Manifest {
    version: u32,
    /// The next entry's number.
    pub(super) next: u64,
    pub(super) entries: Vec<Entry>,
}

impl Default for Manifest {
    fn default() -> Manifest {
        Manifest {
            version: VERSION,
            next: 1,
            entries: Vec::new(),
        }
    }
}

/// Where one matter's set-aside lives.
#[derive(Debug, Clone)]
pub(super) struct Place {
    /// `<data_dir>\sandbox`, where each move's folder chain starts.
    pub(super) sandbox: PathBuf,
    /// `<data_dir>\sandbox\set-aside\<matter>`.
    base: PathBuf,
    matter: MatterId,
}

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

    /// The parts below the sandbox folder of the pass's folder `folder`.
    pub(super) fn parts(&self, folder: &str) -> [String; 3] {
        [
            SET_ASIDE_DIR.to_owned(),
            self.matter.to_string(),
            folder.to_owned(),
        ]
    }

    /// Where `name` in the pass's folder `folder` is.
    pub(super) fn to(&self, folder: &str, name: &str) -> PathBuf {
        self.base.join(folder).join(name)
    }

    /// Makes (or checks) the sandbox folder, `set-aside` and the matter's folder in turn, then
    /// takes the matter's lock, which is held until the file is dropped.
    pub(super) fn lock(&self) -> Result<File, SetAsideFailure> {
        write::make_sandbox_dir(&self.sandbox).map_err(|e| io_failure(&e))?;
        write::make_sandbox_dir(&self.sandbox.join(SET_ASIDE_DIR)).map_err(|e| io_failure(&e))?;
        write::make_sandbox_dir(&self.base).map_err(|e| io_failure(&e))?;
        let lock = self.base.join(LOCK_FILE);
        write::check_owned_file(&lock).map_err(|e| io_failure(&e))?;
        write::lock_exclusive(&lock).map_err(|e| io_failure(&e))
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

/// The file at `path`, checked first; `None` when it is missing.
fn read(path: &Path) -> Result<Option<Vec<u8>>, SetAsideFailure> {
    write::check_owned_file(path).map_err(|e| io_failure(&e))?;
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_failure(&error)),
    }
}

/// Replaces the file at `path` with `value` as JSON, checking the old file and the new one.
fn save(path: &Path, value: &impl Serialize) -> Result<(), SetAsideFailure> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|_unwritable| SetAsideFailure::Io(io::ErrorKind::InvalidData))?;
    bytes.push(b'\n');
    write::check_owned_file(path).map_err(|e| io_failure(&e))?;
    write::write_atomic(path, &bytes).map_err(|e| io_failure(&e))?;
    write::check_owned_file(path).map_err(|e| io_failure(&e))
}

/// `error` as a failure of its kind; the message, which may name a path, goes to the debug log.
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
