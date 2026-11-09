//! How a refusal spells the held file it names: as the matter stores it, not as the agent
//! typed it. The guard's network-safe walk keeps the agent's spelling off Windows, and APFS and
//! NTFS find one file by several spellings.

use std::path::{Path, PathBuf};

use clauseo_protocol::MatterPath;
use unicode_normalization::UnicodeNormalization;

use super::{Held, MAX_ENTRIES};

#[cfg(test)]
mod tests;

/// `path`, a held file a command or tool names by path, spelled as the matter lists it in
/// `held` ([`super::held_files`]), so a refusal names the file as the lawyer sees it. The
/// guard's network-safe walk keeps the agent's spelling of each name it finds (it never
/// canonicalises an agent's path off Windows, [`crate::pre_write_guard::Reach::walked`]), and
/// APFS finds `CONTRACT.DOCX` or a decomposed (NFD) `Pétition.docx` as the held `Contract.docx`
/// or composed `Pétition.docx`; NTFS ignores case too. The listed file with this exact path,
/// else the first whose path this system's file system takes for the same ([`disk_key`]), else
/// `path` as given. Only the name shown changes: the write is refused either way.
pub(super) fn as_listed(path: MatterPath, held: &[Held]) -> MatterPath {
    as_listed_by(path, held, disk_key)
}

/// [`as_listed`], names compared by `key`.
pub(super) fn as_listed_by(path: MatterPath, held: &[Held], key: fn(&str) -> String) -> MatterPath {
    if held.iter().any(|file| file.path == path) {
        return path;
    }
    let wanted = key(path.as_str());
    held.iter()
        .find(|file| key(file.path.as_str()) == wanted)
        .map_or(path, |file| file.path.clone())
}

/// A held file found by a tool's path (`Edit`, `Write`, a patch), spelled as stored on the
/// disk, for its refusal. On macOS each of its names as its folder in the matter `folder` lists
/// it ([`stored_spelling`], APFS's rules), else as typed; elsewhere as found: Windows' walk already
/// gives each name as stored, and ext4 has one spelling per name. Never the whole matter listed,
/// never the agent's path canonicalised, nothing counted in the guard's timings.
pub(in crate::pre_write_guard) fn listed_spelling(folder: &Path, path: MatterPath) -> MatterPath {
    if cfg!(target_os = "macos") {
        let root = dunce::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
        stored_spelling(&root, &path, apfs_key).unwrap_or(path)
    } else {
        path
    }
}

/// `path` under the matter's canonical `root` with each name as its folder lists it: the entry
/// with that exact name, else the first equal under `key`. Reads only the folders on the way (the
/// file's parent and the folders above it in the matter), at most [`MAX_ENTRIES`] entries each;
/// `None` when one can't be read or lists no such name. `path` came from the network-safe walk,
/// every link in it followed, so each folder read is a local folder of the matter.
pub(super) fn stored_spelling(
    root: &Path,
    path: &MatterPath,
    key: fn(&str) -> String,
) -> Option<MatterPath> {
    let mut at = root.to_path_buf();
    let mut stored = PathBuf::new();
    for part in path.as_str().split('/') {
        let wanted = key(part);
        let mut equal = None;
        for entry in std::fs::read_dir(&at).ok()?.flatten().take(MAX_ENTRIES) {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if name == part {
                equal = Some(name);
                break;
            }
            if equal.is_none() && key(&name) == wanted {
                equal = Some(name);
            }
        }
        let name = equal?;
        at.push(&name);
        stored.push(name);
    }
    MatterPath::from_relative(&stored)
}

/// `text` as this system's file system tells names apart: APFS ignores case and Unicode form
/// ([`apfs_key`]), NTFS case alone; ext4 neither.
fn disk_key(text: &str) -> String {
    if cfg!(target_os = "macos") {
        apfs_key(text)
    } else if cfg!(windows) {
        text.to_lowercase()
    } else {
        text.to_owned()
    }
}

/// `text` as APFS compares names: lower case and composed (NFC).
pub(super) fn apfs_key(text: &str) -> String {
    text.to_lowercase().nfc().collect()
}
