//! The browser's socket folder: `/tmp/clauseo-ab-<id>`, made by the host for one launch.
//!
//! agent-browser, which the browser guard runs inside the jail, puts its daemon's Unix socket
//! in a folder of its own. The matter's state path is too long for a socket path (103 bytes on
//! macOS), so the host makes a short folder in `/tmp` before the launch and the jail allows
//! that one folder ([`PolicyBuilder::browser_socket_dir`](crate::PolicyBuilder)). The folder is
//! made with one atomic `mkdir` at mode 0700, never reused, checked with `lstat` after it is
//! made, and removed when its [`BrowserSocketDir`] drops; [`BrowserSocketDir::sweep_stale`]
//! removes folders a crashed host left behind.
//!
//! Beside each folder sits its lock file, `/tmp/clauseo-ab-<id>.lock`, made first and held
//! with `flock` for the folder's whole life, outside the folder so the jail cannot touch it:
//! a sweep skips a folder whose lock another live host process holds.

use std::fs::{File, Metadata};
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, Mode, OFlags};

mod write;

/// Every folder's name starts with this.
const PREFIX: &str = "clauseo-ab-";
/// The lock file's name is the folder's and this.
const LOCK_SUFFIX: &str = ".lock";
/// The id's length.
const ID_LEN: usize = 16;
/// The id's letters.
const ALPHABET: &[u8; 36] = b"abcdefghijklmnopqrstuvwxyz0123456789";
/// How many ids [`BrowserSocketDir::create`] draws before it gives up.
const TRIES: usize = 32;
/// The folder's mode.
const MODE: u32 = 0o700;

/// One launch's browser socket folder, `/tmp/clauseo-ab-<16 letters of a-z and 0-9>`, mode
/// 0700, owned by this process's effective user, with its lock file
/// `/tmp/clauseo-ab-<id>.lock` held (`flock`) for as long as the value lives. Dropping it
/// removes the folder (with what is in it), then the lock file, then releases the lock.
///
/// Drop it only after the launch's whole process tree has exited, as far as the host can tell:
/// the host drops it once the agent's tree was killed, and two processes can still outlive that
/// (an agent that ignores its kill, the process crate's `Unresponsive` step, and agent-browser's
/// Unix daemon, which leaves the tree with `setsid` and lives until its idle timeout, G17 item
/// 6; decision record 0029). The jail's rules match
/// paths, not owners: a jailed process that outlived its session could meet a folder another
/// local user made again at the same path once this one is gone. The random 16-letter name,
/// the drop order and agent-browser's daemon living inside the launch make that unlikely, not
/// impossible.
#[derive(Debug)]
pub struct BrowserSocketDir {
    path: PathBuf,
    canonical: PathBuf,
    id: String,
    lock_path: PathBuf,
    /// Held with `flock(LOCK_EX)` until the value drops; closing it releases the lock.
    _lock: File,
}

impl BrowserSocketDir {
    /// Makes a new folder in `/tmp`. Each attempt draws a fresh id, makes the lock file
    /// (`O_CREAT|O_EXCL|O_NOFOLLOW`, mode 0600) and locks it (`flock`, non-blocking), checks
    /// that the name still points at the file it locked (same device and inode, a regular
    /// file), then makes the folder with one `mkdir` at mode 0700. A name whose lock file or
    /// folder already exists, or whose lock file a sweep holds or replaced in the meantime, is
    /// never used: a new id is drawn, at most 32 times. The folder made is then checked with
    /// `lstat`: a folder, not a link, owned by the effective user, mode exactly 0700. Any
    /// failure after the lock is held removes what this call made (the folder only if it is
    /// the empty one it made) and is an error.
    pub fn create() -> io::Result<BrowserSocketDir> {
        BrowserSocketDir::create_in(Path::new("/tmp"))
    }

    /// [`BrowserSocketDir::create`] in `root` instead of `/tmp`, for tests.
    #[doc(hidden)]
    pub fn create_in(root: &Path) -> io::Result<BrowserSocketDir> {
        create_with(root, random_id)
    }

    /// Makes the lock file and folder for `id` in `root`, or fails; never draws another id.
    pub(crate) fn create_named_in(root: &Path, id: &str) -> io::Result<BrowserSocketDir> {
        BrowserSocketDir::create_checked(root, id, checked)
    }

    /// [`BrowserSocketDir::create_named_in`] with the steps after `mkdir` given as `check`,
    /// which returns the canonical path; tests pass one that fails.
    pub(crate) fn create_checked(
        root: &Path,
        id: &str,
        check: impl FnOnce(&Path) -> io::Result<PathBuf>,
    ) -> io::Result<BrowserSocketDir> {
        BrowserSocketDir::create_hooked(root, id, |_| {}, check)
    }

    /// [`BrowserSocketDir::create_checked`] with `after_lock` run once the lock is held and
    /// before its identity check; tests pass one that swaps the lock file.
    pub(crate) fn create_hooked(
        root: &Path,
        id: &str,
        after_lock: impl FnOnce(&Path),
        check: impl FnOnce(&Path) -> io::Result<PathBuf>,
    ) -> io::Result<BrowserSocketDir> {
        let path = root.join(format!("{PREFIX}{id}"));
        let lock_path = lock_path_of(&path);
        let lock = File::from(write::make_lock(&lock_path)?);
        // A sweeper may hold the new lock file for an instant, or have unlinked it between our
        // open and our lock: either way the name is lost, and the caller draws another id.
        match rustix::fs::flock(&lock, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => {}
            Err(rustix::io::Errno::WOULDBLOCK) => return Err(name_lost(id)),
            Err(error) => {
                drop(lock);
                if let Err(remove) = write::remove_file(&lock_path) {
                    tracing::warn!(%id, %remove, "browser socket lock file left");
                }
                return Err(error.into());
            }
        }
        after_lock(&lock_path);
        if !same_file(&lock, &lock_path)? {
            return Err(name_lost(id));
        }
        let made = write::make_dir(&path, MODE).and_then(|()| check(&path));
        let canonical = match made {
            Ok(canonical) => canonical,
            Err(error) => {
                let folder_ours = std::fs::symlink_metadata(&path).is_ok_and(|meta| ours(&meta));
                // AlreadyExists from mkdir: the folder is someone else's.
                // The folder is still the empty one this call made: nothing has run in it.
                if folder_ours
                    && error.kind() != io::ErrorKind::AlreadyExists
                    && let Err(remove) = write::remove_empty(&path)
                {
                    tracing::warn!(%id, %remove, "browser socket folder left");
                }
                if let Err(remove) = write::remove_file(&lock_path) {
                    tracing::warn!(%id, %remove, "browser socket lock file left");
                }
                return Err(error);
            }
        };
        Ok(BrowserSocketDir {
            path,
            canonical,
            id: id.to_owned(),
            lock_path,
            _lock: lock,
        })
    }

    /// The folder as made, `/tmp/clauseo-ab-<id>`: the path the browser guard gets.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The id, 16 letters of `a-z` and `0-9`.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The folder's canonical path (`/private/tmp/...` on macOS), resolved once when it was
    /// made: the path the jail's rules name.
    pub fn canonical(&self) -> &Path {
        &self.canonical
    }

    /// Removes the browser socket folders in `/tmp` an earlier host left behind, for host
    /// start. A folder goes when it is named `clauseo-ab-` and 16 letters of `a-z` and `0-9`,
    /// is a real folder (not a link) owned by the effective user, `keep` says no to its id,
    /// and its lock file is a regular file we own that no live process holds (a non-blocking
    /// `flock` takes it, and the name still points at the file locked); then the folder, then
    /// its lock file, are removed. A lock file that is missing, a link, not a regular file, or
    /// not ours keeps its folder: a crash cannot leave a folder without its lock file, which
    /// is made first and removed last. A lock file
    /// of ours with no folder and no holder is removed too. Anything else is left alone.
    ///
    /// Returns how many folders were removed. Only failing to read `/tmp` is an error; a
    /// failure on one entry is logged by name and the sweep goes on.
    pub fn sweep_stale(keep: impl Fn(&str) -> bool) -> io::Result<usize> {
        BrowserSocketDir::sweep_stale_in(Path::new("/tmp"), keep)
    }

    /// [`BrowserSocketDir::sweep_stale`] in `root` instead of `/tmp`, for tests.
    #[doc(hidden)]
    pub fn sweep_stale_in(root: &Path, keep: impl Fn(&str) -> bool) -> io::Result<usize> {
        let mut removed = 0;
        for entry in std::fs::read_dir(root)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    tracing::warn!(%error, "browser socket sweep: an entry could not be read");
                    continue;
                }
            };
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let path = root.join(name);
            let swept = if let Some(id) = id_of(name) {
                if keep(id) {
                    continue;
                }
                sweep_folder(&path)
            } else if let Some(id) = name.strip_suffix(LOCK_SUFFIX).and_then(id_of) {
                if keep(id) {
                    continue;
                }
                sweep_lone_lock(&path, &root.join(format!("{PREFIX}{id}")))
            } else {
                continue;
            };
            match swept {
                Ok(true) => removed += usize::from(id_of(name).is_some()),
                Ok(false) => {}
                Err(error) => tracing::warn!(%name, %error, "browser socket sweep: entry left"),
            }
        }
        Ok(removed)
    }
}

impl Drop for BrowserSocketDir {
    /// Removes the folder, then the lock file; the lock is released when the descriptor
    /// closes, after this body. Drop only after the launch's process tree has exited (see the
    /// type's documentation).
    fn drop(&mut self) {
        // `remove_dir_all` removes a link without following it.
        if let Err(error) = write::remove_tree(&self.path) {
            tracing::debug!(id = %self.id, %error, "browser socket folder left");
        }
        if let Err(error) = write::remove_file(&self.lock_path) {
            tracing::debug!(id = %self.id, %error, "browser socket lock file left");
        }
    }
}

/// The steps after `mkdir`: [`verify`], then the canonical path.
fn checked(path: &Path) -> io::Result<PathBuf> {
    verify(path)?;
    dunce::canonicalize(path)
}

/// Whether `path` is, without following a link, the regular file `file` has open (same
/// device and inode). False when the name is gone or names something else.
fn same_file(file: &File, path: &Path) -> io::Result<bool> {
    let open = file.metadata()?;
    match std::fs::symlink_metadata(path) {
        Ok(named) => Ok(named.file_type().is_file()
            && named.dev() == open.dev()
            && named.ino() == open.ino()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// The error for a lock file a sweeper took or replaced before this create held it: the
/// caller draws another id.
fn name_lost(id: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        format!("browser socket lock file for {id} was taken by a sweep"),
    )
}

/// The lock file beside `folder`.
fn lock_path_of(folder: &Path) -> PathBuf {
    let mut name = folder.as_os_str().to_owned();
    name.push(LOCK_SUFFIX);
    PathBuf::from(name)
}

/// What a sweep found at a lock file's path.
enum Lock {
    /// No lock file.
    Missing,
    /// Ours and unheld: taken here, held while the file lives.
    Taken(File),
    /// Held by a live process, or not a regular file of ours: leave its folder.
    Keep,
}

/// Looks at the lock file `path` without following a link, and takes it if no one holds it.
fn take_lock(path: &Path) -> io::Result<Lock> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Lock::Missing),
        Err(error) => return Err(error),
    };
    if !meta.file_type().is_file() || meta.uid() != effective_uid() {
        return Ok(Lock::Keep);
    }
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
    let file = File::from(rustix::fs::open(path, flags, Mode::empty())?);
    match rustix::fs::flock(&file, FlockOperation::NonBlockingLockExclusive) {
        // The file opened may have been unlinked and the name made again since: only the lock
        // of the file the name still points at counts.
        Ok(()) if same_file(&file, path)? => Ok(Lock::Taken(file)),
        Ok(()) | Err(rustix::io::Errno::WOULDBLOCK) => Ok(Lock::Keep),
        Err(error) => Err(error.into()),
    }
}

/// Removes a stale folder of ours and then its lock file. `Ok(true)` when the folder went.
fn sweep_folder(path: &Path) -> io::Result<bool> {
    let meta = std::fs::symlink_metadata(path)?;
    if !ours(&meta) {
        return Ok(false);
    }
    let lock_path = lock_path_of(path);
    // A crash cannot leave a folder without its lock file: the lock file is made before the
    // folder and removed after it. A folder with none is being made or dropped right now, or
    // is not one of ours to judge, so it stays.
    let _held = match take_lock(&lock_path)? {
        Lock::Keep | Lock::Missing => return Ok(false),
        Lock::Taken(held) => held,
    };
    write::remove_tree(path)?;
    write::remove_file(&lock_path)?;
    Ok(true)
}

/// Removes a lock file of ours whose folder is gone and that no one holds.
fn sweep_lone_lock(lock_path: &Path, folder: &Path) -> io::Result<bool> {
    if std::fs::symlink_metadata(folder).is_ok() {
        // The folder's own entry decides.
        return Ok(false);
    }
    match take_lock(lock_path)? {
        Lock::Taken(_held) => write::remove_file(lock_path).map(|()| true),
        Lock::Missing | Lock::Keep => Ok(false),
    }
}

/// Draws ids from `next` until a folder is made in `root`, an error other than a lost name
/// happens, or [`TRIES`] names were lost.
fn create_with(
    root: &Path,
    next: impl FnMut() -> io::Result<String>,
) -> io::Result<BrowserSocketDir> {
    create_each(next, |id| BrowserSocketDir::create_named_in(root, id))
}

/// Draws ids from `next` and tries `make` on each until one is made, an error other than a
/// lost name happens, or [`TRIES`] names were lost.
fn create_each(
    mut next: impl FnMut() -> io::Result<String>,
    mut make: impl FnMut(&str) -> io::Result<BrowserSocketDir>,
) -> io::Result<BrowserSocketDir> {
    for _ in 0..TRIES {
        let id = next()?;
        match make(&id) {
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::AlreadyExists | io::ErrorKind::WouldBlock
                ) => {}
            made => return made,
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("{TRIES} browser socket folder names in a row were taken"),
    ))
}

/// 16 letters of [`ALPHABET`], from the OS's randomness without bias.
fn random_id() -> io::Result<String> {
    let mut id = String::with_capacity(ID_LEN);
    let limit = u8::try_from(256 - 256 % ALPHABET.len()).unwrap_or(u8::MAX);
    while id.len() < ID_LEN {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
        for byte in bytes {
            if byte < limit && id.len() < ID_LEN {
                let letter = ALPHABET.get(usize::from(byte) % ALPHABET.len());
                id.extend(letter.map(|&b| char::from(b)));
            }
        }
    }
    Ok(id)
}

/// The id in a folder name `clauseo-ab-<id>`, if the name has that shape.
fn id_of(name: &str) -> Option<&str> {
    let id = name.strip_prefix(PREFIX)?;
    let shaped = id.len() == ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    shaped.then_some(id)
}

/// Whether `meta` (from `lstat`) is a real folder owned by the effective user.
fn ours(meta: &Metadata) -> bool {
    meta.file_type().is_dir() && meta.uid() == effective_uid()
}

/// The checks after `mkdir`: a real folder, owned by the effective user, mode exactly 0700.
pub(crate) fn verify(path: &Path) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    let refuse = |why: &str| {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("browser socket folder {} refused: {why}", path.display()),
        ))
    };
    if meta.file_type().is_symlink() {
        return refuse("a symbolic link");
    }
    if !meta.file_type().is_dir() {
        return refuse("not a folder");
    }
    if meta.uid() != effective_uid() {
        return refuse("owned by another user");
    }
    let mode = meta.permissions().mode() & 0o7777;
    if mode != MODE {
        return refuse(&format!("mode {mode:o}, not {MODE:o}"));
    }
    Ok(())
}

/// This process's effective user id.
fn effective_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}

#[cfg(test)]
mod tests;
