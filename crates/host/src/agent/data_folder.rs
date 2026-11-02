//! The launch refusal of a matter folder in Clauseo's own data folder, for every agent. iris's
//! #155 refuses the whole data tree at import; this launch check is the second line, for matters
//! imported before #155 and for paths that change after import (a link pointed elsewhere). It
//! refuses a matter folder that is the data folder, holds it, or overlaps `<data>/agents`, so
//! no agent can read a login or plant what Codex loads from any agent's home, a sibling's
//! included. The gap, named: a matter elsewhere inside the data folder still launches; it holds
//! no logins.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf, Prefix};

use clauseo_protocol::ErrorCode;

use crate::error::Error;

/// Refuses the matter folder `stored` when it is the data folder `data`, holds it, or is, holds
/// or sits inside `<data>/agents`, with `matter.folder_refused`. The paths go to the log only.
/// It stats the file system: run it off the actor.
pub(crate) fn refuse_data_folder(stored: &Path, data: &Path) -> Result<(), Error> {
    let data = resolved(data);
    let agents = data.join("agents");
    if !(holds(stored, &data) || overlaps_tree(stored, &agents)) {
        return Ok(());
    }
    tracing::warn!(
        folder = %stored.display(),
        data = %data.display(),
        "an agent was refused: the matter folder overlaps Clauseo's data folder"
    );
    Err(Error::refused(
        ErrorCode::MatterFolderRefused,
        "An agent can't work in this matter: its folder holds Clauseo's own data, where agents \
         keep their sign-in.",
    ))
}

/// Whether `folder` is `tree`, holds it, or sits inside it, as matters' refused-list check
/// compares folders (`crates/matters/src/refuse.rs`, `check_folder`).
// replaced by clauseo_matters::overlaps_tree once #155 lands
pub(crate) fn overlaps_tree(folder: &Path, tree: &Path) -> bool {
    holds(folder, tree) || holds(tree, folder)
}

/// Whether `inner` is `outer` or sits inside it: a key of `inner` starts with a key of `outer`.
fn holds(outer: &Path, inner: &Path) -> bool {
    let (outer, inner) = (spellings(&resolved(outer)), spellings(&resolved(inner)));
    inner
        .iter()
        .any(|inner| outer.iter().any(|outer| inner.starts_with(outer)))
}

/// `path` resolved through links, or as written when it does not resolve.
fn resolved(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

/// The comparison keys of `path`: its components, case-folded on macOS and Windows, with the
/// Windows verbatim prefix forms made plain. On macOS a key starting with
/// `/System/Volumes/Data` comes twice: as it is and without those three parts. A copy of
/// matters' private `spellings`.
fn spellings(path: &Path) -> Vec<Vec<OsString>> {
    let key: Vec<OsString> = path.components().map(component_key).collect();
    let mut keys = Vec::with_capacity(2);
    #[cfg(target_os = "macos")]
    if let [_root, system, volumes, data, ..] = key.as_slice()
        && system == "system"
        && volumes == "volumes"
        && data == "data"
    {
        let mut stripped = key.clone();
        stripped.drain(1..4);
        keys.push(stripped);
    }
    keys.push(key);
    keys
}

/// One component's comparison key.
fn component_key(component: Component<'_>) -> OsString {
    match component {
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                OsString::from(format!("{}:", char::from(letter.to_ascii_lowercase())))
            }
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                let mut text = OsString::from(r"\\");
                text.push(server);
                text.push(r"\");
                text.push(share);
                fold(text)
            }
            Prefix::Verbatim(_) | Prefix::DeviceNS(_) => fold(prefix.as_os_str().to_owned()),
        },
        Component::RootDir => OsString::from("/"),
        Component::CurDir | Component::ParentDir | Component::Normal(_) => {
            fold(component.as_os_str().to_owned())
        }
    }
}

/// `name` lowercased on the case-insensitive file systems (macOS, Windows) when it is UTF-8;
/// as it is elsewhere.
fn fold(name: OsString) -> OsString {
    if cfg!(any(target_os = "macos", windows)) {
        match name.to_str() {
            Some(text) => OsString::from(text.to_lowercase()),
            None => name,
        }
    } else {
        name
    }
}

#[cfg(test)]
mod tests;
