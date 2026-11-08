//! Opening a local drive path for the Authz check one part at a time, never through a reparse
//! point (0049, "The Authz check"): the drive's root by its name, then each part relative to the
//! handle of the folder before it, so a part a pool user swaps for a link after an earlier part
//! was opened is still never followed. No full path is ever opened.

use std::path::Path;

use super::is_local_drive_path;

/// The opens [`open_no_reparse`] makes; `authz.rs` implements it on Windows and the tests
/// fake it.
pub trait Walker {
    /// An open handle.
    type Handle;
    /// Opens the drive root `root` (`X:\`) for `READ_CONTROL` and its attributes.
    fn root(&mut self, root: &str) -> Option<Self::Handle>;
    /// Opens the one part `name` relative to `dir`, never following a reparse point (one met
    /// fails the open, and one at `name` itself is opened, not followed), for its attributes,
    /// and `READ_CONTROL` as well when it is the `last` part. Gives the handle and whether
    /// `name` is a reparse point; `None` when the open fails.
    fn child(&mut self, dir: &Self::Handle, name: &str, last: bool)
    -> Option<(Self::Handle, bool)>;
}

/// The handle of the local drive path `path`, opened part by part; `None` for a path that
/// isn't a local drive path, an open that fails, or any part, the last included, that is a
/// reparse point (a junction, a symbolic link, a mount point).
pub fn open_no_reparse<W: Walker>(path: &Path, walker: &mut W) -> Option<W::Handle> {
    if !is_local_drive_path(path) {
        return None;
    }
    let text = path.to_str()?;
    let parts: Vec<&str> = text
        .get(3..)?
        .split('\\')
        .filter(|part| !part.is_empty())
        .collect();
    let mut handle = walker.root(text.get(..3)?)?;
    for (at, part) in parts.iter().enumerate() {
        let (next, reparse) = walker.child(&handle, part, at + 1 == parts.len())?;
        if reparse {
            return None;
        }
        handle = next;
    }
    Some(handle)
}
