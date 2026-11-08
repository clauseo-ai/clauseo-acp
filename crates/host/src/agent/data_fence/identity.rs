//! The data folder found by identity, once a path is proven local: a path is in `<data>` when it
//! or a folder above it has `<data>`'s identity (device or volume and file number), whatever its
//! text. Each identity is scoped to the one decision it serves:
//!
//! - a path's own folders: one that can't be read refuses that path (logged by its kind alone,
//!   no path), never the others. A folder that is by its text one of `<data>`'s parents (the
//!   drive's root, the home) is not read for it: it is not `<data>`, so an unreadable root or
//!   home never refuses every local matter;
//! - a missing name, or one the system rejects as a name (a shell word's `*`, a URL's `https:`:
//!   the walk keeps such a name as written), is no file, so it can't be `<data>`: not an error;
//! - `<data>`'s own identity and its folders' above it (the host's own folders, which a search
//!   root is compared with): when one can't be read, nothing can be compared with it, and
//!   refusing every path or search for it would refuse every local matter. The text compare
//!   still runs; the identity compare with that one folder is off for the decision, and that is
//!   logged (never silent).

use std::cell::Cell;
use std::path::Path;

use crate::network_paths::resolve::{Fs, no_such_name};

/// One file's identity.
type Id = (u64, u128);

/// An identity that could not be read (logged by [`DataIdentity::read`]).
struct Unreadable;

/// `<data>`'s identity and its folders', read once per decision.
pub(super) struct DataIdentity<'a> {
    fs: &'a dyn Fs,
    /// `<data>`'s resolved path, whose parents are known by their text not to be `<data>`.
    real_data: Option<&'a Path>,
    /// `<data>`'s own; `None` when `<data>` is missing (nothing is in it by identity), when it
    /// can't be read (logged), or when it is not captured (a data folder on the network, never
    /// touched).
    data: Option<Id>,
    /// The identities of `<data>` and its folders above it that could be read, for a search
    /// root.
    line: Vec<Id>,
    /// Whether an identity this decision needed could not be read (logged once).
    unreadable: Cell<bool>,
}

impl<'a> DataIdentity<'a> {
    /// `real_data`'s identity and its folders' on `fs`, each kept for the decisions it serves
    /// (the module's list).
    pub(super) fn capture(fs: &'a dyn Fs, real_data: &'a Path) -> Self {
        let mut me = Self::none(fs);
        me.real_data = Some(real_data);
        me.data = me.read(real_data).unwrap_or_else(|Unreadable| {
            tracing::warn!(
                "the data folder's identity could not be read; its paths are compared by text"
            );
            None
        });
        let mut line_unreadable = false;
        for folder in real_data.ancestors().skip(1) {
            match me.read(folder) {
                Ok(id) => me.line.extend(id),
                Err(Unreadable) => line_unreadable = true,
            }
        }
        if line_unreadable {
            tracing::warn!(
                "a data folder's parent's identity could not be read; searches are compared with \
                 it by text"
            );
        }
        me.line.extend(me.data);
        // The host's own folders' read errors are logged above; a path's are its own.
        me.unreadable.set(false);
        me
    }

    /// No identity to compare (a data folder on the network: a local path can't be in it).
    pub(super) fn none(fs: &'a dyn Fs) -> Self {
        Self {
            fs,
            real_data: None,
            data: None,
            line: Vec::new(),
            unreadable: Cell::new(false),
        }
    }

    /// Whether `path` (local, walked) or a folder above it is `<data>` by identity. A folder of
    /// it whose identity can't be read counts as `<data>`: that path is refused (logged).
    pub(super) fn holds(&self, path: &Path) -> bool {
        let Some(data) = self.data else {
            return false;
        };
        let a_parent_of_data = |folder: &Path| {
            self.real_data
                .is_some_and(|real| real != folder && real.starts_with(folder))
        };
        path.ancestors()
            .filter(|folder| !a_parent_of_data(folder))
            .any(|folder| match self.read(folder) {
                Ok(id) => id == Some(data),
                Err(Unreadable) => true,
            })
    }

    /// Whether the search root `root` (local, walked) is `<data>` or a folder above it by
    /// identity, of those whose identity could be read. A root whose own identity can't be read
    /// counts as one: that root is refused (logged).
    pub(super) fn is_on_line(&self, root: &Path) -> bool {
        if self.line.is_empty() {
            return false;
        }
        match self.read(root) {
            Ok(id) => id.is_some_and(|id| self.line.contains(&id)),
            Err(Unreadable) => true,
        }
    }

    /// `path`'s identity: `None` for no such name ([`no_such_name`]: missing, or a name the
    /// system rejects), [`Unreadable`] when it can't be read (the first time per decision
    /// logged by its kind, no path).
    fn read(&self, path: &Path) -> Result<Option<Id>, Unreadable> {
        match self.fs.identity(path) {
            Ok(id) => Ok(Some(id)),
            Err(error) if no_such_name(&error) => Ok(None),
            Err(error) => {
                if !self.unreadable.replace(true) {
                    tracing::warn!(
                        kind = ?error.kind(),
                        "a file identity could not be read; the path is refused"
                    );
                }
                Err(Unreadable)
            }
        }
    }
}
