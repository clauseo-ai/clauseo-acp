//! One capture's look at the disk, each name looked at once. The walk of every place starts again
//! from the root, so without this the folders the places share (home, the folder of the
//! matters) would be looked at once for each place under them: hundreds of matters made the
//! capture quadratic in the depth. Kept for one capture only, so what it remembers is never older
//! than that capture.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use crate::network_paths::resolve::{Entry, Fs};

/// `fs` with each answer kept by path (an error by its kind, all the walk reads of it).
pub(super) struct Memo<'a> {
    fs: &'a dyn Fs,
    entries: RefCell<HashMap<PathBuf, Result<Entry, io::ErrorKind>>>,
    links: RefCell<HashMap<PathBuf, Result<PathBuf, io::ErrorKind>>>,
    real: RefCell<HashMap<PathBuf, Option<PathBuf>>>,
}

impl<'a> Memo<'a> {
    pub(super) fn new(fs: &'a dyn Fs) -> Self {
        Self {
            fs,
            entries: RefCell::default(),
            links: RefCell::default(),
            real: RefCell::default(),
        }
    }

    /// `real`'s canonical form of `path`, asked once.
    pub(super) fn real(
        &self,
        path: &Path,
        real: &dyn Fn(&Path) -> Option<PathBuf>,
    ) -> Option<PathBuf> {
        if let Some(known) = self.real.borrow().get(path) {
            return known.clone();
        }
        let found = real(path);
        self.real
            .borrow_mut()
            .insert(path.to_path_buf(), found.clone());
        found
    }
}

impl Fs for Memo<'_> {
    fn entry(&self, path: &Path) -> io::Result<Entry> {
        if let Some(known) = self.entries.borrow().get(path) {
            return known.map_err(io::Error::from);
        }
        let found = self.fs.entry(path).map_err(|error| error.kind());
        self.entries.borrow_mut().insert(path.to_path_buf(), found);
        found.map_err(io::Error::from)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        if let Some(known) = self.links.borrow().get(path) {
            return known.clone().map_err(io::Error::from);
        }
        let found = self.fs.read_link(path).map_err(|error| error.kind());
        self.links
            .borrow_mut()
            .insert(path.to_path_buf(), found.clone());
        found.map_err(io::Error::from)
    }

    fn identity(&self, path: &Path) -> io::Result<(u64, u128)> {
        self.fs.identity(path)
    }
}
