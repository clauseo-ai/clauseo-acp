//! Node: the user's when it qualifies, else the pinned download unpacked under
//! `<data_dir>/agents/runtime/`.
//!
//! The user's Node is searched in each folder on `PATH`, then `%ProgramFiles%\nodejs` on
//! Windows. A candidate is resolved through its links (`dunce::canonicalize`) and is used only
//! if the result is a regular file with npm's `npm-cli.js` beside it and `node --version`
//! (through the launcher, 10 s at most) reports v22 or later.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launched, Launcher, Timer};
use futures::AsyncReadExt as _;
use futures::future::{Either, join3, select};
use serde::{Deserialize, Serialize};

use crate::config::NodeSearch;
use crate::platform::{Os, Platform};
use crate::snapshot::{NodeArchive, NodeEntry};
use crate::write::{self, ArchiveKind, ExtractError};

/// The oldest Node major the adapters run on.
pub(crate) const MIN_MAJOR: u32 = 22;
/// How long `node --version` may take.
pub(crate) const VERSION_LIMIT: Duration = Duration::from_secs(10);
/// The most stdout bytes kept from `node --version`; the rest is read and dropped.
const VERSION_MAX: usize = 1024;
/// The receipt a complete runtime folder holds.
pub(crate) const RUNTIME_RECEIPT: &str = "clauseo-runtime.json";

/// A Node that qualifies: a regular file, v22 or later, npm beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FoundNode {
    /// The program, links resolved.
    pub(crate) program: PathBuf,
    /// npm's `npm-cli.js` beside it.
    pub(crate) npm_cli: PathBuf,
    /// What `node --version` printed, trimmed (`v24.22.0`).
    pub(crate) version: String,
}

/// Why a candidate was not used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NotUsed {
    /// It does not resolve (missing, or a broken link).
    Missing,
    /// It resolves to something that is not a regular file.
    NotAFile,
    /// No `npm-cli.js` beside it.
    NoNpm,
    /// The launcher refused or failed.
    LaunchFailed(String),
    /// `node --version` took longer than [`VERSION_LIMIT`].
    TimedOut,
    /// It exited with this code, or printed no version.
    BadAnswer(Option<i32>),
    /// Its version is below [`MIN_MAJOR`].
    TooOld(String),
}

/// The candidates [`search_user_node`] tries, in order: `node` (`node.exe` on Windows) in each
/// absolute folder of `search.path`, then `<program_files>\nodejs\node.exe` on Windows.
#[expect(
    clippy::disallowed_methods,
    reason = "agents splits the PATH it is given here alone, to find the user's Node"
)]
pub(crate) fn candidates(search: &NodeSearch) -> Vec<PathBuf> {
    let name = if cfg!(windows) { "node.exe" } else { "node" };
    let mut found: Vec<PathBuf> = search
        .path
        .as_deref()
        .map(|path| {
            std::env::split_paths(path)
                .filter(|dir| dir.is_absolute())
                .map(|dir| dir.join(name))
                .collect()
        })
        .unwrap_or_default();
    if cfg!(windows)
        && let Some(program_files) = &search.program_files
    {
        found.push(program_files.join("nodejs").join(name));
    }
    found
}

/// What a search for the user's Node saw.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct UserSearch {
    /// The first candidate that qualifies.
    pub(crate) found: Option<FoundNode>,
    /// Candidates before it (links resolved) whose `--version` took too long: not proven bad.
    pub(crate) timed_out: Vec<PathBuf>,
}

/// The first candidate of `search` that qualifies, and the ones before it that timed out.
pub(crate) async fn search_user_node<T: Timer>(
    search: &NodeSearch,
    launcher: &dyn Launcher,
    timer: &T,
) -> UserSearch {
    let mut seen = UserSearch::default();
    for candidate in candidates(search) {
        match check_node(&candidate, launcher, timer).await {
            Ok(found) => {
                seen.found = Some(found);
                break;
            }
            Err(NotUsed::TimedOut) => {
                if let Ok(program) = dunce::canonicalize(&candidate) {
                    seen.timed_out.push(program);
                }
            }
            Err(_) => {}
        }
    }
    seen
}

/// Checks one Node: the user's candidate, or a receipt's Node at start.
pub(crate) async fn check_node<T: Timer>(
    candidate: &Path,
    launcher: &dyn Launcher,
    timer: &T,
) -> Result<FoundNode, NotUsed> {
    let program = dunce::canonicalize(candidate).map_err(|_missing| NotUsed::Missing)?;
    if !std::fs::metadata(&program).is_ok_and(|meta| meta.is_file()) {
        return Err(NotUsed::NotAFile);
    }
    let os = if cfg!(windows) { Os::Win32 } else { Os::Linux };
    let npm_cli = npm_cli_beside(&program, os).ok_or(NotUsed::NoNpm)?;
    if !npm_cli.is_file() {
        return Err(NotUsed::NoNpm);
    }
    let version = node_version(&program, launcher, timer).await?;
    match parse_version(&version) {
        Some((major, _, _)) if major >= MIN_MAJOR => Ok(FoundNode {
            program,
            npm_cli,
            version,
        }),
        Some(_) => Err(NotUsed::TooOld(version)),
        None => Err(NotUsed::BadAnswer(Some(0))),
    }
}

/// Runs `program --version` with `NODE_OPTIONS` empty and returns its first line, trimmed.
/// stdout is read to its end while the stderr pump drains and the exit is awaited, all under
/// [`VERSION_LIMIT`]; past it the handle is dropped, which kills the tree.
async fn node_version<T: Timer>(
    program: &Path,
    launcher: &dyn Launcher,
    timer: &T,
) -> Result<String, NotUsed> {
    let cwd = program.parent().map(Path::to_path_buf).unwrap_or_default();
    let env = BTreeMap::from([(OsString::from("NODE_OPTIONS"), OsString::new())]);
    let spec = LaunchSpec::new(program.to_owned(), vec!["--version".into()], env, cwd)
        .map_err(|err| NotUsed::LaunchFailed(err.to_string()))?;
    let Launched {
        stdin,
        mut stdout,
        stderr,
        mut handle,
        ..
    } = launcher
        .launch(&spec)
        .map_err(|err| NotUsed::LaunchFailed(err.to_string()))?;
    drop(stdin);
    let ran = {
        let work = pin!(async {
            let read = async {
                let mut kept = Vec::new();
                let mut chunk = [0_u8; 4096];
                while let Ok(n @ 1..) = stdout.read(&mut chunk).await {
                    let room = VERSION_MAX.saturating_sub(kept.len());
                    kept.extend_from_slice(chunk.get(..n.min(room)).unwrap_or_default());
                }
                kept
            };
            let (kept, drained, report) = join3(read, stderr.drain(), handle.exited()).await;
            drop(drained);
            (kept, report.code())
        });
        match select(work, pin!(timer.after(VERSION_LIMIT))).await {
            Either::Left((ran, _)) => Some(ran),
            Either::Right(((), _)) => None,
        }
    };
    let Some((kept, code)) = ran else {
        drop(handle);
        return Err(NotUsed::TimedOut);
    };
    let text = String::from_utf8_lossy(&kept);
    let line = text.lines().next().unwrap_or_default().trim().to_owned();
    if code != Some(0) || line.is_empty() {
        return Err(NotUsed::BadAnswer(code));
    }
    Ok(line)
}

/// `vMAJOR.MINOR.PATCH` (the `v` optional, anything after the patch number ignored) as
/// numbers.
pub(crate) fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let text = text.trim();
    let text = text.strip_prefix('v').unwrap_or(text);
    let mut parts = text.splitn(3, '.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let rest = parts.next()?;
    let digits = rest
        .find(|ch: char| !ch.is_ascii_digit())
        .map_or(rest, |end| &rest[..end]);
    Some((major, minor, digits.parse().ok()?))
}

/// npm's `npm-cli.js` beside the Node `program` as the `os` lays it out:
/// `../lib/node_modules/npm/bin/npm-cli.js` on Unix, `node_modules\npm\bin\npm-cli.js` on
/// Windows.
fn npm_cli_beside(program: &Path, os: Os) -> Option<PathBuf> {
    let dir = program.parent()?;
    let base = match os {
        Os::Win32 => dir.to_path_buf(),
        Os::Darwin | Os::Linux => dir.parent()?.join("lib"),
    };
    Some(
        base.join("node_modules")
            .join("npm")
            .join("bin")
            .join("npm-cli.js"),
    )
}

/// The downloaded Node's folder: `<data_dir>/agents/runtime/node-v<version>-<os>-<arch>`.
pub(crate) fn runtime_dir(data_dir: &Path, entry: &NodeEntry, platform: Platform) -> PathBuf {
    data_dir.join("agents").join("runtime").join(format!(
        "node-v{}-{}",
        entry.version(),
        platform.node_name()
    ))
}

/// The program inside a runtime folder: `node.exe` on Windows, `bin/node` elsewhere.
pub(crate) fn runtime_program(dir: &Path, os: Os) -> PathBuf {
    match os {
        Os::Win32 => dir.join("node.exe"),
        Os::Darwin | Os::Linux => dir.join("bin").join("node"),
    }
}

/// npm's `npm-cli.js` inside a runtime folder.
pub(crate) fn runtime_npm_cli(dir: &Path, os: Os) -> PathBuf {
    npm_cli_beside(&runtime_program(dir, os), os).unwrap_or_default()
}

/// What `clauseo-runtime.json` holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RuntimeReceipt {
    /// Always 1.
    schema: u32,
    /// Node's version, without a `v`.
    version: String,
    /// The archive's SHA-256, lowercase hex.
    sha256: String,
}

/// Whether `dir` is a complete runtime for `archive`: its receipt names the archive's SHA-256,
/// and the program and `npm-cli.js` are regular files.
pub(crate) fn runtime_valid(dir: &Path, archive: &NodeArchive, os: Os) -> bool {
    let Ok(bytes) = std::fs::read(dir.join(RUNTIME_RECEIPT)) else {
        return false;
    };
    let Ok(receipt) = serde_json::from_slice::<RuntimeReceipt>(&bytes) else {
        return false;
    };
    let is_file = |path: &Path| std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file());
    receipt.schema == 1
        && receipt.sha256 == archive.sha256_hex()
        && is_file(&runtime_program(dir, os))
        && is_file(&runtime_npm_cli(dir, os))
}

/// Unpacks the archive at `archive_path` into `dir`. The archive is read once into memory (at
/// most its snapshot size, about 60 MB) and its SHA-256 checked on those bytes, which are the
/// ones unpacked: nothing is opened again by path after the check. A mismatch deletes the
/// archive. The bytes go into `.tmp-<folder name>` beside `dir` first (any old one removed),
/// then `node` and `npm-cli.js` are synced, the folder renamed into place over any incomplete
/// one, and `clauseo-runtime.json` written last.
pub(crate) fn install_runtime(
    archive_path: &Path,
    entry: &NodeEntry,
    archive: &NodeArchive,
    dir: &Path,
    os: Os,
) -> Result<(), ExtractError> {
    let (Some(parent), Some(name)) = (dir.parent(), dir.file_name()) else {
        return Err(ExtractError::Malformed(
            "the runtime folder has no parent".into(),
        ));
    };
    let bytes = read_checked(archive_path, archive)?;
    let mut tmp_name = OsString::from(".tmp-");
    tmp_name.push(name);
    let tmp = parent.join(tmp_name);
    write::remove_dir_all(&tmp)?;
    let kind = if archive.is_zip() {
        ArchiveKind::Zip
    } else {
        ArchiveKind::TarGz
    };
    let unpacked = write::extract(&bytes, kind, &tmp).and_then(|()| {
        let complete = runtime_program(&tmp, os).is_file() && runtime_npm_cli(&tmp, os).is_file();
        if complete {
            // After a power cut a receipt must not sit beside a truncated `node` or npm. (The
            // rest of npm's files are not synced: an npm that then fails to load its modules
            // has the runtime removed and fetched again.)
            write::sync_file(&runtime_program(&tmp, os))
                .and_then(|()| write::sync_file(&runtime_npm_cli(&tmp, os)))
                .map_err(ExtractError::Io)
        } else {
            Err(ExtractError::Malformed(
                "the archive holds no node and npm".into(),
            ))
        }
    });
    if let Err(err) = unpacked {
        drop(write::remove_dir_all(&tmp));
        return Err(err);
    }
    write::remove_dir_all(dir)?;
    write::rename(&tmp, dir)?;
    let receipt = RuntimeReceipt {
        schema: 1,
        version: entry.version().to_owned(),
        sha256: archive.sha256_hex(),
    };
    let bytes = serde_json::to_vec_pretty(&receipt)
        .map_err(|err| ExtractError::Malformed(err.to_string()))?;
    write::write_atomic(&dir.join(RUNTIME_RECEIPT), &bytes)?;
    Ok(())
}

/// The bytes of the archive at `path`, once they are exactly `archive`'s size and SHA-256. A
/// file that is not is deleted, so the next try fetches it again.
fn read_checked(path: &Path, archive: &NodeArchive) -> Result<Vec<u8>, ExtractError> {
    use std::io::Read as _;
    let size = archive.size();
    let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
    write::open_read(path)?
        .take(size.saturating_add(1))
        .read_to_end(&mut bytes)?;
    let mut hasher = archive.expected().hasher();
    hasher.update(&bytes);
    if u64::try_from(bytes.len()).ok() == Some(size) && hasher.matches(archive.expected()) {
        return Ok(bytes);
    }
    drop(write::remove_file(path));
    Err(ExtractError::Malformed(
        "Node's archive no longer matches its pinned checksum; it was deleted".into(),
    ))
}

#[cfg(test)]
mod tests;
