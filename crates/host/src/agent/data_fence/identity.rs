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
//!   still runs; the identity compare with that one folder is off, the host's
//!   [`DataFenceHealth`] says so (logged once as it starts and clears, and
//!   `ProtectionView::data_fence_names_only`), and each check reads it again. Only these
//!   folders, the host's configured data folder resolved and its parents, can bring this
//!   fallback: an identity a request leads to that can't be read refuses that request.

use std::cell::Cell;
use std::path::Path;

use super::health::{Captured, DataFenceHealth, Id};
use crate::network_paths::resolve::{Fs, no_such_name};

/// An identity that could not be read.
struct Unreadable;

/// `path`'s identity on `fs`, `None` for no such name ([`no_such_name`]), not logged.
fn identity_of(fs: &dyn Fs, path: &Path) -> Result<Option<Id>, Unreadable> {
    match fs.identity(path) {
        Ok(id) => Ok(Some(id)),
        Err(error) if no_such_name(&error) => Ok(None),
        Err(_) => Err(Unreadable),
    }
}

/// The identities of `real_data` and its folders above it, read now. The first that can't be
/// read is `failing` (the health logs when that starts and clears, never a path).
fn read_line(fs: &dyn Fs, real_data: &Path) -> Captured {
    let mut captured = Captured {
        real_data: real_data.to_owned(),
        ..Captured::default()
    };
    for (index, folder) in real_data.ancestors().enumerate() {
        match identity_of(fs, folder) {
            Ok(id) => {
                if index == 0 {
                    captured.data = id;
                }
                captured.line.extend(id);
            }
            Err(Unreadable) => {
                if captured.failing.is_none() {
                    captured.failing = Some(folder.to_owned());
                }
            }
        }
    }
    captured.complete = captured.failing.is_none() && captured.data.is_some();
    captured
}

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
    /// (the module's list), through the host's `health`: a complete capture kept from an
    /// earlier check is used once `real_data`'s own identity, read again (one read), is the kept
    /// one; while one of them can't be read, that one is read again (one read); otherwise, or
    /// once it reads, every one is read again. The result is recorded under the sequence number
    /// taken as this capture starts ([`DataFenceHealth::begin`]). Only `real_data` (the host's
    /// data folder, resolved) and its parents are read here, never anything a request names.
    pub(super) fn capture(fs: &'a dyn Fs, real_data: &'a Path, health: &DataFenceHealth) -> Self {
        let sequence = health.begin();
        let fresh = || {
            let fresh = read_line(fs, real_data);
            health.record(sequence, fresh.clone());
            fresh
        };
        let captured = match health.kept(real_data) {
            Some(kept) if kept.complete => match identity_of(fs, real_data) {
                Ok(Some(id)) if kept.data == Some(id) => kept,
                // Recreated (a new identity), gone, or unreadable: every one read again.
                Ok(_) | Err(Unreadable) => fresh(),
            },
            Some(kept) => {
                let retried = kept
                    .failing
                    .as_deref()
                    .map(|folder| identity_of(fs, folder));
                if matches!(retried, Some(Err(Unreadable))) {
                    kept
                } else {
                    fresh()
                }
            }
            None => fresh(),
        };
        let mut me = Self::none(fs);
        me.real_data = Some(real_data);
        me.data = captured.data;
        me.line = captured.line;
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
