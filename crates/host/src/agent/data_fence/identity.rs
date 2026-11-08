//! The data folder found by identity, once a path is proven local: a path is in `<data>` when it
//! or a folder above it has `<data>`'s identity (device or volume and file number), whatever its
//! text. Fail-closed: an identity that can't be read (any error but a missing name, which can't
//! be the folder) makes the decision a refusal, logged by its kind alone, never the backstop
//! silently off.

use std::cell::Cell;
use std::io;
use std::path::Path;

use crate::network_paths::resolve::Fs;

/// One file's identity.
type Id = (u64, u128);

/// An identity that could not be read (logged by [`DataIdentity::read`]).
struct Unreadable;

/// `<data>`'s identity and its folders', read once per decision.
pub(super) struct DataIdentity<'a> {
    fs: &'a dyn Fs,
    /// `<data>`'s own; `None` when `<data>` is missing (nothing is in it by identity) or not
    /// captured (a data folder on the network, never touched).
    data: Option<Id>,
    /// `<data>`'s and every folder's above it, for a search root that holds it.
    line: Vec<Id>,
    /// Whether an identity this decision needed could not be read.
    unreadable: Cell<bool>,
}

impl<'a> DataIdentity<'a> {
    /// `real_data`'s identity and its folders' on `fs`. `None` when one can't be read (logged):
    /// the decision is refused.
    pub(super) fn capture(fs: &'a dyn Fs, real_data: &Path) -> Option<Self> {
        let me = Self::none(fs);
        let data = me.read(real_data).ok()?;
        let mut line = Vec::new();
        for folder in real_data.ancestors() {
            line.extend(me.read(folder).ok()?);
        }
        Some(Self { data, line, ..me })
    }

    /// No identity to compare (a data folder on the network: a local path can't be in it).
    pub(super) fn none(fs: &'a dyn Fs) -> Self {
        Self {
            fs,
            data: None,
            line: Vec::new(),
            unreadable: Cell::new(false),
        }
    }

    /// Whether `path` (local, walked) or a folder above it is `<data>` by identity. An identity
    /// that can't be read counts as in it (refused, logged).
    pub(super) fn holds(&self, path: &Path) -> bool {
        let Some(data) = self.data else {
            return false;
        };
        path.ancestors().any(|folder| match self.read(folder) {
            Ok(id) => id == Some(data),
            Err(Unreadable) => true,
        })
    }

    /// Whether the search root `root` (local, walked) is `<data>` or a folder above it by
    /// identity. An identity that can't be read counts as one (refused, logged).
    pub(super) fn is_on_line(&self, root: &Path) -> bool {
        if self.data.is_none() {
            return false;
        }
        match self.read(root) {
            Ok(id) => id.is_some_and(|id| self.line.contains(&id)),
            Err(Unreadable) => true,
        }
    }

    /// `path`'s identity: `None` for a missing name, [`Unreadable`] when it can't be read (the
    /// first time per decision logged by its kind, no path).
    fn read(&self, path: &Path) -> Result<Option<Id>, Unreadable> {
        match self.fs.identity(path) {
            Ok(id) => Ok(Some(id)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => {
                if !self.unreadable.replace(true) {
                    tracing::warn!(
                        kind = ?error.kind(),
                        "a file identity could not be read; the request is refused"
                    );
                }
                Err(Unreadable)
            }
        }
    }
}
