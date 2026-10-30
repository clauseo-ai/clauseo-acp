//! Test doubles: files in memory, folders and variables from a list, and policy sources that
//! return what they were given.

use crate::files::Files;
use crate::locations::{Env, KnownFolder, PlatformDirs};
use crate::policy::{PolicyRead, PolicySource};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// Files in memory. A path listed in `broken` fails every read with `PermissionDenied`.
#[derive(Debug, Default)]
pub(crate) struct MemFiles {
    pub files: RefCell<BTreeMap<PathBuf, Vec<u8>>>,
    pub broken: Vec<PathBuf>,
    pub fail_writes: bool,
}

impl MemFiles {
    pub(crate) fn with(path: &Path, text: &str) -> MemFiles {
        let files = MemFiles::default();
        files.put(path, text);
        files
    }

    pub(crate) fn put(&self, path: &Path, text: &str) {
        self.files
            .borrow_mut()
            .insert(path.to_owned(), text.as_bytes().to_vec());
    }

    pub(crate) fn text(&self, path: &Path) -> Option<String> {
        self.files
            .borrow()
            .get(path)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
    }
}

impl Files for MemFiles {
    fn read(&self, path: &Path) -> io::Result<Option<Vec<u8>>> {
        if self.broken.iter().any(|broken| broken == path) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
        }
        Ok(self.files.borrow().get(path).cloned())
    }

    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        if self.fail_writes {
            return Err(io::Error::new(io::ErrorKind::StorageFull, "disk full"));
        }
        self.files
            .borrow_mut()
            .insert(path.to_owned(), bytes.to_vec());
        Ok(())
    }
}

/// Folders from fields; `None` where a field is `None`.
#[derive(Debug, Default)]
pub(crate) struct FakeDirs {
    pub home: Option<PathBuf>,
    pub known: Vec<(KnownFolder, PathBuf)>,
}

impl PlatformDirs for FakeDirs {
    fn home(&self) -> Option<PathBuf> {
        self.home.clone()
    }

    fn known_folder(&self, folder: KnownFolder) -> Option<PathBuf> {
        self.known
            .iter()
            .find(|(known, _)| *known == folder)
            .map(|(_, path)| path.clone())
    }
}

/// Variables from a list.
#[derive(Debug, Default)]
pub(crate) struct FakeEnv(pub Vec<(&'static str, String)>);

impl Env for FakeEnv {
    fn var(&self, name: &str) -> Option<OsString> {
        self.0
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| OsString::from(value))
    }
}

/// A policy source that returns what `read` builds, each time it is asked.
pub(crate) struct FakeSource<F> {
    pub name: &'static str,
    pub read: F,
}

impl<F: Fn() -> PolicyRead> PolicySource for FakeSource<F> {
    fn name(&self) -> &str {
        self.name
    }

    fn read(&self) -> PolicyRead {
        (self.read)()
    }
}

/// An absolute path on this OS, from a relative one written with `/`.
pub(crate) fn abs(path: &str) -> PathBuf {
    let root = if cfg!(windows) { r"C:\" } else { "/" };
    path.split('/')
        .fold(PathBuf::from(root), |path, part| path.join(part))
}

/// [`abs`] as a string, for JSON.
pub(crate) fn abs_str(path: &str) -> String {
    abs(path).display().to_string()
}

/// [`abs_str`] escaped for a JSON string.
pub(crate) fn abs_json(path: &str) -> String {
    abs_str(path).replace('\\', "\\\\")
}
