//! The real file system of the git watch: reads never follow a link, the folders it makes are
//! the lawyer's alone, and a rename never replaces. Also the matter state file's lock and
//! writes, and the volume set-aside folder's checks.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write as _};
use std::os::unix::fs::{
    DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rustix::fs::{CWD, FlockOperation, RenameFlags, flock, renameat_with};

use super::{Fs, Kind, Meta};

/// `statfs`'s flag for a volume on this Mac (`<sys/mount.h>`).
const MNT_LOCAL: u32 = 0x0000_1000;

/// The most bytes of a state file read.
const STATE_MAX: u64 = 4 << 20;

/// The file system itself.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Real;

fn kind(file_type: fs::FileType) -> Kind {
    if file_type.is_symlink() {
        Kind::Link
    } else if file_type.is_dir() {
        Kind::Dir
    } else if file_type.is_file() {
        Kind::File
    } else {
        Kind::Other
    }
}

/// Whether `dir` is on a local volume: its `statfs` has `MNT_LOCAL`. One that can't be read
/// counts as local; its listing then fails on its own.
// once #766 lands: use network_paths
pub(crate) fn local(dir: &Path) -> bool {
    rustix::fs::statfs(dir).map_or(true, |stat| stat.f_flags & MNT_LOCAL != 0)
}

impl Fs for Real {
    fn meta(&self, path: &Path) -> Option<Meta> {
        let meta = fs::symlink_metadata(path).ok()?;
        Some(Meta {
            kind: kind(meta.file_type()),
            id: (meta.dev(), meta.ino()),
        })
    }

    fn list(&self, dir: &Path) -> io::Result<Vec<(OsString, Kind)>> {
        let mut out = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            out.push((entry.file_name(), kind(entry.file_type()?)));
        }
        Ok(out)
    }

    fn resolve(&self, path: &Path) -> Option<PathBuf> {
        dunce::canonicalize(path).ok()
    }

    fn local(&self, dir: &Path) -> bool {
        local(dir)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the git watch's set-aside folder, outside the jail, the lawyer's alone"
    )]
    fn make_dirs(&self, dir: &Path) -> io::Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "one set-aside item's folder, outside the jail, the lawyer's alone"
    )]
    fn make_dir(&self, dir: &Path) -> io::Result<()> {
        fs::DirBuilder::new().mode(0o700).create(dir)
    }

    fn rename_new(&self, from: &Path, to: &Path) -> io::Result<()> {
        // renameatx_np with RENAME_EXCL on macOS.
        renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE).map_err(io::Error::from)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "one set-aside item's record, new, in its own folder"
    )]
    fn write_new(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(bytes)?;
        file.sync_all()
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        fs::read_link(path)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "a link an agent left in the matter, its target recorded, once it is gone"
    )]
    fn remove_link(&self, path: &Path) -> io::Result<()> {
        if !fs::symlink_metadata(path)?.file_type().is_symlink() {
            return Err(io::Error::other("not a link"));
        }
        fs::remove_file(path)
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

    #[expect(
        clippy::disallowed_methods,
        reason = "the records' wall time; the watch's logic takes it through Fs"
    )]
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// The matter state file's lock: held while the file is open.
pub(crate) struct Lock(#[expect(dead_code, reason = "held for its flock")] fs::File);

/// Takes the advisory lock on `path`, made if missing (the lawyer's alone), waiting for it.
#[expect(
    clippy::disallowed_methods,
    reason = "the matter state file's lock, in the git set-aside folder outside the jail"
)]
pub(crate) fn lock(path: &Path) -> io::Result<Lock> {
    if let Some(dir) = path.parent() {
        Real.make_dirs(dir)?;
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits().cast_signed())
        .open(path)?;
    flock(&file, FlockOperation::LockExclusive)?;
    Ok(Lock(file))
}

/// The bytes of the small regular file at `path`, never through a link.
pub(crate) fn read_small(path: &Path) -> io::Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.len() > STATE_MAX {
        return Err(io::Error::other(format!(
            "{} is not a small file",
            path.display()
        )));
    }
    fs::read(path)
}

/// Replaces the file at `path` with `bytes`: a new file beside it, synced, renamed over it.
#[expect(
    clippy::disallowed_methods,
    reason = "the matter state file, in the git set-aside folder outside the jail"
)]
pub(crate) fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    let temp = path.with_file_name(format!(
        "{}.{:016x}.new",
        name.unwrap_or_default(),
        Real.random()
    ));
    Real.write_new(&temp, bytes)?;
    fs::rename(&temp, path)
}

/// Makes and checks the volume set-aside folder `dir` (`<volume root>/.clauseo`), returning
/// the git set-aside folder inside it. It must be a folder, never a link, owned by this user;
/// it is made private when the volume keeps modes. An error says why, plainly.
#[expect(
    clippy::disallowed_methods,
    reason = "the volume's .clauseo folder, outside the jail, the lawyer's alone"
)]
pub(crate) fn volume_folder(dir: &Path) -> io::Result<PathBuf> {
    let plain = |why: String| {
        io::Error::other(format!(
            "Clauseo can't start an AI tool in this matter: the matter is on another disk, and \
             Clauseo can't keep its safe folder {} there ({why}).",
            dir.display()
        ))
    };
    match Real.make_dir(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(plain(error.to_string())),
    }
    let meta = fs::symlink_metadata(dir).map_err(|error| plain(error.to_string()))?;
    if !meta.file_type().is_dir() {
        return Err(plain("it is not a folder".to_owned()));
    }
    if meta.uid() != rustix::process::geteuid().as_raw() {
        return Err(plain("another user owns it".to_owned()));
    }
    if meta.mode() & 0o077 != 0 {
        let private = fs::Permissions::from_mode(0o700);
        if let Err(error) = fs::set_permissions(dir, private) {
            return Err(plain(error.to_string()));
        }
        if fs::symlink_metadata(dir).is_ok_and(|meta| meta.mode() & 0o077 != 0) {
            let shown = dir.display();
            tracing::warn!(dir = %shown, "the volume keeps no modes: the folder stays open");
        }
    }
    let inner = dir.join("git-set-aside");
    match Real.make_dir(&inner) {
        Ok(()) => Ok(inner),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            match fs::symlink_metadata(&inner) {
                Ok(meta) if meta.file_type().is_dir() => Ok(inner),
                _ => Err(plain(format!("{} is not a folder", inner.display()))),
            }
        }
        Err(error) => Err(plain(error.to_string())),
    }
}
