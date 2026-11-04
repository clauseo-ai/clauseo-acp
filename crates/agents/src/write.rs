//! The std calls that change files: the one place in `agents` that may call them. The workspace
//! `clippy.toml` bans each of them everywhere else, and the expect below covers only this file.
//!
//! Everything written is under `<data_dir>/agents`: downloads, the npm cache's folder, the
//! runtime, install folders and receipts. Callers pass paths they built from the data folder.
//! Sign-in's folders are made with [`create_private_dirs_under`], which follows no link below
//! the data folder.

#![expect(
    clippy::disallowed_methods,
    reason = "agents writes files here alone; clippy.toml bans std's writes elsewhere"
)]

use std::fs::{File, OpenOptions};
use std::io::{self, Write as _};
use std::path::Path;

pub(crate) mod extract;

pub(crate) use extract::{ArchiveKind, ExtractError, extract};

/// `fs::create_dir_all`.
pub(crate) fn create_dir_all(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Creates the folder `target` and every missing folder between `data_dir` and it, through no
/// link, following host's `make_home` (decision 0025): each part below `data_dir` that exists
/// must be a real folder, never a symbolic link, junction or other reparse point (read with
/// `symlink_metadata`, which never follows one), and each missing one is made, mode 0700 on
/// Unix whatever the umask. Another thread or process making one first is fine; the check after
/// still refuses a link or a file there. A link or a file below `data_dir`, or a `target` not
/// under it, fails with `InvalidInput`, and nothing is made where a link points. `data_dir`
/// itself may be a link: `CLAUSEO_DATA_DIR` may name one. A folder already there keeps its
/// mode. Every sign-in folder is made with this: the agent's home, `agents/<id>` for the
/// markers, `agents/signin` and its `detect` folder.
pub(crate) fn create_private_dirs_under(data_dir: &Path, target: &Path) -> io::Result<()> {
    let below = target.strip_prefix(data_dir).map_err(|_outside| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "the folder is not under the data folder",
        )
    })?;
    let mut at = data_dir.to_owned();
    for part in below.components() {
        if !matches!(part, std::path::Component::Normal(_)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the folder's path leaves the data folder",
            ));
        }
        at.push(part);
        match std::fs::symlink_metadata(&at) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => match create_private_dir(&at) {
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
                made => made?,
            },
            Err(err) => return Err(err),
            Ok(_) => {}
        }
        let meta = std::fs::symlink_metadata(&at)?;
        if !meta.is_dir() || is_reparse_point(&meta) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a link or a file stands where a folder goes",
            ));
        }
    }
    Ok(())
}

/// Makes the one folder `path` (its parent must exist), 0700 on Unix (a umask only narrows it).
fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(path)
}

/// Whether `meta` (from `symlink_metadata`) is a symbolic link or, on Windows, any reparse point
/// (a junction, a symbolic link, or another kind).
pub(crate) fn is_reparse_point(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        /// `FILE_ATTRIBUTE_REPARSE_POINT`.
        const REPARSE_POINT: u32 = 0x400;
        if meta.file_attributes() & REPARSE_POINT != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

/// Opens `path` to append, making it if missing: a partial download resumed. A link at `path`
/// is refused, never followed.
pub(crate) fn open_append(path: &Path) -> io::Result<File> {
    open_no_follow(OpenOptions::new().create(true).append(true), path)
}

/// Opens `path` empty, making it if missing and truncating it if not: a download started over.
/// A link at `path` is refused, never followed.
pub(crate) fn open_truncate(path: &Path) -> io::Result<File> {
    open_no_follow(
        OpenOptions::new().create(true).write(true).truncate(true),
        path,
    )
}

/// Opens `path` to read, refusing a link: a download or archive this crate wrote, about to be
/// hashed or unpacked. (A read changes nothing; it lives here beside the writes' link rule.)
pub(crate) fn open_read(path: &Path) -> io::Result<File> {
    open_no_follow(OpenOptions::new().read(true), path)
}

/// Opens `path` with `options`, never through a link: `O_NOFOLLOW` on Unix (the open itself
/// fails on a link, with no gap), and on Windows a link found by `symlink_metadata` is refused.
fn open_no_follow(options: &mut OpenOptions, path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(nix::libc::O_NOFOLLOW);
    }
    #[cfg(not(unix))]
    if std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a link stands where a file goes",
        ));
    }
    options.open(path)
}

/// Takes the exclusive lock on `<data_dir>/agents/.lock` (made if missing) without waiting:
/// `Ok(None)` when another process holds it, i.e. another Clauseo is installing. The lock lasts
/// as long as the file it gives.
pub(crate) fn try_lock_agents(data_dir: &Path) -> io::Result<Option<File>> {
    let agents = data_dir.join("agents");
    create_dir_all(&agents)?;
    let file = open_no_follow(
        OpenOptions::new().create(true).write(true).truncate(false),
        &agents.join(".lock"),
    )?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(err)) => Err(err),
    }
}

/// Reads the small text file at `path`, refusing a link and anything past `limit` bytes:
/// Clauseo's own sign-in markers (`signin/marker.rs`), never a credential. `Ok(None)` when
/// there is no file.
pub(crate) fn read_small(path: &Path, limit: u64) -> io::Result<Option<String>> {
    use std::io::Read as _;
    let file = match open_read(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    let mut text = String::new();
    file.take(limit.saturating_add(1))
        .read_to_string(&mut text)?;
    if u64::try_from(text.len()).unwrap_or(u64::MAX) > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the file is too long",
        ));
    }
    Ok(Some(text))
}

/// Flushes `path`'s bytes to the disk (`fsync`): a file about to be vouched for by a receipt.
pub(crate) fn sync_file(path: &Path) -> io::Result<()> {
    open_no_follow(OpenOptions::new().write(true), path)?.sync_all()
}

/// `fs::rename`: `from` replaces `to` (a file), or moves a folder to a free name.
pub(crate) fn rename(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

/// Removes the file at `path`; a missing file is fine.
pub(crate) fn remove_file(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Removes the folder at `path` and everything in it, never following a link out of it; a
/// missing folder is fine.
pub(crate) fn remove_dir_all(path: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Writes `bytes` to `path` atomically: into `.<name>.tmp` beside it, synced, then renamed
/// over `path`. A crash leaves the old file or none, never half of the new one.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    let written = (|| {
        let mut file = open_truncate(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    match written.and_then(|()| std::fs::rename(&tmp, path)) {
        Ok(()) => Ok(()),
        Err(err) => {
            drop(remove_file(&tmp));
            Err(err)
        }
    }
}

/// Writes the `sh` script `bytes` to `path` atomically, as [`write_atomic`] does, with mode 0700
/// set before the rename: the sign-in's `BROWSER` shim, which only its owner may run or change.
#[cfg(unix)]
pub(crate) fn write_script(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    let written = (|| {
        let mut file = open_truncate(&tmp)?;
        file.write_all(bytes)?;
        file.set_permissions(std::fs::Permissions::from_mode(0o700))?;
        file.sync_all()
    })();
    match written.and_then(|()| std::fs::rename(&tmp, path)) {
        Ok(()) => Ok(()),
        Err(err) => {
            drop(remove_file(&tmp));
            Err(err)
        }
    }
}

/// Creates `path` new and empty, readable and writable by its owner alone (mode 0600, set again
/// on the open file so no umask widens or narrows it). An existing file or a link at `path` is
/// refused: a sign-in run's URL file.
#[cfg(unix)]
pub(crate) fn create_private(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
    let file = open_no_follow(
        OpenOptions::new().write(true).create_new(true).mode(0o600),
        path,
    )?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
}

/// Removes `path` whatever it is: a folder with everything in it, or a file or link (never
/// what a link points to). A missing path is fine.
pub(crate) fn remove_any(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => remove_dir_all(path),
        Ok(_) => remove_file(path),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// Tests only: sets `path`'s Unix mode.
#[cfg(all(test, unix))]
pub(crate) fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

/// Tests only: makes a symbolic link at `link` to `target`.
#[cfg(all(unix, any(test, feature = "test-support")))]
pub(crate) fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// Makes a hard link at `link` to `original`: one file under two names, `std::fs::hard_link`,
/// which opens neither (`link(2)` on Unix, `CreateHardLinkW` on Windows). An existing `link`
/// is an error, never replaced. Codex's reused login (`signin/reuse.rs`), and tests.
pub(crate) fn hard_link(original: &Path, link: &Path) -> io::Result<()> {
    std::fs::hard_link(original, link)
}

/// Tests only: makes a symbolic link at `link` to the file `target`. Err when this account may
/// not make one (no Developer Mode or privilege).
#[cfg(all(test, windows))]
pub(crate) fn symlink_file(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[cfg(test)]
mod tests;

/// Tests only: a fresh folder under the temp folder, removed on drop.
#[cfg(test)]
pub(crate) struct TestDir(pub(crate) std::path::PathBuf);

#[cfg(test)]
impl TestDir {
    /// A new empty folder named for `tag`, this process and a counter.
    pub(crate) fn new(tag: &str) -> TestDir {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("clauseo-agents-{tag}-{}-{n}", std::process::id()));
        drop(remove_dir_all(&path));
        std::fs::create_dir_all(&path).unwrap();
        TestDir(path)
    }

    /// Writes `contents` to `rel` (`/`-separated) under the folder, making its folders.
    pub(crate) fn file(&self, rel: &str, contents: &[u8]) -> std::path::PathBuf {
        let path = rel
            .split('/')
            .fold(self.0.clone(), |path, part| path.join(part));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        path
    }
}

#[cfg(test)]
impl Drop for TestDir {
    fn drop(&mut self) {
        drop(remove_dir_all(&self.0));
    }
}
