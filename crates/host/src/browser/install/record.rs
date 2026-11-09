//! The install record (decision record 0029, "The install record"): what the download put in
//! `browsers/chrome-<version>/`, written by the host once the folder is in place and checked
//! again at every browser start (`chrome::find_checked`), before the binary's path goes to the
//! guard.
//!
//! It sits in `<app data>/browser-pins/` ([`RECORD_FILE`]), beside the stored browser pins and
//! not in the browsers folder, where agents cannot write either. It holds the version, the
//! platform, the zip's sha256 and the whole tree: every entry's path relative to the version
//! folder (`/` between parts, sorted), its kind, its mode, and for a file its size and sha256,
//! for a link its target as text. The check at launch walks the folder again and wants the same
//! listing (an added, removed or changed entry is refused before anything is hashed), then the
//! same sha256 of every file, hashed on several threads at once. A missing, unreadable or
//! unknown record refuses the browser too.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};

use crate::browser::pins::PINS_FOLDER;
use crate::write;

/// The record's file name in `<app data>/browser-pins/`.
pub const RECORD_FILE: &str = "installed-chrome-v1.json";

/// The record's `schema`; any other is refused.
const SCHEMA: u32 = 1;

/// The largest record read: a Chrome for Testing tree's is well under 1 MiB.
const MAX_RECORD_BYTES: u64 = 16 * 1024 * 1024;

/// The most entries a tree may hold, as the unpack check allows (`archive::MAX_ENTRIES`).
const MAX_ENTRIES: usize = 100_000;

/// The most threads that hash at once.
const MAX_HASHERS: usize = 8;

/// What the host installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallRecord {
    /// [`SCHEMA`].
    pub schema: u32,
    /// The pinned version installed, dotted digits, exactly as the pins wrote it.
    pub version: String,
    /// Chrome for Testing's platform name (`mac-arm64`, `mac-x64`, `win64`).
    pub platform: String,
    /// The zip's sha256, as pinned: 64 lowercase hex digits.
    pub zip_sha256: String,
    /// Every entry under the version folder, sorted by path.
    pub tree: Vec<Entry>,
}

/// One entry of the tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Its path relative to the version folder, `/` between parts.
    pub path: String,
    /// What it is.
    pub kind: Kind,
    /// Its permission bits (`mode & 0o7777`) on Unix; on Windows `0o444` when read-only, else
    /// `0o666`.
    pub mode: u32,
    /// A file's size in bytes; 0 for the rest.
    pub size: u64,
    /// A file's sha256, 64 lowercase hex digits; empty for the rest.
    pub sha256: String,
    /// A link's target as text; empty for the rest.
    pub target: String,
}

/// What an entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A regular file.
    File,
    /// A folder.
    Dir,
    /// A symbolic link (macOS app bundles hold some).
    Link,
}

/// Why the check at launch refused the folder. Never names a path, so it can be logged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// No record.
    Missing,
    /// The record could not be read or parsed, or its schema is unknown.
    Unreadable,
    /// The record is for another version or platform than the one asked for.
    OtherVersion,
    /// The folder's listing differs: an entry added, removed, or of another kind, mode, size
    /// or link target.
    Listing,
    /// A file's content differs from its recorded sha256.
    Content,
    /// The folder could not be walked or a file read.
    Io,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Missing => "no install record",
            Self::Unreadable => "the install record is unreadable",
            Self::OtherVersion => "the install record is for another version",
            Self::Listing => "the folder's files differ from the install record",
            Self::Content => "a file's content differs from the install record",
            Self::Io => "the folder could not be read",
        })
    }
}

/// Where the record lives for the app data folder `app_data`.
pub fn record_path(app_data: &Path) -> PathBuf {
    app_data.join(PINS_FOLDER).join(RECORD_FILE)
}

/// Walks `folder` (the installed `chrome-<version>/`), hashes every file and writes the record
/// for `version`, `platform` and `zip_sha256` (the folder `browser-pins/` made if missing, the
/// file replaced atomically).
pub fn write(
    app_data: &Path,
    platform: &str,
    version: &str,
    zip_sha256: &str,
    folder: &Path,
) -> std::io::Result<()> {
    let mut tree = list(folder)?;
    let files: Vec<PathBuf> = file_paths(folder, &tree);
    let digests = hash_all(&files)?;
    let mut digests = digests.into_iter();
    for entry in tree.iter_mut().filter(|entry| entry.kind == Kind::File) {
        entry.sha256 = digests.next().map(hex).unwrap_or_default();
    }
    let record = InstallRecord {
        schema: SCHEMA,
        version: version.to_owned(),
        platform: platform.to_owned(),
        zip_sha256: zip_sha256.to_owned(),
        tree,
    };
    let bytes = serde_json::to_vec(&record).map_err(std::io::Error::other)?;
    let path = record_path(app_data);
    write::create_dir_all(path.parent().unwrap_or(app_data))?;
    write::sweep_temp_files(&path);
    write::replace_private_file(&path, &bytes)
}

/// Checks `folder` against the record for `version` and `platform`: the same listing, then the
/// same content.
pub fn verify(
    app_data: &Path,
    platform: &str,
    version: &str,
    folder: &Path,
) -> Result<(), Refusal> {
    let record = read(app_data)?;
    if record.version != version || record.platform != platform {
        return Err(Refusal::OtherVersion);
    }
    let found = list(folder).map_err(|_unsaid| Refusal::Io)?;
    let same_listing = found.len() == record.tree.len()
        && found.iter().zip(&record.tree).all(|(found, recorded)| {
            found.path == recorded.path
                && found.kind == recorded.kind
                && found.mode == recorded.mode
                && found.size == recorded.size
                && found.target == recorded.target
        });
    if !same_listing {
        return Err(Refusal::Listing);
    }
    let files = file_paths(folder, &record.tree);
    let digests = hash_all(&files).map_err(|_unsaid| Refusal::Io)?;
    let recorded = record.tree.iter().filter(|entry| entry.kind == Kind::File);
    if digests
        .into_iter()
        .zip(recorded)
        .all(|(digest, entry)| hex(digest) == entry.sha256)
    {
        Ok(())
    } else {
        Err(Refusal::Content)
    }
}

/// The record in `app_data`, parsed, its schema known.
fn read(app_data: &Path) -> Result<InstallRecord, Refusal> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    let file = match std::fs::File::open(record_path(app_data)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Refusal::Missing);
        }
        Err(_) => return Err(Refusal::Unreadable),
    };
    file.take(MAX_RECORD_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_unsaid| Refusal::Unreadable)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_RECORD_BYTES {
        return Err(Refusal::Unreadable);
    }
    let record: InstallRecord =
        serde_json::from_slice(&bytes).map_err(|_unsaid| Refusal::Unreadable)?;
    if record.schema != SCHEMA {
        return Err(Refusal::Unreadable);
    }
    Ok(record)
}

/// The listing of `root`, sorted by path, with no sha256 yet. Links are listed, never
/// followed; anything that is not a file, a folder or a link, a name that is not UTF-8, or
/// more than [`MAX_ENTRIES`] entries fails the walk.
fn list(root: &Path) -> std::io::Result<Vec<Entry>> {
    let mut tree = Vec::new();
    let mut folders: Vec<(PathBuf, String)> = vec![(root.to_owned(), String::new())];
    while let Some((folder, prefix)) = folders.pop() {
        for entry in std::fs::read_dir(&folder)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| std::io::Error::other("a name that is not UTF-8"))?;
            let relative = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            };
            let meta = std::fs::symlink_metadata(entry.path())?;
            let kind = meta.file_type();
            let (kind, size, target) = if kind.is_symlink() {
                let target = std::fs::read_link(entry.path())?;
                let target = target
                    .to_str()
                    .ok_or_else(|| std::io::Error::other("a link target that is not UTF-8"))?
                    .to_owned();
                (Kind::Link, 0, target)
            } else if kind.is_dir() {
                folders.push((entry.path(), relative.clone()));
                (Kind::Dir, 0, String::new())
            } else if kind.is_file() {
                (Kind::File, meta.len(), String::new())
            } else {
                return Err(std::io::Error::other(
                    "an entry that is not a file or a folder",
                ));
            };
            tree.push(Entry {
                path: relative,
                kind,
                mode: mode(&meta),
                size,
                sha256: String::new(),
                target,
            });
            if tree.len() > MAX_ENTRIES {
                return Err(std::io::Error::other("too many entries"));
            }
        }
    }
    tree.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(tree)
}

/// The paths of `tree`'s files under `root`, in `tree`'s order.
fn file_paths(root: &Path, tree: &[Entry]) -> Vec<PathBuf> {
    tree.iter()
        .filter(|entry| entry.kind == Kind::File)
        .map(|entry| {
            entry
                .path
                .split('/')
                .fold(root.to_owned(), |path, part| path.join(part))
        })
        .collect()
}

/// One hashing thread's files: each one's place in the list and its sha256.
type Hashed = std::io::Result<Vec<(usize, [u8; 32])>>;

/// The sha256 of each of `files`, in order, hashed by up to [`MAX_HASHERS`] threads, each
/// taking the next file not yet taken. The first failure fails the whole.
fn hash_all(files: &[PathBuf]) -> std::io::Result<Vec<[u8; 32]>> {
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .clamp(1, MAX_HASHERS)
        .min(files.len().max(1));
    // The largest files first, so one big file never starts last.
    let mut order: Vec<(usize, &PathBuf, u64)> = files
        .iter()
        .enumerate()
        .map(|(at, path)| {
            let size = std::fs::symlink_metadata(path).map_or(0, |meta| meta.len());
            (at, path, size)
        })
        .collect();
    order.sort_by_key(|&(_, _, size)| std::cmp::Reverse(size));
    let next = AtomicUsize::new(0);
    let hashed: Vec<Hashed> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    while let Some(&(at, path, _)) = order.get(next.fetch_add(1, Ordering::Relaxed))
                    {
                        done.push((at, super::sha256_file(path)?));
                    }
                    Ok(done)
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| {
                worker
                    .join()
                    .unwrap_or_else(|_| Err(std::io::Error::other("a hashing thread panicked")))
            })
            .collect()
    });
    let mut digests = vec![[0_u8; 32]; files.len()];
    for done in hashed {
        for (at, digest) in done? {
            if let Some(slot) = digests.get_mut(at) {
                *slot = digest;
            }
        }
    }
    Ok(digests)
}

/// `digest` as 64 lowercase hex digits.
fn hex(digest: [u8; 32]) -> String {
    use std::fmt::Write as _;
    digest
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _infallible = write!(hex, "{byte:02x}");
            hex
        })
}

/// An entry's mode as the record keeps it.
#[cfg(unix)]
fn mode(meta: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    meta.permissions().mode() & 0o7777
}

/// An entry's mode as the record keeps it.
#[cfg(not(unix))]
fn mode(meta: &std::fs::Metadata) -> u32 {
    if meta.permissions().readonly() {
        0o444
    } else {
        0o666
    }
}

#[cfg(test)]
mod tests;
