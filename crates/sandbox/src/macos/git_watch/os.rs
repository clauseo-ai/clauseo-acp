//! The real file system of the git watch: reads never follow a link, the folders it makes are
//! the lawyer's alone, and a rename never replaces.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rustix::fs::{CWD, RenameFlags, renameat_with};

use super::{Fs, Kind, Meta};

/// `statfs`'s flag for a volume on this Mac (`<sys/mount.h>`).
const MNT_LOCAL: u32 = 0x0000_1000;

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

    #[expect(
        clippy::disallowed_methods,
        reason = "the records' wall time; the watch's logic takes it through Fs"
    )]
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}
