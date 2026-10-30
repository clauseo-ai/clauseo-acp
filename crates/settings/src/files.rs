//! Reading and writing files: the [`Files`] trait, and [`SystemFiles`] over `std::fs`.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{fs, process};

/// How this crate reads and writes files, so tests never touch a real folder.
pub trait Files {
    /// The file's bytes, or `None` when it does not exist.
    fn read(&self, path: &Path) -> io::Result<Option<Vec<u8>>>;

    /// Replaces the file whole: a reader sees the old bytes or the new ones, never a mix.
    /// Creates the parent folder when it is missing. When `path` is a symlink, the file it
    /// points to is replaced and the link stays; on Unix the file keeps its permissions.
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;
}

/// [`Files`] over the real file system.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemFiles;

/// Tells apart the temporary files of two writes from one process.
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// How many symlinks a write follows before it gives up, as Linux's `MAXSYMLINKS`.
const MAX_LINKS: usize = 40;

impl Files for SystemFiles {
    fn read(&self, path: &Path) -> io::Result<Option<Vec<u8>>> {
        match fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the one place this crate writes: a temporary file beside the target, then a rename"
    )]
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        // A rename over a symlink would replace the link with a regular file, and the file it
        // pointed to (a dotfiles copy, say) would silently stop changing.
        let path = &link_target(path)?;
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} names no file in a folder", path.display()),
            ));
        };
        fs::create_dir_all(dir)?;
        let temp = dir.join(format!(
            ".{}.{}.{}.tmp",
            name.to_string_lossy(),
            process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let written = fs::File::create(&temp).and_then(|mut file| {
            // The new file would get the umask's mode; a file the user locked down to 0600
            // (agent env lives in it) keeps its mode. Set before any byte is written.
            #[cfg(unix)]
            match fs::metadata(path) {
                Ok(old) => file.set_permissions(old.permissions())?,
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                Err(_) => {}
            }
            file.write_all(bytes)?;
            file.sync_all()
        });
        match written.and_then(|()| fs::rename(&temp, path)) {
            Ok(()) => Ok(()),
            Err(error) => {
                // The write already failed; a temporary file left behind is only litter.
                drop(fs::remove_file(&temp));
                Err(error)
            }
        }
    }
}

/// The file `path` names after following every symlink, or `path` when it is no symlink. A link
/// to a missing file gives that file's path, so the write creates it. Reads only.
fn link_target(path: &Path) -> io::Result<PathBuf> {
    let mut path = path.to_owned();
    for _ in 0..MAX_LINKS {
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let target = fs::read_link(&path)?;
                // A relative target counts from the link's folder; `join` keeps an absolute one.
                path = match path.parent() {
                    Some(dir) => dir.join(target),
                    None => target,
                };
            }
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(path),
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "{} goes through more than {MAX_LINKS} symlinks",
            path.display()
        ),
    ))
}
