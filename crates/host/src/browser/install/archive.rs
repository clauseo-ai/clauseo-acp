//! The unpack checks: the zip's entry names before the system tool unpacks it
//! ([`check_zip`]), the tree it made afterwards ([`check_tree`]), and the start-up sweep of the
//! working folders a crash left ([`sweep_partials`]).
//!
//! The name check reads the zip's central directory and each entry's local header, so whichever
//! one the tool trusts, a name that would land outside the folder is refused before anything is
//! written: an absolute name, a drive (`C:`) or any `:`, a `..` part (with `/` or `\` as the
//! separator), a control character, a name that is not ASCII, a name twice (ignoring ASCII
//! case, as macOS and Windows do), a Unicode-path extra field naming something else, and any
//! entry under an entry that is a symbolic link (writing through it would leave the folder).
//! The walk then refuses any link whose target is absolute or, resolved, leaves the folder,
//! and anything that is not a file, a folder or a link.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path};

use clauseo_protocol::BrowserInstallFailure;

use super::{Failure, PARTIAL_PREFIX};
use crate::write;

/// The most entries a zip may hold: Chrome for Testing's hold a few hundred.
const MAX_ENTRIES: u64 = 100_000;

/// The largest central directory read: far above Chrome for Testing's.
const MAX_DIRECTORY_BYTES: u64 = 64 * 1024 * 1024;

/// The end of central directory record's fixed part, and its longest comment.
const EOCD_LEN: u64 = 22;
const MAX_COMMENT: u64 = 0xFFFF;

const EOCD_SIG: u32 = 0x0605_4b50;
const ZIP64_LOCATOR_SIG: u32 = 0x0706_4b50;
const ZIP64_EOCD_SIG: u32 = 0x0606_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const LOCAL_SIG: u32 = 0x0403_4b50;

/// The extra fields read: zip64 sizes and offset, and Info-ZIP's Unicode path.
const ZIP64_EXTRA: u16 = 0x0001;
const UNICODE_PATH_EXTRA: u16 = 0x7075;

/// An `unpack` failure.
fn bad(message: impl Into<String>) -> Failure {
    Failure::new(BrowserInstallFailure::Unpack, message)
}

/// Reads `N` bytes at `at` of `bytes`, or fails as a truncated zip.
fn take<const N: usize>(bytes: &[u8], at: usize) -> Result<[u8; N], Failure> {
    bytes
        .get(at..at.saturating_add(N))
        .and_then(|slice| slice.try_into().ok())
        .ok_or_else(|| bad("the zip is truncated"))
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, Failure> {
    take::<2>(bytes, at).map(u16::from_le_bytes)
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, Failure> {
    take::<4>(bytes, at).map(u32::from_le_bytes)
}

fn u64_at(bytes: &[u8], at: usize) -> Result<u64, Failure> {
    take::<8>(bytes, at).map(u64::from_le_bytes)
}

/// `len` bytes of `file` at `offset`.
fn read_at(file: &mut File, offset: u64, len: u64) -> Result<Vec<u8>, Failure> {
    let io = |error: std::io::Error| bad(format!("reading the zip: {error}"));
    file.seek(SeekFrom::Start(offset)).map_err(io)?;
    let mut bytes = Vec::new();
    file.take(len).read_to_end(&mut bytes).map_err(io)?;
    if u64::try_from(bytes.len()).ok() == Some(len) {
        Ok(bytes)
    } else {
        Err(bad("the zip is truncated"))
    }
}

/// Where the central directory is and how many entries it holds.
struct Directory {
    entries: u64,
    offset: u64,
    size: u64,
}

/// Finds the end of central directory record (and its zip64 form when the plain one is full).
fn directory(file: &mut File, len: u64) -> Result<Directory, Failure> {
    let start = len.saturating_sub(EOCD_LEN + MAX_COMMENT);
    let tail = read_at(file, start, len - start)?;
    let at = (0..=tail.len().saturating_sub(4))
        .rev()
        .find(|&at| u32_at(&tail, at).is_ok_and(|sig| sig == EOCD_SIG))
        .ok_or_else(|| bad("the zip has no end of central directory"))?;
    let entries = u16_at(&tail, at + 10)?;
    let size = u32_at(&tail, at + 12)?;
    let offset = u32_at(&tail, at + 16)?;
    if entries != u16::MAX && size != u32::MAX && offset != u32::MAX {
        return Ok(Directory {
            entries: entries.into(),
            offset: offset.into(),
            size: size.into(),
        });
    }
    let locator = at
        .checked_sub(20)
        .ok_or_else(|| bad("the zip64 locator is missing"))?;
    if u32_at(&tail, locator)? != ZIP64_LOCATOR_SIG {
        return Err(bad("the zip64 locator is missing"));
    }
    let record = read_at(file, u64_at(&tail, locator + 8)?, 56)?;
    if u32_at(&record, 0)? != ZIP64_EOCD_SIG {
        return Err(bad("the zip64 end of central directory is missing"));
    }
    Ok(Directory {
        entries: u64_at(&record, 32)?,
        size: u64_at(&record, 40)?,
        offset: u64_at(&record, 48)?,
    })
}

/// The extra field `id`'s data in `extra`, if there.
fn extra_field(extra: &[u8], id: u16) -> Result<Option<&[u8]>, Failure> {
    let mut at = 0;
    while at + 4 <= extra.len() {
        let kind = u16_at(extra, at)?;
        let len = usize::from(u16_at(extra, at + 2)?);
        let data = extra
            .get(at + 4..at + 4 + len)
            .ok_or_else(|| bad("an extra field is truncated"))?;
        if kind == id {
            return Ok(Some(data));
        }
        at += 4 + len;
    }
    Ok(None)
}

/// Refuses a Unicode-path extra field whose name is not `name`.
fn check_unicode_path(extra: &[u8], name: &str) -> Result<(), Failure> {
    if let Some(data) = extra_field(extra, UNICODE_PATH_EXTRA)? {
        let other = data.get(5..).unwrap_or_default();
        if other != name.as_bytes() {
            return Err(bad(format!(
                "{name:?} has a Unicode path naming another file"
            )));
        }
    }
    Ok(())
}

/// Checks one entry name: ASCII, relative, `/` as the only separator, no drive or `:`, no control
/// character, no `~` (Windows short names), and every segment spelled one way only: not empty
/// (a folder entry's one trailing `/` aside), not `.` or `..`, not ending in a dot or a space
/// (Windows drops those). So each path has one spelling, and the entry-under-a-link check below
/// compares like with like. ASCII only, because macOS and Windows match names ignoring Unicode
/// case and normalisation.
pub fn check_name(name: &str) -> Result<(), Failure> {
    let refuse = |why: &str| Err(bad(format!("the zip entry {name:?} is refused: {why}")));
    if name.is_empty() {
        return refuse("it is empty");
    }
    if name.starts_with(['/', '\\']) {
        return refuse("it is absolute");
    }
    if name.contains(':') {
        return refuse("it names a drive or a stream");
    }
    if !name.is_ascii() {
        return refuse("it is not ASCII");
    }
    if name.chars().any(char::is_control) {
        return refuse("it holds a control character");
    }
    if name.contains('\\') {
        return refuse("it holds a backslash");
    }
    // Windows reads `ABCDEF~1` as the 8.3 short name of a longer folder, a second spelling.
    if name.contains('~') {
        return refuse("it holds a ~ (a Windows short name)");
    }
    for part in segments(name) {
        if part == ".." {
            return refuse("it climbs out with ..");
        }
        if part.is_empty() || part == "." {
            return refuse("it has an empty or . segment");
        }
        if part.ends_with(['.', ' ']) {
            return refuse("a segment ends in a dot or a space");
        }
    }
    Ok(())
}

/// A name's segments, without a folder entry's one trailing `/`.
fn segments(name: &str) -> std::str::Split<'_, char> {
    name.strip_suffix('/').unwrap_or(name).split('/')
}

/// A checked name as the file systems compare it: its segments, ASCII case folded.
fn folded(name: &str) -> Vec<String> {
    segments(name).map(str::to_ascii_lowercase).collect()
}

/// Whether a central directory entry is a symbolic link: made on Unix (3) or OS X (19), with
/// the link file type.
fn is_symlink(made_by: u16, external: u32) -> bool {
    matches!(made_by >> 8, 3 | 19) && (external >> 16) & 0o170_000 == 0o120_000
}

/// Checks every entry name in the zip at `path`, in its central directory and its local
/// headers, before anything is unpacked.
pub fn check_zip(path: &Path) -> Result<(), Failure> {
    let mut file = File::open(path).map_err(|error| bad(format!("opening the zip: {error}")))?;
    let len = file
        .metadata()
        .map_err(|error| bad(format!("reading the zip: {error}")))?
        .len();
    let dir = directory(&mut file, len)?;
    if dir.entries > MAX_ENTRIES || dir.size > MAX_DIRECTORY_BYTES {
        return Err(bad("the zip's directory is too large"));
    }
    let central = read_at(&mut file, dir.offset, dir.size)?;
    let mut names: BTreeSet<Vec<String>> = BTreeSet::new();
    let mut links = Vec::new();
    let mut at = 0;
    for _ in 0..dir.entries {
        if u32_at(&central, at)? != CENTRAL_SIG {
            return Err(bad("a central directory entry is malformed"));
        }
        let made_by = u16_at(&central, at + 4)?;
        let name_len = usize::from(u16_at(&central, at + 28)?);
        let extra_len = usize::from(u16_at(&central, at + 30)?);
        let comment_len = usize::from(u16_at(&central, at + 32)?);
        let compressed = u32_at(&central, at + 20)?;
        let uncompressed = u32_at(&central, at + 24)?;
        let external = u32_at(&central, at + 38)?;
        let local = u32_at(&central, at + 42)?;
        let name_bytes = central
            .get(at + 46..at + 46 + name_len)
            .ok_or_else(|| bad("the zip is truncated"))?;
        let extra = central
            .get(at + 46 + name_len..at + 46 + name_len + extra_len)
            .ok_or_else(|| bad("the zip is truncated"))?;
        let name = std::str::from_utf8(name_bytes)
            .map_err(|_utf8| bad("a zip entry's name is not UTF-8"))?;
        check_name(name)?;
        check_unicode_path(extra, name)?;
        let folded = folded(name);
        if !names.insert(folded.clone()) {
            return Err(bad(format!("the zip holds {name:?} twice")));
        }
        if is_symlink(made_by, external) {
            links.push(folded);
        }
        let local = if local == u32::MAX {
            local_from_zip64(extra, [uncompressed, compressed])?
        } else {
            local.into()
        };
        check_local(&mut file, local, name_bytes)?;
        at += 46 + name_len + extra_len + comment_len;
    }
    for link in &links {
        let under = |name: &&Vec<String>| name.len() > link.len() && name.starts_with(link);
        if let Some(name) = names.iter().find(under) {
            let (name, link) = (name.join("/"), link.join("/"));
            return Err(bad(format!("{name:?} sits under the link {link:?}")));
        }
    }
    Ok(())
}

/// The local header offset from the zip64 extra field: after the uncompressed and compressed
/// sizes that are full in the central directory, in that order (APPNOTE 4.5.3).
fn local_from_zip64(extra: &[u8], sizes: [u32; 2]) -> Result<u64, Failure> {
    let data = extra_field(extra, ZIP64_EXTRA)?.ok_or_else(|| bad("a zip64 offset is missing"))?;
    let skip = sizes.iter().filter(|&&size| size == u32::MAX).count() * 8;
    u64_at(data, skip)
}

/// The local header at `offset` must name the entry as the central directory does.
fn check_local(file: &mut File, offset: u64, name: &[u8]) -> Result<(), Failure> {
    let header = read_at(file, offset, 30)?;
    if u32_at(&header, 0)? != LOCAL_SIG {
        return Err(bad("a local header is missing"));
    }
    let name_len = u64::from(u16_at(&header, 26)?);
    let extra_len = u64::from(u16_at(&header, 28)?);
    let rest = read_at(file, offset + 30, name_len + extra_len)?;
    let (local, extra) = rest.split_at(usize::try_from(name_len).unwrap_or(usize::MAX));
    if local != name {
        return Err(bad("a local header names another file than the directory"));
    }
    let name = std::str::from_utf8(name).map_err(|_utf8| bad("a name is not UTF-8"))?;
    check_unicode_path(extra, name)
}

/// Walks what the tool unpacked into `root`: each link's target must be relative and, resolved,
/// inside `root`; each other entry a file or a folder. Links are never followed by the walk.
pub fn check_tree(root: &Path) -> Result<(), Failure> {
    let io = |error: std::io::Error| bad(format!("walking the unpacked folder: {error}"));
    let real_root = dunce::canonicalize(root).map_err(io)?;
    let mut folders = vec![root.to_owned()];
    let mut seen: u64 = 0;
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).map_err(io)? {
            let path = entry.map_err(io)?.path();
            seen += 1;
            if seen > MAX_ENTRIES {
                return Err(bad("the unpacked folder holds too many entries"));
            }
            let kind = std::fs::symlink_metadata(&path).map_err(io)?.file_type();
            if kind.is_symlink() {
                check_link(root, &path, &real_root)?;
            } else if kind.is_dir() {
                folders.push(path);
            } else if !kind.is_file() {
                return Err(bad(format!("{} is not a file or a folder", path.display())));
            }
        }
    }
    Ok(())
}

/// One link under `root`: a relative target that never climbs above `root`, read as text step
/// by step from the link's folder (so it cannot leave and come back in by `root`'s own name,
/// which changes at the rename), and that, resolved, stays inside `real_root`.
fn check_link(root: &Path, link: &Path, real_root: &Path) -> Result<(), Failure> {
    let target = std::fs::read_link(link)
        .map_err(|error| bad(format!("reading the link {}: {error}", link.display())))?;
    if target
        .components()
        .any(|part| matches!(part, Component::RootDir | Component::Prefix(_)))
    {
        return Err(bad(format!("the link {} is absolute", link.display())));
    }
    let below = link
        .parent()
        .and_then(|folder| folder.strip_prefix(root).ok())
        .ok_or_else(|| bad(format!("the link {} is outside the walk", link.display())))?;
    let mut depth = below.components().count();
    for part in target.components() {
        match part {
            Component::ParentDir => {
                depth = depth.checked_sub(1).ok_or_else(|| {
                    bad(format!(
                        "the link {} climbs above its folder",
                        link.display()
                    ))
                })?;
            }
            Component::Normal(_) => depth += 1,
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    match dunce::canonicalize(link) {
        Ok(real) if real.starts_with(real_root) => Ok(()),
        Ok(_) => Err(bad(format!("the link {} leads out", link.display()))),
        Err(_) => Err(bad(format!("the link {} leads nowhere", link.display()))),
    }
}

/// Removes every `.partial-*` entry a crashed or killed download left in `browsers`: a folder
/// with all it holds (links inside removed, never followed), a link or file itself only.
/// Best effort: one that cannot be removed is logged and stays. File system calls: off the
/// actor.
pub fn sweep_partials(browsers: &Path) {
    // A link at `browsers` is never followed: what it points at is not Clauseo's.
    if !std::fs::symlink_metadata(browsers).is_ok_and(|meta| meta.is_dir()) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(browsers) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry
            .file_name()
            .as_encoded_bytes()
            .starts_with(PARTIAL_PREFIX.as_bytes())
        {
            continue;
        }
        let path = entry.path();
        let removed = match entry.file_type() {
            Ok(kind) if kind.is_dir() => write::remove_tree(&path),
            Ok(_) => write::remove_link(&path),
            Err(error) => Err(error),
        };
        if let Err(error) = removed {
            tracing::warn!(
                error = %crate::log_file::Cause(&error),
                "a leftover download was not removed"
            );
        }
    }
}
