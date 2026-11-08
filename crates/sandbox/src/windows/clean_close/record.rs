//! The clean-close record, `<data_dir>\sandbox\clean\<matter>.bin`: what a later launch checks
//! the journal against. It lives in the sandbox folder, under its protected DACL (the desktop
//! user, `SYSTEM` and `Administrators`; no pool user), and is checked before it is read and
//! after it is written (`write::check_owned_file`), replaced in one step (`write::write_atomic`)
//! and removed through the same check (`write::remove_owned_file`).
//!
//! The format: 16 magic bytes, the version (`u32`, little-endian), the header's length (`u32`)
//! and the header as JSON ([`Header`]), then each volume's items in the header's order, each
//! 17 bytes: the file reference (`u64`), its folder's (`u64`) and flags (`u8`: 1 a folder the
//! collection listed, 2 an own root). The IDs are binary so 50,000 of them load in a few ms.

use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;
use serde::{Deserialize, Serialize};

use super::journal::Usn;
use crate::windows::admit::PathKind;
use crate::windows::matters::SANDBOX_DIR;
use crate::windows::write;

use super::inputs::WalkInputs;

/// The records' folder in the sandbox folder.
const CLEAN_DIR: &str = "clean";
const MAGIC: &[u8; 16] = b"CLAUSEO-CLEAN\0\0\0";
/// The version written, and the only one read. 2 added each volume's first record.
pub(super) const VERSION: u32 = 2;
const ITEM: usize = 17;
/// The most items a record holds: past this the walk is cheaper than reading it back.
const MOST_ITEMS: usize = 4_000_000;

/// An own root as the record knows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RootMark {
    pub(super) path: PathBuf,
    pub(super) kind: PathKind,
    pub(super) volume: u64,
    pub(super) id: u64,
}

/// One volume's journal at the cursor, and how many items follow for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VolumeMark {
    pub(super) volume: u64,
    pub(super) journal: u64,
    pub(super) cursor: Usn,
    /// The record at the cursor when the record was written; `None` when there was none yet.
    /// A launch finds the same one there, or the volume went back and on (a restored image
    /// that has written past the cursor since).
    pub(super) first: Option<FirstRecord>,
    pub(super) items: usize,
}

/// A journal record, as a later launch must find it again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FirstRecord {
    pub(super) usn: Usn,
    pub(super) time: i64,
    pub(super) file: u64,
}

/// The record's JSON part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Header {
    pub(super) matter: MatterId,
    pub(super) inputs: WalkInputs,
    pub(super) roots: Vec<RootMark>,
    pub(super) volumes: Vec<VolumeMark>,
}

/// One name in an own tree at the clean close.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Item {
    pub(super) id: u64,
    pub(super) parent: u64,
    pub(super) flags: u8,
}

/// [`Item::flags`]: a folder the collection listed.
pub(super) const FOLDER: u8 = 1;
/// [`Item::flags`]: an own root.
pub(super) const ROOT: u8 = 2;

/// A volume's items, sorted by ID then folder, for lookups by ID.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Known {
    items: Vec<Item>,
}

impl Known {
    pub(super) fn new(mut items: Vec<Item>) -> Known {
        items.sort_unstable();
        items.dedup();
        Known { items }
    }

    pub(super) fn items(&self) -> &[Item] {
        &self.items
    }

    /// Every item with `id`: one per name it had (a file with several links in the trees).
    pub(super) fn get(&self, id: u64) -> &[Item] {
        let start = self.items.partition_point(|item| item.id < id);
        let end = self.items.partition_point(|item| item.id <= id);
        self.items.get(start..end).unwrap_or_default()
    }

    /// Whether `id` is a folder the collection listed (an own root included).
    pub(super) fn folder(&self, id: u64) -> bool {
        self.get(id).iter().any(|item| item.flags & FOLDER != 0)
    }

    /// Whether `id` is an own root.
    pub(super) fn root(&self, id: u64) -> bool {
        self.get(id).iter().any(|item| item.flags & ROOT != 0)
    }

    pub(super) fn len(&self) -> usize {
        self.items.len()
    }
}

/// The whole record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CleanRecord {
    pub(super) header: Header,
    /// Each volume's items, in the header's order.
    pub(super) known: Vec<Known>,
}

/// Why a record couldn't be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Unusable {
    /// It can't be read or parsed, or is another matter's.
    Unreadable,
    /// It was written in another version.
    Version,
}

/// `<data_dir>\sandbox\clean\<matter>.bin`.
pub(crate) fn record_path(data_dir: &Path, matter: &MatterId) -> PathBuf {
    data_dir
        .join(SANDBOX_DIR)
        .join(CLEAN_DIR)
        .join(format!("{matter}.bin"))
}

/// Removes `matter`'s record, if there is one.
pub(crate) fn remove(data_dir: &Path, matter: &MatterId) -> io::Result<()> {
    write::remove_owned_file(&record_path(data_dir, matter))
}

/// `matter`'s record: `Ok(None)` when there is none.
pub(super) fn read(
    data_dir: &Path,
    matter: &MatterId,
) -> io::Result<Option<Result<CleanRecord, Unusable>>> {
    let path = record_path(data_dir, matter);
    write::check_owned_file(&path)?;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(Some(parse(&bytes).and_then(|record| {
        if record.header.matter == *matter {
            Ok(record)
        } else {
            Err(Unusable::Unreadable)
        }
    })))
}

/// Writes `record` in one step, making or checking the folders on the way.
pub(super) fn write(data_dir: &Path, record: &CleanRecord) -> io::Result<()> {
    let sandbox = data_dir.join(SANDBOX_DIR);
    write::make_sandbox_dir(&sandbox)?;
    write::make_sandbox_dir(&sandbox.join(CLEAN_DIR))?;
    let path = record_path(data_dir, &record.header.matter);
    let bytes = render(record)?;
    write::check_owned_file(&path)?;
    write::write_atomic(&path, &bytes)?;
    write::check_owned_file(&path)
}

pub(super) fn render(record: &CleanRecord) -> io::Result<Vec<u8>> {
    let header = serde_json::to_vec(&record.header).map_err(io::Error::other)?;
    let len = u32::try_from(header.len()).map_err(io::Error::other)?;
    let count: usize = record.known.iter().map(Known::len).sum();
    let mut bytes = Vec::with_capacity(24 + header.len() + count.saturating_mul(ITEM));
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&len.to_le_bytes());
    bytes.extend_from_slice(&header);
    for (known, mark) in record.known.iter().zip(&record.header.volumes) {
        if known.len() != mark.items {
            return Err(io::Error::other("a volume's item count isn't its header's"));
        }
        for item in known.items() {
            bytes.extend_from_slice(&item.id.to_le_bytes());
            bytes.extend_from_slice(&item.parent.to_le_bytes());
            bytes.push(item.flags);
        }
    }
    Ok(bytes)
}

pub(super) fn parse(bytes: &[u8]) -> Result<CleanRecord, Unusable> {
    let bad = Unusable::Unreadable;
    let (magic, rest) = bytes.split_at_checked(MAGIC.len()).ok_or(bad)?;
    if magic != MAGIC {
        return Err(bad);
    }
    let (version, rest) = rest.split_first_chunk::<4>().ok_or(bad)?;
    if u32::from_le_bytes(*version) != VERSION {
        return Err(Unusable::Version);
    }
    let (len, rest) = rest.split_first_chunk::<4>().ok_or(bad)?;
    let len = usize::try_from(u32::from_le_bytes(*len)).map_err(|_too_long| bad)?;
    let (header, mut rest) = rest.split_at_checked(len).ok_or(bad)?;
    let header: Header = serde_json::from_slice(header).map_err(|_unparsed| bad)?;
    let total = header
        .volumes
        .iter()
        .try_fold(0_usize, |sum, v| sum.checked_add(v.items))
        .ok_or(bad)?;
    if total > MOST_ITEMS || Some(rest.len()) != total.checked_mul(ITEM) {
        return Err(bad);
    }
    let mut known = Vec::with_capacity(header.volumes.len());
    for mark in &header.volumes {
        let size = mark.items.checked_mul(ITEM).ok_or(bad)?;
        let (mine, next) = rest.split_at_checked(size).ok_or(bad)?;
        rest = next;
        let items = mine
            .as_chunks::<ITEM>()
            .0
            .iter()
            .map(|chunk| {
                let (id, rest) = chunk.split_first_chunk::<8>().ok_or(bad)?;
                let (parent, rest) = rest.split_first_chunk::<8>().ok_or(bad)?;
                Ok(Item {
                    id: u64::from_le_bytes(*id),
                    parent: u64::from_le_bytes(*parent),
                    flags: rest.first().copied().ok_or(bad)?,
                })
            })
            .collect::<Result<Vec<Item>, Unusable>>()?;
        known.push(Known::new(items));
    }
    // Each volume once, and the items as sorted and deduplicated as written.
    let mut volumes: Vec<u64> = header.volumes.iter().map(|v| v.volume).collect();
    volumes.sort_unstable();
    volumes.dedup();
    let counts_kept = known
        .iter()
        .zip(&header.volumes)
        .all(|(k, v)| k.len() == v.items);
    if volumes.len() != header.volumes.len() || !counts_kept {
        return Err(bad);
    }
    Ok(CleanRecord { header, known })
}
