//! The std calls that change files, as thin wrappers: the one place in `matters` (outside its
//! tests and bench) that may call them. The workspace `clippy.toml` bans each of them everywhere
//! else, and the expect below covers only this file. [`replace`] is the one way a file in the
//! matter folder changes: a temp file beside it, synced, then renamed over it.

#![expect(
    clippy::disallowed_methods,
    reason = "matters writes files here alone; clippy.toml bans std's writes elsewhere"
)]

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use crate::Error;
use crate::fsync;

/// `fs::create_dir`.
pub(crate) fn create_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir(path)
}

/// `fs::create_dir_all`.
pub(crate) fn create_dir_all(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// `fs::remove_file`.
pub(crate) fn remove_file(path: &Path) -> io::Result<()> {
    std::fs::remove_file(path)
}

/// `fs::remove_dir` (an empty folder only).
pub(crate) fn remove_dir(path: &Path) -> io::Result<()> {
    std::fs::remove_dir(path)
}

/// `fs::remove_dir_all`.
pub(crate) fn remove_dir_all(path: &Path) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}

/// `fs::rename`.
pub(crate) fn rename(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

/// A new file opened for writing; fails if `path` exists.
pub(crate) fn create_new(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

/// Opens an existing file for `fsync::sync_file`: read-only on Unix (loose objects are
/// read-only there), with write access on Windows, where `FlushFileBuffers` needs it.
pub(crate) fn open_to_sync(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        OpenOptions::new().write(true).open(path)
    }
    #[cfg(not(windows))]
    {
        File::open(path)
    }
}

/// Opens `path` for reading without following a symlink at its last component (`O_NOFOLLOW`)
/// on macOS and on Linux x86-64 and aarch64; elsewhere a plain open. Callers compare the open
/// file with the path's `symlink_metadata` where the flag is missing.
pub(crate) fn open_read_nofollow(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(any(
        target_os = "macos",
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(O_NOFOLLOW);
    }
    options.open(path)
}

/// `O_NOFOLLOW`: from nix on macOS; on Linux the values of the libc crate (0.2.191) for the
/// two architectures, since std has no constant for it.
#[cfg(target_os = "macos")]
const O_NOFOLLOW: i32 = nix::fcntl::OFlag::O_NOFOLLOW.bits();
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const O_NOFOLLOW: i32 = 0x2_0000;
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const O_NOFOLLOW: i32 = 0x8000;

/// Windows: opens an existing file for reading with no sharing, which fails with a sharing
/// violation while another program has it open.
#[cfg(windows)]
pub(crate) fn open_exclusive(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    OpenOptions::new().read(true).share_mode(0).open(path)
}

/// Sets the modification time of the file `path`.
#[cfg(test)]
pub(crate) fn set_mtime(path: &Path, mtime: std::time::SystemTime) -> io::Result<()> {
    let file = OpenOptions::new().write(true).open(path)?;
    file.set_times(std::fs::FileTimes::new().set_modified(mtime))
}

/// Appends `bytes` to the file `path` in place.
#[cfg(test)]
pub(crate) fn append(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write as _;
    OpenOptions::new().append(true).open(path)?.write_all(bytes)
}

/// The most temp names [`replace`] tries in one folder before giving up.
const TEMP_TRIES: u32 = 1000;

/// Replaces (or creates) `target` with what `fill` writes: [`stage`], then [`Staged::commit`].
pub(crate) fn replace(
    target: &Path,
    fill: &mut dyn FnMut(&mut File) -> Result<(), Error>,
) -> Result<(), Error> {
    stage(target, fill)?.commit()
}

/// A temp file beside its target, written and synced, waiting to be renamed over it. Dropped
/// without [`Staged::commit`], it removes the temp file.
#[derive(Debug)]
pub(crate) struct Staged {
    temp: Option<PathBuf>,
    target: PathBuf,
}

/// Writes what `fill` writes to a new temp file in `target`'s folder, named `.clauseo-<n>.tmp`
/// (excluded from every checkpoint), and syncs it. Never touches `target` itself; the temp file
/// is removed if anything fails.
pub(crate) fn stage(
    target: &Path,
    fill: &mut dyn FnMut(&mut File) -> Result<(), Error>,
) -> Result<Staged, Error> {
    let dir = target
        .parent()
        .ok_or_else(|| Error::io("replace", target, io::ErrorKind::InvalidInput.into()))?;
    let (temp, mut file) = new_temp(dir)?;
    let staged = Staged {
        temp: Some(temp),
        target: target.to_owned(),
    };
    fill(&mut file)?;
    fsync::sync_file(&file).map_err(|err| Error::io("sync", target, err))?;
    Ok(staged)
}

impl Staged {
    /// Renames the temp file over the target, then syncs the folder. Never truncates the
    /// target in place.
    pub(crate) fn commit(mut self) -> Result<(), Error> {
        let Some(temp) = self.temp.take() else {
            return Ok(());
        };
        if let Err(err) = rename(&temp, &self.target) {
            self.temp = Some(temp);
            return Err(Error::io("rename", &self.target, err));
        }
        let dir = self.target.parent().unwrap_or(&self.target);
        fsync::sync_dir(dir).map_err(|err| Error::io("sync", dir, err))
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if let Some(temp) = self.temp.take()
            && let Err(err) = remove_file(&temp)
        {
            tracing::warn!(path = %temp.display(), error = %err, "temp file left behind");
        }
    }
}

/// A new temp file in `dir`, by the first free name `.clauseo-<n>.tmp`.
fn new_temp(dir: &Path) -> Result<(PathBuf, File), Error> {
    for n in 0..TEMP_TRIES {
        let path = dir.join(format!(".clauseo-{n}.tmp"));
        match create_new(&path) {
            Ok(file) => return Ok((path, file)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(Error::io("create", path, err)),
        }
    }
    Err(Error::io(
        "create",
        dir.join(".clauseo-0.tmp"),
        io::ErrorKind::AlreadyExists.into(),
    ))
}
