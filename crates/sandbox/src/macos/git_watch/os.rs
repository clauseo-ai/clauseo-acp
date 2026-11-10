//! The real file system of the git watch, every operation by descriptor (`os/fd.rs`): a path's
//! folder is opened with `O_NOFOLLOW_ANY`, so no link anywhere in it is followed, and its last
//! name is read or changed with the `*at` calls on that folder; a walk holds each folder open
//! while it lists it and opens its children from it. The folders it makes are the lawyer's
//! alone, and a rename never replaces. Also the matter state file's lock and writes, and the
//! volume set-aside folder's checks, by descriptor too.
//!
//! Automounts. The watch never enters or lists a folder on a network volume (no `MNT_LOCAL`),
//! one the automounter mounted (`MNT_AUTOMOUNTED`, which autofs's own mounts carry), or at or
//! below `/net` or `/Network` (never even looked up), and it decides that without firing an
//! automount trigger. A trigger fires when a lookup crosses it, or when the last name of a
//! lookup is one and the operation opens it; a lookup for attributes doesn't (autofs's
//! `trigger_resolve`, `triggers/triggers.c` in apple-oss-distributions/autofs: `OP_GETATTR`
//! on the last name returns `RESOLVER_NOCHANGE` unless the node is a "force a mount" node, which
//! only autofs's own maps make). So a folder's entry is first read with `fstatat` from its
//! parent's descriptor; on another volume than the parent, that volume's flags are read with
//! `getattrlistat` from the parent (`os/attr.rs`), and only a plain local volume is opened.
//! Checked on this Mac (macOS 27.2): `/System/Volumes/Data/home` is `auto_home`'s autofs mount;
//! `fstatat` of `home` from the Data volume's descriptor gives another `st_dev`, and
//! `getattrlistat` gives flags `0x4500000`, without `MNT_LOCAL` and with `MNT_AUTOMOUNTED`, the
//! same as its `fstatfs`; the mount table was the same 14 mounts before and after (`mount`).
//! `tests_fd.rs` pins that the walk refuses it. `/net` and `/Network` don't exist on this Mac.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Read as _, Write as _};
use std::os::fd::OwnedFd;
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use rustix::fs::{
    AtFlags, CWD, FileType, FlockOperation, Mode, OFlags, RenameFlags, fchmod, flock, fstat,
    mkdirat, openat, readlinkat, renameat, renameat_with, unlinkat,
};

use super::{Fs, Kind, Meta, Stamp};

mod attr;
mod fd;
mod link;

#[cfg(test)]
pub(crate) use fd::fake_flags;
pub(crate) use fd::{Allow, Folder, off_limits};

/// The most bytes of a state file read.
const STATE_MAX: u64 = 4 << 20;

/// The flags a new file is made with: never through a link.
const NEW: OFlags = OFlags::RDWR
    .union(OFlags::CREATE)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

/// The file system itself, which enters no network volume or automount.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Real;

/// The file system itself, let into its matter's own volume ([`Allow`]) at and below the
/// matter: the watches' and the final scans', so an after-only matter on a network or
/// automounted volume is scanned. Without an allowance (a matter that can't be opened) it is
/// [`Real`].
#[derive(Debug, Clone, Default)]
pub(crate) struct Within(Option<Allow>);

impl Within {
    /// The file system of the watch of `matter`, its volume read from its folder now.
    pub(crate) fn of(matter: &Path) -> io::Result<Within> {
        Allow::of(matter).map(|allow| Within(Some(allow)))
    }
}

impl Real {
    /// The wall time, for records.
    #[expect(
        clippy::disallowed_methods,
        reason = "the records' wall time; the watch's logic takes it through Fs"
    )]
    fn now() -> SystemTime {
        SystemTime::now()
    }
}

/// What the file system may enter beyond local volumes that are not automounts.
pub(crate) trait Scope: Send {
    /// The volume it may enter inside its matter, if any.
    fn allow(&self) -> Option<&Allow>;
}

impl Scope for Real {
    fn allow(&self) -> Option<&Allow> {
        None
    }
}

impl Scope for Within {
    fn allow(&self) -> Option<&Allow> {
        self.0.as_ref()
    }
}

/// The lawyer's own mode for what the watch makes.
fn private(dir: bool) -> Mode {
    Mode::from_raw_mode(if dir { 0o700 } else { 0o600 })
}

/// Makes folder `name` in `dir`, the lawyer's alone; an existing one is `AlreadyExists`.
fn make_at(dir: &OwnedFd, name: &OsStr) -> io::Result<()> {
    Ok(mkdirat(dir, name, private(true))?)
}

/// Writes new file `name` in folder `dir`, synced; an existing one is `AlreadyExists`.
fn write_at(dir: &OwnedFd, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
    let fd = openat(dir, name, NEW | OFlags::EXCL, private(false))?;
    let mut file = fs::File::from(fd);
    file.write_all(bytes)?;
    file.sync_all()
}

/// Where the lawyer's own folder `path` leads with every link followed, canonical: the
/// set-aside folder, for the canary's path as `FSEvents` gives it. Never the matter's content:
/// what an agent left is judged with [`Fs::leads_out`].
pub(crate) fn resolve(path: &Path) -> Option<PathBuf> {
    dunce::canonicalize(path).ok()
}

impl<Os: Scope> Fs for Os {
    type Dir = Folder;

    fn meta(&self, path: &Path) -> Option<Meta> {
        self.dated(path).map(|(meta, _)| meta)
    }

    fn dated(&self, path: &Path) -> Option<(Meta, Stamp)> {
        let stat = fd::entry(path, self.allow()).ok()??;
        let stamp = [
            stat.st_mtime,
            stat.st_mtime_nsec,
            stat.st_ctime,
            stat.st_ctime_nsec,
        ];
        Some((fd::meta(&stat), stamp))
    }

    fn touch(&self, path: &Path) -> io::Result<()> {
        let (dir, name) = fd::parent(path, false, self.allow())?;
        let fd = openat(&dir, name, NEW | OFlags::TRUNC, private(false))?;
        fs::File::from(fd).write_all(b"canary\n")
    }

    fn list(&self, dir: &Path) -> io::Result<Vec<(OsString, Kind)>> {
        match fd::open_dir(dir, self.allow())? {
            Some((folder, _)) => self.entries(&folder),
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} is not a folder", dir.display()),
            )),
        }
    }

    fn open_dir(&self, dir: &Path) -> io::Result<Option<(Folder, Meta)>> {
        Ok(fd::open_dir(dir, self.allow())?.map(|(folder, stat)| (folder, fd::meta(&stat))))
    }

    fn open_child(
        &self,
        dir: &Folder,
        name: &OsStr,
        path: &Path,
    ) -> io::Result<Option<(Folder, Meta)>> {
        let Some(folder) = fd::child(dir, name, path, self.allow())? else {
            return Ok(None);
        };
        let stat = fstat(&folder.fd)?;
        Ok(Some((folder, fd::meta(&stat))))
    }

    fn entries(&self, dir: &Folder) -> io::Result<Vec<(OsString, Kind)>> {
        let mut out = Vec::new();
        for entry in rustix::fs::Dir::read_from(&dir.fd)? {
            let entry = entry?;
            let name = OsStr::from_bytes(entry.file_name().to_bytes());
            if name == "." || name == ".." {
                continue;
            }
            let kind = if entry.file_type() == FileType::Unknown {
                fd::lstat_at(&dir.fd, name)?.map_or(Kind::Other, |stat| fd::meta(&stat).kind)
            } else {
                fd::kind(entry.file_type())
            };
            out.push((name.to_owned(), kind));
        }
        Ok(out)
    }

    fn leads_out(&self, link: &Path, root: &Path) -> bool {
        link::leads_out(link, root, self.allow())
    }

    fn local(&self, dir: &Path) -> bool {
        match fd::open_dir(dir, self.allow()) {
            Err(error) => error.kind() != io::ErrorKind::Unsupported,
            Ok(_) => true,
        }
    }

    fn make_dirs(&self, dir: &Path) -> io::Result<()> {
        let mounted = self.allow().is_some_and(|allow| allow.covers(dir));
        if !dir.is_absolute() || (off_limits(dir) && !mounted) {
            return Err(io::Error::other(format!(
                "{} can't be a set-aside folder",
                dir.display()
            )));
        }
        let mut at = openat(CWD, "/", OFlags::RDONLY | OFlags::DIRECTORY, Mode::empty())?;
        for part in dir.components() {
            let name = match part {
                Component::RootDir => continue,
                Component::Normal(name) => name,
                Component::Prefix(_) | Component::CurDir | Component::ParentDir => {
                    return Err(io::Error::other(format!(
                        "{} is not canonical",
                        dir.display()
                    )));
                }
            };
            let open = || openat(&at, name, fd::DIR, Mode::empty());
            at = match open() {
                Err(rustix::io::Errno::NOENT) => {
                    match make_at(&at, name) {
                        Ok(()) => {}
                        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                        Err(error) => return Err(error),
                    }
                    open()?
                }
                found => found?,
            };
        }
        Ok(())
    }

    fn make_dir(&self, dir: &Path) -> io::Result<()> {
        let (above, name) = fd::parent(dir, false, self.allow())?;
        make_at(&above, name)
    }

    fn rename_new(&self, from: &Path, to: &Path) -> io::Result<()> {
        let (from_dir, from_name) = fd::parent(from, true, self.allow())?;
        let (to_dir, to_name) = fd::parent(to, false, self.allow())?;
        // renameatx_np with RENAME_EXCL on macOS.
        renameat_with(
            &from_dir,
            from_name,
            &to_dir,
            to_name,
            RenameFlags::NOREPLACE,
        )
        .map_err(io::Error::from)
    }

    fn write_new(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let (dir, name) = fd::parent(path, false, self.allow())?;
        write_at(&dir, name, bytes)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        let (dir, name) = fd::parent(path, true, self.allow())?;
        let target = readlinkat(&dir, name, Vec::new())?;
        Ok(PathBuf::from(OsString::from_vec(target.into_bytes())))
    }

    fn remove_link(&self, path: &Path) -> io::Result<()> {
        let (dir, name) = fd::parent(path, true, self.allow())?;
        let is_link = fd::lstat_at(&dir, name)?
            .is_some_and(|stat| FileType::from_raw_mode(stat.st_mode) == FileType::Symlink);
        if !is_link {
            return Err(io::Error::other("not a link"));
        }
        Ok(unlinkat(&dir, name, AtFlags::empty())?)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        let (dir, name) = fd::parent(path, false, self.allow())?;
        let is_file = fd::lstat_at(&dir, name)?
            .is_some_and(|stat| FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile);
        if !is_file {
            return Err(io::Error::other("not a file"));
        }
        Ok(unlinkat(&dir, name, AtFlags::empty())?)
    }

    fn random(&self) -> u64 {
        let mut bytes = [0_u8; 8];
        if getrandom::fill(&mut bytes).is_err() {
            // Unguessable is the point; the clock's nanoseconds at least differ.
            let nanos = self
                .now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default();
            return u64::from(nanos.subsec_nanos()) ^ nanos.as_secs().rotate_left(32);
        }
        u64::from_ne_bytes(bytes)
    }

    fn now(&self) -> SystemTime {
        Real::now()
    }
}

/// The matter state file's lock: held while the file is open.
pub(crate) struct Lock(#[expect(dead_code, reason = "held for its flock")] OwnedFd);

/// Takes the advisory lock on `path`, made if missing (the lawyer's alone), waiting for it.
pub(crate) fn lock(path: &Path) -> io::Result<Lock> {
    // Only the `matters` folder: the set-aside folder above it was made at the launch, and one
    // removed since is not made again.
    if let Some(dir) = path.parent() {
        match Real.make_dir(dir) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    let (dir, name) = fd::parent(path, false, None)?;
    let flags = OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = openat(&dir, name, flags, private(false))?;
    flock(&fd, FlockOperation::LockExclusive)?;
    Ok(Lock(fd))
}

/// The bytes of the small regular file at `path`, never through a link.
pub(crate) fn read_small(path: &Path) -> io::Result<Vec<u8>> {
    let (dir, name) = fd::parent(path, false, None)?;
    let not_small = || io::Error::other(format!("{} is not a small file", path.display()));
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = match openat(&dir, name, flags, Mode::empty()) {
        Err(rustix::io::Errno::LOOP) => return Err(not_small()),
        opened => opened?,
    };
    let stat = fstat(&fd)?;
    let size = u64::try_from(stat.st_size).unwrap_or(u64::MAX);
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile || size > STATE_MAX {
        return Err(not_small());
    }
    let mut bytes = Vec::new();
    fs::File::from(fd)
        .take(STATE_MAX + 1)
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > STATE_MAX {
        return Err(not_small());
    }
    Ok(bytes)
}

/// Replaces the file at `path` with `bytes`: a new file beside it, synced, renamed over it.
pub(crate) fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let (dir, name) = fd::parent(path, false, None)?;
    let mut temp = name.to_owned();
    temp.push(format!(".{:016x}.new", Real.random()));
    write_at(&dir, &temp, bytes)?;
    Ok(renameat(&dir, &temp, &dir, name)?)
}

/// Makes and checks the volume set-aside folder `dir` (`<volume root>/.clauseo`), returning
/// the git set-aside folder inside it. It must be a folder, never a link, owned by this user;
/// it is made private when the volume keeps modes. An error says why, plainly.
pub(crate) fn volume_folder(dir: &Path) -> io::Result<PathBuf> {
    let plain = |why: String| {
        io::Error::other(format!(
            "Clauseo can't start an AI tool in this matter: the matter is on another disk, and \
             Clauseo can't keep its safe folder {} there ({why}).",
            dir.display()
        ))
    };
    // The volume is mounted: a volume under `/net` or `/Network` is looked up in.
    let volume = dir.parent().and_then(|root| Allow::of(root).ok());
    let (root, name) =
        fd::parent(dir, false, volume.as_ref()).map_err(|error| plain(error.to_string()))?;
    match make_at(&root, name) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(plain(error.to_string())),
    }
    let folder = match openat(&root, name, fd::DIR, Mode::empty()) {
        Ok(folder) => folder,
        Err(rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR) => {
            return Err(plain("it is not a folder".to_owned()));
        }
        Err(error) => return Err(plain(error.to_string())),
    };
    let stat = fstat(&folder).map_err(|error| plain(error.to_string()))?;
    if stat.st_uid != rustix::process::geteuid().as_raw() {
        return Err(plain("another user owns it".to_owned()));
    }
    if u32::from(stat.st_mode) & 0o077 != 0 {
        if let Err(error) = fchmod(&folder, private(true)) {
            return Err(plain(error.to_string()));
        }
        if fstat(&folder).is_ok_and(|stat| u32::from(stat.st_mode) & 0o077 != 0) {
            let shown = dir.display();
            tracing::warn!(dir = %shown, "the volume keeps no modes: the folder stays open");
        }
    }
    let inner = OsStr::new("git-set-aside");
    match make_at(&folder, inner) {
        Ok(()) => Ok(dir.join(inner)),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            match fd::lstat_at(&folder, inner) {
                Ok(Some(stat)) if FileType::from_raw_mode(stat.st_mode) == FileType::Directory => {
                    Ok(dir.join(inner))
                }
                _ => Err(plain(format!(
                    "{} is not a folder",
                    dir.join(inner).display()
                ))),
            }
        }
        Err(error) => Err(plain(error.to_string())),
    }
}
