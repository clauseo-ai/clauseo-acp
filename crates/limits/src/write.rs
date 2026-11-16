//! The std calls that change files: the one place in `limits` that may call them. The workspace
//! `clippy.toml` bans each of them everywhere else, and the expect below covers only this file.
//!
//! Two things change: the helper file in the adapter's install folder ([`replace_if_differs`]),
//! and the readers' working folder `<data_dir>/limits/run` ([`create_dir_all`]), made if missing;
//! nothing in Clauseo writes there.

#![expect(
    clippy::disallowed_methods,
    reason = "limits writes files here alone; clippy.toml bans std's writes elsewhere"
)]

use std::fs::{File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::Error;

/// Numbers the temp files of one process, so two calls never pick the same name.
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// `fs::create_dir_all`.
pub(crate) fn create_dir_all(path: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(path).map_err(|err| Error::io("create", path, err))
}

/// Makes `target` hold exactly `bytes`. Reads the file there first, and writes only when it
/// differs (a missing file differs): a new temp file beside it, named for this process and this
/// call, written, synced, then renamed over `target`. The rename replaces a link at `target`
/// rather than writing through it; the caller refuses a link before calling. On any error the
/// temp file is removed, and a removal that fails is logged by `agent` (its settings id) and the
/// error's kind, never a path (decision record 0047). Returns whether it wrote.
pub(crate) fn replace_if_differs(
    agent: &'static str,
    target: &Path,
    bytes: &[u8],
) -> Result<bool, Error> {
    if holds(target, bytes)? {
        return Ok(false);
    }
    let dir = target
        .parent()
        .ok_or_else(|| Error::io("replace", target, io::ErrorKind::InvalidInput.into()))?;
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let n = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let temp = Temp {
        agent,
        path: Some(dir.join(format!(".{name}.{}-{n}.tmp", std::process::id()))),
    };
    let mut file = create_new(temp.path())?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|err| Error::io("write", temp.path(), err))?;
    drop(file);
    temp.rename_to(target)?;
    Ok(true)
}

/// Whether the file at `target` holds exactly `bytes`: false when it is missing, is not a
/// regular file, or has another size (then it is not opened at all).
fn holds(target: &Path, bytes: &[u8]) -> Result<bool, Error> {
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    match std::fs::symlink_metadata(target) {
        Ok(meta) if meta.is_file() && meta.len() == len => {}
        Ok(_) => return Ok(false),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(Error::io("read", target, err)),
    }
    // A swap between the check above and this open is left open: whoever can write the install
    // folder can change the adapter itself anyway.
    let file = match File::open(target) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(Error::io("read", target, err)),
    };
    if !file
        .metadata()
        .map_err(|err| Error::io("read", target, err))?
        .is_file()
    {
        return Ok(false);
    }
    let mut held = Vec::with_capacity(bytes.len().saturating_add(1));
    file.take(len.saturating_add(1))
        .read_to_end(&mut held)
        .map_err(|err| Error::io("read", target, err))?;
    Ok(held == bytes)
}

/// A new file opened for writing; fails if `path` exists.
fn create_new(path: &Path) -> Result<File, Error> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|err| Error::io("create", path, err))
}

/// A temp file, removed on drop unless it was renamed; `agent` names it in the log.
struct Temp {
    agent: &'static str,
    path: Option<PathBuf>,
}

impl Temp {
    fn path(&self) -> &Path {
        self.path.as_deref().unwrap_or_else(|| Path::new(""))
    }

    /// Renames the temp file over `target`.
    fn rename_to(mut self, target: &Path) -> Result<(), Error> {
        let Some(temp) = self.path.take() else {
            return Ok(());
        };
        if let Err(err) = std::fs::rename(&temp, target) {
            self.path = Some(temp);
            return Err(Error::io("rename", target, err));
        }
        Ok(())
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        if let Some(temp) = self.path.take()
            && let Err(err) = std::fs::remove_file(&temp)
            && err.kind() != io::ErrorKind::NotFound
        {
            let kind = err.kind();
            tracing::warn!(
                agent = self.agent,
                ?kind,
                "a limits temp file was left behind"
            );
        }
    }
}

#[cfg(test)]
pub(crate) mod scratch {
    //! A test's own folder, and the file changes the unit tests make in it.

    use std::path::PathBuf;
    use std::sync::atomic::Ordering;

    use super::NEXT_TEMP;

    /// `rel` (`/`-separated) under `base`, joined one component at a time.
    pub(crate) fn under(base: &std::path::Path, rel: &str) -> PathBuf {
        rel.split('/')
            .fold(base.to_path_buf(), |path, part| path.join(part))
    }

    /// A folder of this test's own under the temp folder, removed on drop.
    pub(crate) struct Scratch(pub(crate) PathBuf);

    impl Scratch {
        pub(crate) fn new(name: &str) -> Scratch {
            let n = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir()
                .join(format!("clauseo-limits-{name}-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        /// `rel` (`/`-separated) under the folder, joined one component at a time, so it is
        /// spelled with the OS's own separator and compares equal to a path the code built.
        pub(crate) fn path(&self, rel: &str) -> PathBuf {
            under(&self.0, rel)
        }

        /// Writes `contents` to `rel`, making its folders.
        pub(crate) fn file(&self, rel: &str, contents: &str) -> PathBuf {
            let path = self.path(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, contents).unwrap();
            path
        }

        /// Makes the folder `rel` and its parents.
        pub(crate) fn dir(&self, rel: &str) -> PathBuf {
            let path = self.path(rel);
            std::fs::create_dir_all(&path).unwrap();
            path
        }

        /// A link at `rel` to `target` (Unix).
        #[cfg(unix)]
        pub(crate) fn link(&self, rel: &str, target: &std::path::Path) -> PathBuf {
            let path = self.path(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(target, &path).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::scratch::Scratch;
    use super::*;

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_missing_file_is_written_and_a_same_one_is_left() {
        let scratch = Scratch::new("missing");
        let target = scratch.0.join("helper.mjs");
        assert!(replace_if_differs("claude-acp", &target, b"one").unwrap());
        assert_eq!(std::fs::read(&target).unwrap(), b"one");
        assert!(!replace_if_differs("claude-acp", &target, b"one").unwrap());
        assert_eq!(names(&scratch.0), ["helper.mjs"]);
    }

    #[test]
    fn a_different_file_is_replaced_with_no_temp_left() {
        let scratch = Scratch::new("differs");
        let target = scratch.0.join("helper.mjs");
        std::fs::write(&target, b"old and longer").unwrap();
        assert!(replace_if_differs("claude-acp", &target, b"new").unwrap());
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(names(&scratch.0), ["helper.mjs"]);
    }

    #[test]
    fn a_folder_at_the_target_is_an_error_with_no_temp_left() {
        let scratch = Scratch::new("folder");
        // A non-empty folder at the target: the rename fails on every OS, and the temp file goes.
        let target = scratch.0.join("helper.mjs");
        std::fs::create_dir_all(target.join("inside")).unwrap();
        let err = replace_if_differs("claude-acp", &target, b"new").unwrap_err();
        assert!(matches!(err, Error::Io { .. }), "{err}");
        assert_eq!(names(&scratch.0), ["helper.mjs"]);
    }

    #[test]
    fn create_dir_all_makes_nested_folders() {
        let scratch = Scratch::new("dirs");
        let run = scratch.0.join("limits").join("run");
        create_dir_all(&run).unwrap();
        create_dir_all(&run).unwrap();
        assert!(run.is_dir());
    }
}
