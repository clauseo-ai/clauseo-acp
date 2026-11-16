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

/// `fs::remove_file`. In tests a file can come back after it, as after any [`removing`].
pub(crate) fn remove_file(path: &Path) -> io::Result<()> {
    removing(path, delete_file)
}

/// Takes the file at `path` out of the folder with `remove`: a move to a trash
/// ([`crate::Trash`]), or [`delete_file`]. In tests a file can come back after it
/// (`no_download::resurrect` in unit tests, `test_support::bring_back_after_delete` with the
/// `test-support` feature), whichever way it went.
pub(crate) fn removing<T>(
    path: &Path,
    remove: impl FnOnce(&Path) -> io::Result<T>,
) -> io::Result<T> {
    #[cfg(test)]
    let back = crate::no_download::resurrect::removing(path);
    #[cfg(feature = "test-support")]
    let bytes = crate::test_support::before_delete(path);
    let removed = remove(path);
    #[cfg(test)]
    if let (Ok(_), Some(back)) = (&removed, back) {
        crate::no_download::resurrect::removed(path, back);
    }
    #[cfg(feature = "test-support")]
    if let (Ok(_), Some(bytes)) = (&removed, bytes) {
        crate::test_support::after_delete(path, &bytes);
    }
    removed
}

/// `fs::remove_file`, without the test hooks: what [`removing`] calls.
pub(crate) fn delete_file(path: &Path) -> io::Result<()> {
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

/// Windows, tests only: opens `path` as Word holds a document it has open, for reading and
/// writing, sharing reads only (`FILE_SHARE_READ`): no one may delete it or rename over it
/// until the handle drops.
#[cfg(all(test, windows))]
pub(crate) fn open_as_word(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    /// `FILE_SHARE_READ`.
    const SHARE_READ: u32 = 0x1;
    OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(SHARE_READ)
        .open(path)
}

/// `FILE_ATTRIBUTE_HIDDEN`.
#[cfg(any(windows, test))]
const HIDDEN: u32 = 0x2;

/// The attributes that hide a file or folder whose attributes are `current`, keeping every
/// other one: `None` when it is hidden already. Git for Windows' rule for `.git`
/// (`set_hidden_flag` in its `compat/mingw.c`). Plain arithmetic, so tested on every OS.
#[cfg(any(windows, test))]
pub(crate) const fn hidden_attributes(current: u32) -> Option<u32> {
    if current & HIDDEN == 0 {
        Some(current | HIDDEN)
    } else {
        None
    }
}

/// Windows: sets `FILE_ATTRIBUTE_HIDDEN` on the folder `dir`, keeping the attributes it has,
/// unless it is hidden already or is not a plain folder. "Plain" is std's `is_dir` on the
/// link's own metadata: a symlink or junction (a name-surrogate reparse point) is left alone; a
/// folder with any other reparse point, such as a `OneDrive` placeholder, is hidden like any other.
/// A failure is logged, not returned: hiding is cosmetic. The first failure in a process is a
/// warning and later ones are debug, so a share that refuses the attribute does not warn on every
/// open. Elsewhere nothing; the leading dot of `.clauseo` hides it on macOS and Linux.
pub(crate) fn hide_dir(dir: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        let result = std::fs::symlink_metadata(dir).and_then(|metadata| {
            if !metadata.is_dir() {
                tracing::debug!(path = %dir.display(), "not a plain folder; not hidden");
                return Ok(());
            }
            match hidden_attributes(metadata.file_attributes()) {
                Some(attributes) => windows::set_attributes(dir, attributes),
                None => Ok(()),
            }
        });
        if let Err(err) = result {
            if HIDE_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                tracing::debug!(path = %dir.display(), error = %err, "folder not hidden");
            } else {
                tracing::warn!(error = %err, "folder not hidden");
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = dir;
    }
}

/// Whether [`hide_dir`] has warned once in this process.
#[cfg(windows)]
static HIDE_WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(windows)]
mod windows;

#[cfg(unix)]
mod keep;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(any(windows, test))]
mod replace_keeping;

/// Sets the modification time of the file `path`.
#[cfg(test)]
pub(crate) fn set_mtime(path: &Path, mtime: std::time::SystemTime) -> io::Result<()> {
    let file = OpenOptions::new().write(true).open(path)?;
    file.set_times(std::fs::FileTimes::new().set_modified(mtime))
}

/// `fs::set_permissions`.
#[cfg(test)]
pub(crate) fn set_permissions(path: &Path, permissions: std::fs::Permissions) -> io::Result<()> {
    std::fs::set_permissions(path, permissions)
}

/// Appends `bytes` to the file `path` in place.
#[cfg(test)]
pub(crate) fn append(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write as _;
    OpenOptions::new().append(true).open(path)?.write_all(bytes)
}

/// Writes `bytes` over the start of the file `path` in place, keeping the same file.
#[cfg(test)]
pub(crate) fn overwrite(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write as _;
    OpenOptions::new().write(true).open(path)?.write_all(bytes)
}

/// The most temp names [`replace`] tries in one folder before giving up.
const TEMP_TRIES: u32 = 1000;

/// Replaces (or creates) `target`, one of Clauseo's own files, with what `fill` writes: staged
/// as [`stage`] does but with the defaults' permissions and nothing kept of the old file, then
/// [`Staged::commit`].
pub(crate) fn replace(
    target: &Path,
    fill: &mut dyn FnMut(&mut File) -> Result<(), Error>,
) -> Result<(), Error> {
    stage_as(target, false, fill)?.commit()
}

/// A temp file beside its target, written and synced, waiting to be renamed over it. Dropped
/// without [`Staged::commit`], it removes the temp file.
#[derive(Debug)]
pub(crate) struct Staged {
    temp: Option<PathBuf>,
    target: PathBuf,
    /// Whether the target keeps its metadata: a lawyer's file, staged by [`stage`].
    keep: bool,
    /// The things of the target's metadata the temp file could not take ([`keep::copy`]).
    lost: Lost,
    /// Unix: what is copied once the temp file is the target (macOS: the ACL).
    #[cfg(unix)]
    after: Option<keep::After>,
}

/// How many things of a replaced file's metadata (its permissions, an extended attribute, its
/// ACL; on Windows all of them, when `ReplaceFileW` failed) a rewrite did not keep, and the OS
/// error code (errno, or the Win32 code) of the last one. Never a failure of the write.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Lost {
    pub(crate) count: u32,
    pub(crate) code: Option<i32>,
}

impl Lost {
    /// One thing lost, with the error's code.
    pub(crate) const fn one(code: Option<i32>) -> Self {
        Self { count: 1, code }
    }

    /// Adds `other` to these; its code, if any, wins.
    pub(crate) fn add(&mut self, other: Self) {
        self.count += other.count;
        if other.code.is_some() {
            self.code = other.code;
        }
    }
}

/// Why [`Staged::rename`] failed.
#[derive(Debug)]
pub(crate) struct RenameFailed {
    pub(crate) error: Error,
    /// The target is not there any more: the folder changed. The temp file, which holds the new
    /// content, is kept (Windows, after a failed `ReplaceFileW` that moved the old file away).
    pub(crate) changed: bool,
}

/// A staged file renamed into place ([`Staged::rename`]).
#[derive(Debug)]
pub(crate) struct Renamed {
    /// The target's folder, which the caller syncs.
    pub(crate) dir: PathBuf,
    /// What of the old file's metadata the new one did not keep; the caller logs the total once.
    pub(crate) lost: Lost,
}

/// Writes what `fill` writes to a new temp file in `target`'s folder, named `.clauseo-<n>.tmp`
/// (excluded from every checkpoint), and syncs it. Never touches `target` itself; the temp file
/// is removed if anything fails. A rewrite of a lawyer's file: the temp file takes the target's
/// permissions and extended attributes (Unix, [`keep::copy`]), and the rename keeps the rest
/// (Windows, `ReplaceFileW`). A missing target leaves the defaults.
pub(crate) fn stage(
    target: &Path,
    fill: &mut dyn FnMut(&mut File) -> Result<(), Error>,
) -> Result<Staged, Error> {
    stage_as(target, true, fill)
}

/// [`stage`], keeping the target's metadata when `keep`.
fn stage_as(
    target: &Path,
    keep: bool,
    fill: &mut dyn FnMut(&mut File) -> Result<(), Error>,
) -> Result<Staged, Error> {
    let dir = target
        .parent()
        .ok_or_else(|| Error::io("replace", target, io::ErrorKind::InvalidInput.into()))?;
    let (temp, mut file) = new_temp(dir)?;
    let staged = Staged {
        temp: Some(temp),
        target: target.to_owned(),
        keep,
        lost: Lost::default(),
        #[cfg(unix)]
        after: None,
    };
    fill(&mut file)?;
    // Before the sync, which then covers the metadata too.
    #[cfg(unix)]
    let staged = {
        let mut staged = staged;
        if keep {
            let (lost, after) = keep::copy(target, &file);
            staged.lost = lost;
            staged.after = Some(after);
        }
        staged
    };
    #[cfg(test)]
    injected("keep").map_err(|err| Error::io("write", target, err))?;
    fsync::sync_file(&file).map_err(|err| Error::io("sync", target, err))?;
    #[cfg(test)]
    injected("sync").map_err(|err| Error::io("sync", target, err))?;
    Ok(staged)
}

impl Staged {
    /// Renames the temp file over the target, then syncs the folder. Never truncates the
    /// target in place.
    pub(crate) fn commit(self) -> Result<(), Error> {
        let Renamed { dir, .. } = self.rename().map_err(|failed| failed.error)?;
        fsync::sync_dir(&dir).map_err(|err| Error::io("sync", &dir, err))
    }

    /// Renames the temp file over the target without syncing the folder; returns the folder,
    /// which the caller syncs (once for many renames in it) before relying on the rename. A
    /// lawyer's file on Windows goes in by `ReplaceFileW` ([`replace_keeping`]).
    pub(crate) fn rename(self) -> Result<Renamed, RenameFailed> {
        #[cfg(windows)]
        let replace: Option<&ReplaceFn> = Some(&windows::replace_file);
        #[cfg(not(windows))]
        let replace: Option<&ReplaceFn> = None;
        self.rename_with(replace)
    }

    /// [`Self::rename`], with `replace` (`ReplaceFileW`, or a simulation of it in tests) for a
    /// lawyer's file, or the plain rename.
    fn rename_with(mut self, replace: Option<&ReplaceFn>) -> Result<Renamed, RenameFailed> {
        let dir = self.target.parent().unwrap_or(&self.target).to_owned();
        let Some(temp) = self.temp.take() else {
            return Ok(Renamed {
                dir,
                lost: self.lost,
            });
        };
        #[cfg(test)]
        if let Err(err) = injected("rename") {
            self.temp = Some(temp);
            return Err(RenameFailed {
                error: Error::io("rename", &self.target, err),
                changed: false,
            });
        }
        #[cfg(any(windows, test))]
        let renamed = match replace {
            Some(replace) if self.keep => {
                replace_keeping::replace_keeping(&temp, &self.target, replace)
            }
            _ => plain_rename(&temp, &self.target),
        };
        #[cfg(not(any(windows, test)))]
        let renamed = {
            // Only Windows keeps metadata at the rename; elsewhere [`stage`] copied it.
            let _ = (replace, self.keep);
            plain_rename(&temp, &self.target)
        };
        match renamed {
            Ok(lost) => {
                self.lost.add(lost);
                #[cfg(unix)]
                if let Some(after) = self.after.take() {
                    self.lost.add(after.apply());
                }
                Ok(Renamed {
                    dir,
                    lost: self.lost,
                })
            }
            #[cfg(any(windows, test))]
            Err(replace_keeping::Failed::TargetGone(err)) => {
                // The temp file stays: it holds the new content, and the old file is not there.
                Err(RenameFailed {
                    error: Error::io("replace; the folder changed", &self.target, err),
                    changed: true,
                })
            }
            Err(failed) => {
                self.temp = Some(temp);
                Err(RenameFailed {
                    error: Error::io("rename", &self.target, failed.into_error()),
                    changed: false,
                })
            }
        }
    }
}

/// `ReplaceFileW`'s shape: `(replaced, replacement, backup)`.
type ReplaceFn = dyn Fn(&Path, &Path, &Path) -> io::Result<()>;

#[cfg(any(windows, test))]
use replace_keeping::Failed;

/// Elsewhere a failed rename is the one failure: the target is as it was.
#[cfg(not(any(windows, test)))]
#[derive(Debug)]
struct Failed(io::Error);

#[cfg(not(any(windows, test)))]
impl Failed {
    fn into_error(self) -> io::Error {
        self.0
    }
}

/// The plain rename of `temp` over `target`; a failure leaves the target as it was.
fn plain_rename(temp: &Path, target: &Path) -> Result<Lost, Failed> {
    #[cfg(any(windows, test))]
    let failed = Failed::Unchanged;
    #[cfg(not(any(windows, test)))]
    let failed = Failed;
    rename(temp, target)
        .map(|()| Lost::default())
        .map_err(failed)
}

#[cfg(test)]
thread_local! {
    /// Tests: the step of a write that fails, once armed by [`fail_at`] on this thread.
    static FAIL_AT: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

/// Tests: makes the step `step` ("keep", "sync" or "rename") of this thread's writes fail.
#[cfg(test)]
pub(crate) fn fail_at(step: Option<&'static str>) {
    FAIL_AT.with(|at| at.set(step));
}

/// Tests: the failure [`fail_at`] armed for `step`, if any.
#[cfg(test)]
fn injected(step: &'static str) -> io::Result<()> {
    if FAIL_AT.with(std::cell::Cell::get) == Some(step) {
        return Err(io::Error::other(step));
    }
    Ok(())
}

/// Removes the temp file `temp`. When its own ACL refuses that (macOS: a deny-delete entry it
/// inherited, or the folder's `delete_child` denied; Windows: the same, or the read-only
/// attribute), it is first given one that allows it, and removed again.
pub(crate) fn remove_temp(temp: &Path) -> io::Result<()> {
    match remove_file(temp) {
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied && unlock(temp) => {
            remove_file(temp)
        }
        removed => removed,
    }
}

/// Lets the file `temp` be removed; whether that went.
fn unlock(temp: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::allow_delete(temp).is_ok()
    }
    #[cfg(windows)]
    {
        windows::allow_delete(temp).is_ok()
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        // A Linux ACL or mode never refuses a removal: that is the folder's permission.
        let _ = temp;
        false
    }
}

/// Removes the temp files a crash left in `dir` (every `.clauseo-<digits>.tmp`, at any number:
/// a write whose first renames went leaves the later ones), before a write there: the matter's
/// one writer has none in flight then. One listing of `dir`; links and anything not a regular
/// file are skipped. A file that does not go is logged once.
pub(crate) fn sweep_temps(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut left = 0_u32;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !is_temp_name(&name) || !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        match remove_temp(&entry.path()) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => left += 1,
            _ => {}
        }
    }
    if left > 0 {
        tracing::warn!(left, "a crashed write's temp files left behind");
    }
}

/// Whether `name` is exactly `.clauseo-<one or more ASCII digits>.tmp`, [`new_temp`]'s names.
fn is_temp_name(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .and_then(|name| name.strip_prefix(".clauseo-"))
        .and_then(|rest| rest.strip_suffix(".tmp"))
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|byte| byte.is_ascii_digit()))
}

impl Drop for Staged {
    fn drop(&mut self) {
        if let Some(temp) = self.temp.take()
            && let Err(err) = remove_temp(&temp)
        {
            tracing::warn!(error = %err, "temp file left behind");
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

#[cfg(test)]
mod keep_tests;
#[cfg(test)]
mod leftover_tests;
#[cfg(test)]
mod tests;
