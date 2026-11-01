//! What `pick_folder` returns: the folder's ref and how the window shows it ([`PickedFolder`]).
//! The display is home-relative, so the form shows `~/Documents/Acme` and not the lawyer's
//! user name in full ([`display`]).

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
    /// The folder as `display` writes it.
    pub(crate) display: String,
}

/// The folder as the window shows it. A drive root (or `/`) is shown as it is. A folder under
/// `home` is `~` and then each part of the rest of the path, joined with `sep` (`~/Documents/Acme`
/// with `/`, `~\Documents\Acme` with `\`); `home` itself is `~`. Any other folder, or every folder
/// when there is no `home`, is shown in full.
pub(crate) fn display(path: &Path, home: Option<&Path>, sep: char) -> String {
    if path.parent().is_none() {
        return path.display().to_string();
    }
    let Some(rest) = home.and_then(|home| path.strip_prefix(home).ok()) else {
        return path.display().to_string();
    };
    let mut shown = String::from("~");
    for part in rest.components() {
        shown.push(sep);
        shown.push_str(&part.as_os_str().to_string_lossy());
    }
    shown
}
