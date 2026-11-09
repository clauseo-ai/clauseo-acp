//! The pinned Python runtime: Clauseo's archive for this platform, unpacked under
//! `<data_dir>/agents/runtime/<archive stem>/` beside Node, read-only on Unix, with a manifest
//! of every file and a receipt written last.
//!
//! The archive is streamed from the disk twice through one open handle: once to check its size
//! and SHA-256, then (from the start again) to unpack, so it is never held in memory and is not
//! opened again by path after the check. The runtime is run once (`python -I -c "import …"`)
//! before its receipt goes in, so a computer whose policy refuses to start it says so at install
//! rather than at the agent's first use.

mod run;

use std::ffi::OsString;
use std::io::{self, Read as _, Seek as _};
use std::path::{Path, PathBuf};

use clauseo_process::{Launcher, Timer};
use serde::{Deserialize, Serialize};

use crate::cancel::Cancel;
use crate::config::AgentsConfig;
use crate::platform::Os;
use crate::snapshot::{PythonArchive, PythonEntry};
use crate::state::PythonState;
use crate::write::{self, ExtractError, ManifestEntry};

pub(crate) use run::RunFailure;

/// The receipt a complete Python runtime folder holds.
pub(crate) const PYTHON_RECEIPT: &str = "clauseo-python.json";
/// The manifest beside it: one line per file, `<sha256> <size> <path>`.
pub(crate) const PYTHON_MANIFEST: &str = "clauseo-python-manifest.txt";
/// The prefix of every Python archive's name, and so of its runtime folder.
const FOLDER_PREFIX: &str = "clauseo-python-";

/// `<data_dir>/agents/runtime`.
fn runtime_root(data_dir: &Path) -> PathBuf {
    data_dir.join("agents").join("runtime")
}

/// The Python runtime's folder: `<data_dir>/agents/runtime/<archive stem>`.
pub(crate) fn runtime_dir(data_dir: &Path, archive: &PythonArchive) -> PathBuf {
    runtime_root(data_dir).join(archive.stem())
}

/// The interpreter inside a runtime folder: `python.exe` on Windows, `bin/python3` elsewhere.
pub(crate) fn runtime_program(dir: &Path, os: Os) -> PathBuf {
    match os {
        Os::Win32 => dir.join("python.exe"),
        Os::Darwin | Os::Linux => dir.join("bin").join("python3"),
    }
}

/// The folder of commands (`python`, `python3`, `pip`, and `py` on Windows) inside a runtime
/// folder: `bin` on macOS, `clauseo-bin` on Windows (the runtime's root holds its DLLs, which
/// must stay off other programs' search path).
pub(crate) fn runtime_command_dir(dir: &Path, os: Os) -> PathBuf {
    match os {
        Os::Win32 => dir.join("clauseo-bin"),
        Os::Darwin | Os::Linux => dir.join("bin"),
    }
}

/// The installed Python runtime's command folder, for the agent's `PATH` (first, before the
/// system's): `Some` only when this platform has Python and its runtime is unpacked, complete
/// and its receipt matches this build's archive. `None` otherwise: the caller then puts its stub
/// folder there instead, which explains that Python is not set up yet.
pub fn python_command_dir(config: &AgentsConfig) -> Option<PathBuf> {
    let archive = config.snapshot.python_archive(config.platform)?;
    let dir = runtime_dir(&config.data_dir, archive);
    python_runtime_valid(&dir, archive, config.platform.os)
        .then(|| runtime_command_dir(&dir, config.platform.os))
}

/// Whether this platform has Python and its runtime is not installed: an install fetches it.
pub(crate) fn needed(config: &AgentsConfig) -> bool {
    config
        .snapshot
        .python_archive(config.platform)
        .is_some_and(|archive| {
            let dir = runtime_dir(&config.data_dir, archive);
            !python_runtime_valid(&dir, archive, config.platform.os)
        })
}

/// Python's state with no job running: `Unavailable` with no archive for this platform,
/// `Ready` once installed, else `Missing` with its archive's size.
pub(crate) fn resting_state(config: &AgentsConfig) -> PythonState {
    let Some(archive) = config.snapshot.python_archive(config.platform) else {
        return PythonState::Unavailable;
    };
    let dir = runtime_dir(&config.data_dir, archive);
    if python_runtime_valid(&dir, archive, config.platform.os) {
        PythonState::Ready {
            version: config.snapshot.python().version().to_owned(),
        }
    } else {
        PythonState::Missing {
            download_bytes: archive.size(),
        }
    }
}

/// What `clauseo-python.json` holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PythonReceipt {
    /// Always 1.
    schema: u32,
    /// Python's version (`3.13.18`).
    version: String,
    /// The archive's file name: which build of the runtime this is.
    archive: String,
    /// The archive's SHA-256, lowercase hex.
    sha256: String,
    /// The SHA-256 of [`PYTHON_MANIFEST`]'s bytes, lowercase hex.
    manifest_sha256: String,
}

fn read_receipt(dir: &Path) -> Option<PythonReceipt> {
    let text = write::read_small(&dir.join(PYTHON_RECEIPT), 4096).ok()??;
    serde_json::from_str(&text).ok()
}

/// Whether `dir` is a complete runtime for `archive`: its receipt is schema 1 and names the
/// archive and its SHA-256, the manifest beside it hashes to the receipt's `manifest_sha256`,
/// and the interpreter is a regular file. Each file's own hash is [`contents_match`]'s.
pub(crate) fn python_runtime_valid(dir: &Path, archive: &PythonArchive, os: Os) -> bool {
    let Some(receipt) = read_receipt(dir) else {
        return false;
    };
    let is_file = |path: &Path| std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file());
    receipt.schema == 1
        && receipt.archive == archive.name()
        && receipt.sha256 == archive.sha256_hex()
        && manifest_hash(dir).is_some_and(|hash| hash == receipt.manifest_sha256)
        && is_file(&runtime_program(dir, os))
}

/// The SHA-256 of `dir`'s manifest, when it reads.
fn manifest_hash(dir: &Path) -> Option<String> {
    sha256_file(&dir.join(PYTHON_MANIFEST)).map(|(_, hex)| hex)
}

/// The size and SHA-256 (lowercase hex) of the file at `path`, read through no link.
fn sha256_file(path: &Path) -> Option<(u64, String)> {
    use sha2::Digest as _;
    let mut file = write::open_read(path).ok()?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0_u8; 64 * 1024];
    let mut size = 0_u64;
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                hasher.update(buf.get(..n).unwrap_or_default());
                size = size.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    Some((size, crate::integrity::to_hex(&hasher.finalize())))
}

/// The SHA-256 of `bytes`, lowercase hex.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    crate::integrity::to_hex(&sha2::Sha256::digest(bytes))
}

/// The manifest's text for `files`: `<sha256> <size> <path>` per line, sorted by path.
fn manifest_text(files: &[ManifestEntry]) -> String {
    files
        .iter()
        .map(|file| {
            [
                file.sha256.clone(),
                file.size.to_string(),
                file.path.clone(),
            ]
            .join(" ")
                + "\n"
        })
        .collect()
}

/// Whether the installed Python runtime's files are all as its manifest lists them, every byte
/// read and hashed: the repair check. `false` when this platform has no Python, or its runtime
/// is not installed or its receipt does not match.
pub fn python_contents_match(config: &AgentsConfig) -> bool {
    let Some(archive) = config.snapshot.python_archive(config.platform) else {
        return false;
    };
    let dir = runtime_dir(&config.data_dir, archive);
    python_runtime_valid(&dir, archive, config.platform.os) && contents_match(&dir)
}

/// Whether every file the manifest of the valid runtime `dir` lists is there with its size and
/// SHA-256: the repair check, reading every byte of the runtime (about 250 MB). Files the
/// manifest does not list (the receipt, the manifest) are not looked at.
pub(crate) fn contents_match(dir: &Path) -> bool {
    let Ok(Some(text)) = write::read_small(&dir.join(PYTHON_MANIFEST), 64 * 1024 * 1024) else {
        return false;
    };
    text.lines().all(|line| {
        let mut parts = line.splitn(3, ' ');
        let (Some(sha), Some(size), Some(rel)) = (parts.next(), parts.next(), parts.next()) else {
            return false;
        };
        let path = rel
            .split('/')
            .fold(dir.to_path_buf(), |path, part| path.join(part));
        sha256_file(&path).is_some_and(|(n, hex)| size.parse::<u64>().ok() == Some(n) && hex == sha)
    })
}

/// Why an install of the runtime did not finish.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PythonError {
    /// The archive did not check or unpack, or a file did not write.
    #[error(transparent)]
    Extract(#[from] ExtractError),
    /// The unpacked runtime would not run.
    #[error("{0}")]
    Run(RunFailure),
}

impl From<io::Error> for PythonError {
    fn from(err: io::Error) -> PythonError {
        PythonError::Extract(ExtractError::Io(err))
    }
}

/// Installs the runtime from the archive at `archive_path`: checks its size and SHA-256 by
/// streaming it (a mismatch deletes it), unpacks it read-only through the same open handle into
/// `.tmp-<folder name>` beside `dir` (any old one removed), syncs the interpreter, renames the
/// folder into place over any incomplete one, runs it once ([`run::smoke`]), then writes the
/// manifest and the receipt last and seals `dir` read-only. A failed unpack or run removes the
/// unpacked tree (as Node's failed unpack does); the archive stays in `downloads/`, so a retry
/// unpacks it again without fetching it.
pub(crate) async fn install_python_runtime<T: Timer>(
    archive_path: &Path,
    entry: &PythonEntry,
    archive: &PythonArchive,
    dir: &Path,
    os: Os,
    launcher: &dyn Launcher,
    timer: &T,
    cancel: &Cancel,
) -> Result<(), PythonError> {
    let (Some(parent), Some(name)) = (dir.parent(), dir.file_name()) else {
        return Err(ExtractError::Malformed("the runtime folder has no parent".into()).into());
    };
    let file = open_checked(archive_path, archive)?;
    let mut tmp_name = OsString::from(".tmp-");
    tmp_name.push(name);
    let tmp = parent.join(tmp_name);
    write::remove_dir_all(&tmp)?;
    let reader = io::BufReader::new(file.take(archive.size()));
    let unpacked = write::extract_tar_gz_read_only(reader, &tmp).and_then(|files| {
        let program = runtime_program(&tmp, os);
        if program.is_file() {
            // After a power cut a receipt must not sit beside a truncated interpreter.
            write::sync_read_only(&program).map_err(ExtractError::Io)?;
            Ok(files)
        } else {
            Err(ExtractError::Malformed(
                "the archive holds no Python interpreter".into(),
            ))
        }
    });
    let files = match unpacked {
        Ok(files) => files,
        Err(err) => {
            drop(write::remove_dir_all(&tmp));
            return Err(err.into());
        }
    };
    write::remove_dir_all(dir)?;
    write::rename(&tmp, dir)?;
    if let Err(failure) = run::smoke(&runtime_program(dir, os), launcher, timer, cancel).await {
        drop(write::remove_dir_all(dir));
        return Err(PythonError::Run(failure));
    }
    let manifest = manifest_text(&files);
    write::write_atomic(&dir.join(PYTHON_MANIFEST), manifest.as_bytes())?;
    let receipt = PythonReceipt {
        schema: 1,
        version: entry.version().to_owned(),
        archive: archive.name().to_owned(),
        sha256: archive.sha256_hex(),
        manifest_sha256: sha256_hex(manifest.as_bytes()),
    };
    let bytes = serde_json::to_vec_pretty(&receipt)
        .map_err(|err| ExtractError::Malformed(err.to_string()))?;
    write::write_atomic(&dir.join(PYTHON_RECEIPT), &bytes)?;
    #[cfg(unix)]
    write::make_read_only(dir)?;
    Ok(())
}

/// The archive at `path`, open, read to its end once and rewound, when it is exactly
/// `archive`'s size and SHA-256. A file that is not is deleted, so the next try
/// fetches it again.
fn open_checked(path: &Path, archive: &PythonArchive) -> Result<std::fs::File, ExtractError> {
    let mut file = write::open_read(path)?;
    let mut hasher = archive.expected().hasher();
    let read = crate::download::hash_read((&mut file).take(archive.size() + 1), &mut hasher)?;
    file.rewind()?;
    if read == archive.size() && hasher.matches(archive.expected()) {
        return Ok(file);
    }
    drop(file);
    drop(write::remove_file(path));
    Err(ExtractError::Malformed(
        "Python's archive no longer matches its pinned checksum; it was deleted".into(),
    ))
}

/// Removes every Python runtime folder and temp folder under `<data_dir>/agents/runtime` but
/// `keep` (the current archive's stem, when this platform has Python): an app update's new
/// archive leaves the old runtime behind. Read-only trees are made writable first
/// ([`write::remove_dir_all`]). Only a real `agents/runtime` folder is listed, never through a
/// link; Node's folders are left alone.
pub(crate) fn remove_stale_runtimes(data_dir: &Path, keep: Option<&str>) -> io::Result<()> {
    let root = runtime_root(data_dir);
    if !write::real_dirs_under(data_dir, &root)? {
        return Ok(());
    }
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let folder = name.strip_prefix(".tmp-").unwrap_or(&name);
        let current = keep.is_some_and(|keep| name == keep);
        if folder.starts_with(FOLDER_PREFIX) && !current {
            write::remove_any(&entry.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
