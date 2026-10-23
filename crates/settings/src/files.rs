//! Reading and writing files: the [`Files`] trait, and [`SystemFiles`] over `std::fs`.

use std::io::{self, Write as _};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{fs, process};

/// How this crate reads and writes files, so tests never touch a real folder.
pub trait Files {
    /// The file's bytes, or `None` when it does not exist.
    fn read(&self, path: &Path) -> io::Result<Option<Vec<u8>>>;

    /// Replaces the file whole: a reader sees the old bytes or the new ones, never a mix.
    /// Creates the parent folder when it is missing.
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;
}

/// [`Files`] over the real file system.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemFiles;

/// Tells apart the temporary files of two writes from one process.
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

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
