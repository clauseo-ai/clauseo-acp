//! What `pick_folder` returns: the folder's ref and how the window shows it ([`PickedFolder`]).
//! The display is relative to the home folder, so the form shows `Documents/Acme` and not the
//! lawyer's user name in full ([`display`]).

#[cfg(test)]
mod tests;

use std::path::Path;

use clauseo_protocol::FolderRefId;
use serde::Serialize;

/// A folder the lawyer picked: the ref `CreateMatter` takes, and the path to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct PickedFolder {
    /// The ref the host issued for the folder.
    pub(crate) folder: FolderRefId,
    /// The path to show: under the home folder, the parts below home joined with the OS
    /// separator (`Documents/Acme`, `Documents\Acme` on Windows); otherwise the full path
    /// (`/Users/rmehra`, `/Volumes/Work/Acme`, `C:\`, `D:\Cases`).
    pub(crate) display: String,
}

/// The folder as the window shows it. A folder under `home` is each part of the rest of its
/// path, joined with `sep` and with no `~` or leading separator (`Documents/Acme` with `/`,
/// `Documents\Acme` with `\`). `home` itself, and any folder outside it, is shown in full, as is
/// every folder when there is no `home`. A `home` that is a root (`/`, or a drive root with no
/// parent) counts as no home.
pub(crate) fn display(path: &Path, home: Option<&Path>, sep: char) -> String {
    let home = home.filter(|h| h.parent().is_some());
    let Some(rest) = home.and_then(|home| path.strip_prefix(home).ok()) else {
        return path.display().to_string();
    };
    if rest.components().next().is_none() {
        return path.display().to_string();
    }
    let mut shown = String::new();
    for (index, part) in rest.components().enumerate() {
        if index > 0 {
            shown.push(sep);
        }
        shown.push_str(&part.as_os_str().to_string_lossy());
    }
    shown
}
