//! Which names of a listed folder an owner file may point at, with no file-system call
//! ([`OwnersBeside`]), so a caller that lists the folder asks [`super::held_open_beside`] only
//! about those.

use unicode_normalization::{UnicodeNormalization, is_nfc, is_nfd};

use super::owner_names;

/// The `~$` entries of one listed folder, ready to tell which of its names one of them may show
/// held open ([`Self::may_hold`]).
#[derive(Debug, Clone)]
pub struct OwnersBeside {
    /// Each entry's [`loose`] form.
    keys: Vec<String>,
}

impl OwnersBeside {
    /// From the folder's entries whose names start with `~$`, of any kind (a folder or a symlink
    /// named `~$…` answers the owner-file lookup's `symlink_metadata` too). Other names are
    /// ignored.
    pub fn new(entries: &[String]) -> Self {
        let keys = entries
            .iter()
            .filter(|entry| entry.starts_with("~$"))
            .map(|entry| loose(entry))
            .collect();
        Self { keys }
    }

    /// Whether one of the owner files may show the file `name` of the same folder held open:
    /// `false` only when [`super::held_open_beside`], given this folder's owner files, finds no
    /// owner file for it, neither by its stats (one per owner name, compared as the file system
    /// compares names) nor by the lookup in another Unicode form. (On Windows it may still find
    /// the file held by a sharing violation; that is the caller's to probe.)
    ///
    /// Each comparison the lookup makes (bytes; NTFS's and APFS's case-insensitive, and APFS's
    /// form-insensitive, lookups; the composed, case-folded compare) equates only names whose
    /// [`loose`] forms are equal, so a name is kept when the loose form of one of its owner names
    /// (from the name as written, composed and decomposed: [`owner_names`] of each) is an
    /// entry's. Before that, a cheap filter: every such owner name ends with the name's trailing
    /// ASCII run less its first two characters (normalising and changing case leave ASCII
    /// characters at the end, and an owner name drops at most two characters from the front), so
    /// an entry whose loose form does not end with it is skipped. On Windows a name with `~` (an
    /// owner name the lookup might find by another entry's 8.3 short name) or a trailing `.` or
    /// space (which Windows trims from a looked-up name) is always kept.
    pub fn may_hold(&self, name: &str) -> bool {
        if self.keys.is_empty() {
            return false;
        }
        if cfg!(windows) && (name.contains('~') || name.ends_with(['.', ' '])) {
            return true;
        }
        let tail = ascii_tail(name);
        let tail = tail.get(2..).unwrap_or_default().to_ascii_lowercase();
        if !self.keys.iter().any(|key| key.ends_with(&tail)) {
            return false;
        }
        let composed: Option<String> = (!is_nfc(name)).then(|| name.nfc().collect());
        let decomposed: Option<String> = (!is_nfd(name)).then(|| name.nfd().collect());
        [Some(name), composed.as_deref(), decomposed.as_deref()]
            .into_iter()
            .flatten()
            .flat_map(owner_names)
            .any(|owner| self.keys.contains(&loose(&owner)))
    }
}

/// The longest run of ASCII characters at the end of `name`.
fn ascii_tail(name: &str) -> &str {
    let start = name
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii())
        .last()
        .map_or(name.len(), |(at, _)| at);
    name.get(start..).unwrap_or_default()
}

/// A name as loosely as any lookup of an owner file compares it: composed (NFC), lower case of
/// upper case of lower case, character by character (so `ß`, `ẞ` and `ss`, the three sigmas,
/// `ſ` and `s`, the Kelvin sign and `k` meet), composed again. An ASCII name is just lower case.
pub(super) fn loose(name: &str) -> String {
    if name.is_ascii() {
        return name.to_ascii_lowercase();
    }
    name.nfc()
        .flat_map(char::to_lowercase)
        .flat_map(char::to_uppercase)
        .flat_map(char::to_lowercase)
        .nfc()
        .collect()
}

#[cfg(test)]
mod tests;
