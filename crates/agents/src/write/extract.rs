//! Unpacking a runtime archive (Node's `.tar.gz` or `.zip`, Python's `.tar.gz`) into a folder:
//! the archive's top folder stripped, no link made, and no entry written outside the folder.
//! Python's is unpacked from a reader (streamed from the disk), read-only on Unix, with a
//! manifest of every file written.

#![expect(
    clippy::disallowed_methods,
    reason = "agents unpacks the runtimes' archives here alone; clippy.toml bans std's writes \
              elsewhere"
)]

use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use sha2::Digest as _;

/// Which format an archive is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArchiveKind {
    /// A gzipped tar: Node for macOS and Linux.
    TarGz,
    /// A zip: Node for Windows.
    Zip,
}

/// Why an archive did not unpack.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ExtractError {
    /// Reading the archive or writing a file failed.
    #[error("unpacking failed: {0}")]
    Io(#[from] io::Error),
    /// The archive does not read as its format.
    #[error("the archive does not read: {0}")]
    Malformed(String),
    /// An entry names an absolute path, a `..` part or a drive: it would land outside.
    #[error("the archive entry {0:?} would land outside its folder")]
    Escapes(String),
}

/// Unpacks `archive` (the archive's bytes, already checked) into `dest` (made if missing),
/// dropping each entry's first path part (the archive's top folder). Symbolic and hard links
/// are skipped, as is any entry that is not a file or a folder. An absolute, drive or `..` entry
/// fails the whole unpack. On Unix a file with any exec bit gets `0o755`, others `0o644`.
pub(crate) fn extract(archive: &[u8], kind: ArchiveKind, dest: &Path) -> Result<(), ExtractError> {
    std::fs::create_dir_all(dest)?;
    match kind {
        ArchiveKind::TarGz => extract_tar_gz(archive, dest),
        ArchiveKind::Zip => extract_zip(archive, dest),
    }
}

fn extract_tar_gz(archive: &[u8], dest: &Path) -> Result<(), ExtractError> {
    let mut tar = tar::Archive::new(GzDecoder::new(archive));
    for entry in tar.entries()? {
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        let name = String::from_utf8(entry.path_bytes().into_owned())
            .map_err(|err| ExtractError::Malformed(format!("an entry's name: {err}")))?;
        let Some(rel) = inside(&name)? else {
            continue;
        };
        if kind.is_dir() {
            std::fs::create_dir_all(dest.join(rel))?;
        } else if kind.is_file() {
            let exec = entry.header().mode().is_ok_and(|mode| mode & 0o111 != 0);
            write_file(&dest.join(rel), &mut entry, exec)?;
        }
        // Links, devices, FIFOs and extension headers are skipped.
    }
    Ok(())
}

/// One file a read-only unpack wrote: its path (`/`-separated, under the destination), size and
/// SHA-256 (lowercase hex).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManifestEntry {
    pub(crate) path: String,
    pub(crate) size: u64,
    pub(crate) sha256: String,
}

/// Unpacks the gzipped tar read from `reader` into `dest` (made if missing) by
/// [`extract`]'s rules, hashing each file as it is written, and gives every file written,
/// sorted by path. On Unix each file is `0o555` when its entry has any exec bit, else `0o444`,
/// and every folder below `dest` is `0o555` once all is written; `dest` itself stays writable
/// so its receipt can go in ([`super::make_read_only`] seals it after). Node's unpack keeps its
/// writable modes.
pub(crate) fn extract_tar_gz_read_only(
    reader: impl io::Read,
    dest: &Path,
) -> Result<Vec<ManifestEntry>, ExtractError> {
    std::fs::create_dir_all(dest)?;
    let mut tar = tar::Archive::new(GzDecoder::new(reader));
    let mut files = BTreeMap::new();
    for entry in tar.entries()? {
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        let name = String::from_utf8(entry.path_bytes().into_owned())
            .map_err(|err| ExtractError::Malformed(format!("an entry's name: {err}")))?;
        let Some(rel) = inside(&name)? else {
            continue;
        };
        if kind.is_dir() {
            std::fs::create_dir_all(dest.join(&rel))?;
        } else if kind.is_file() {
            let exec = entry.header().mode().is_ok_and(|mode| mode & 0o111 != 0);
            let (size, sha256) = write_file_hashed(&dest.join(&rel), &mut entry, exec)?;
            let parts: Vec<String> = rel
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect();
            files.insert(parts.join("/"), (size, sha256));
        }
    }
    super::seal_below(dest)?;
    Ok(files
        .into_iter()
        .map(|(path, (size, sha256))| ManifestEntry { path, size, sha256 })
        .collect())
}

/// [`write_file`] for a read-only unpack: the file's mode is `0o555` or `0o444` (Unix), and
/// its size and SHA-256 come back.
fn write_file_hashed(
    path: &Path,
    from: &mut impl io::Read,
    exec: bool,
) -> io::Result<(u64, String)> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    let mut hashing = Hashing {
        inner: from,
        hasher: sha2::Sha256::new(),
    };
    let size = io::copy(&mut hashing, &mut file)?;
    // Windows has no mode: the sandbox's ACLs keep the runtime read-only for the agent.
    #[cfg(unix)]
    set_read_only(&file, exec)?;
    #[cfg(not(unix))]
    let _ = exec;
    let digest = hashing.hasher.finalize();
    Ok((size, crate::integrity::to_hex(&digest)))
}

/// A reader that hashes what passes through it.
struct Hashing<'a, R> {
    inner: &'a mut R,
    hasher: sha2::Sha256,
}

impl<R: io::Read> io::Read for Hashing<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.hasher.update(buf.get(..n).unwrap_or_default());
        Ok(n)
    }
}

/// Sets `file` to `0o555` when `exec`, else `0o444`.
#[cfg(unix)]
fn set_read_only(file: &File, exec: bool) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = if exec { 0o555 } else { 0o444 };
    file.set_permissions(std::fs::Permissions::from_mode(mode))
}

fn extract_zip(archive: &[u8], dest: &Path) -> Result<(), ExtractError> {
    let malformed = |err: zip::result::ZipError| ExtractError::Malformed(err.to_string());
    let mut zip = zip::ZipArchive::new(io::Cursor::new(archive)).map_err(malformed)?;
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).map_err(malformed)?;
        let name = file.name().map_err(malformed)?.into_owned();
        let Some(rel) = inside(&name)? else {
            continue;
        };
        if file.is_symlink() {
            continue;
        }
        if file.is_dir() {
            std::fs::create_dir_all(dest.join(rel))?;
        } else {
            let exec = file.unix_mode().is_some_and(|mode| mode & 0o111 != 0);
            write_file(&dest.join(rel), &mut file, exec)?;
        }
    }
    Ok(())
}

/// Writes `from` to a new file at `path`, making its folders. Nothing under the destination is
/// a link (none is ever made), so the write cannot be led outside it.
fn write_file(path: &Path, from: &mut impl io::Read, exec: bool) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    io::copy(from, &mut file)?;
    set_exec(&file, exec)
}

#[cfg(unix)]
fn set_exec(file: &File, exec: bool) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = if exec { 0o755 } else { 0o644 };
    file.set_permissions(std::fs::Permissions::from_mode(mode))
}

/// Windows has no exec bit: a file's extension decides what runs.
#[cfg(not(unix))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the Unix version's signature, which can fail; one call site serves both"
)]
fn set_exec(_file: &File, _exec: bool) -> io::Result<()> {
    Ok(())
}

/// The path an entry named `name` lands at, relative to the destination, with its top folder
/// dropped: `None` for the top folder itself. Parts split on `/` and `\`; empty and `.` parts
/// are dropped. An absolute name, a `..` part or a part holding `:` (a drive or a stream) is
/// [`ExtractError::Escapes`].
pub(crate) fn inside(name: &str) -> Result<Option<PathBuf>, ExtractError> {
    let escapes = || ExtractError::Escapes(name.to_owned());
    if name.starts_with(['/', '\\']) {
        return Err(escapes());
    }
    let mut parts = Vec::new();
    for part in name.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return Err(escapes()),
            _ if part.contains(':') || part.contains('\0') => return Err(escapes()),
            _ => parts.push(part),
        }
    }
    let rel: PathBuf = parts.iter().skip(1).collect();
    Ok((parts.len() > 1).then_some(rel))
}
