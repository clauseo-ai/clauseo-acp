//! Names and paths as macOS resolves them, for [`under`](super::under) and the policy's dedup
//! sets: one key per name, so two spellings APFS opens as one entry share it.

use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use caseless::Caseless;
use unicode_normalization::UnicodeNormalization;

/// Whether `c` is a code point case-insensitive HFS+ ignores when it compares names (Apple
/// TN1150's ignorable list): U+200C to U+200F, U+202A to U+202E, U+206A to U+206F, U+FEFF.
pub fn hfs_ignorable(c: char) -> bool {
    matches!(
        c,
        '\u{200C}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{206A}'..='\u{206F}' | '\u{FEFF}'
    )
}

/// `name` without its [`hfs_ignorable`] code points, as HFS+ compares it: `.g`, U+200C, `it`
/// opens `.git` there. Use it before the fold wherever names are compared; borrowed when
/// `name` holds none.
pub fn strip_hfs_ignorables(name: &str) -> Cow<'_, str> {
    if name.chars().any(hfs_ignorable) {
        Cow::Owned(name.chars().filter(|&c| !hfs_ignorable(c)).collect())
    } else {
        Cow::Borrowed(name)
    }
}

/// Whether `name` is the lowercase ASCII name `want` as some file system may open it: without
/// its [`hfs_ignorable`] code points, then by [`name_key`]'s caseless fold on every OS (NTFS
/// opens `objectſ`, U+017F, as `objects`; folding more than one file system does only lists
/// more). For the launch walk's git names; a name that is not UTF-8 folds ASCII case only.
pub(crate) fn opens_as(name: &OsStr, want: &str) -> bool {
    debug_assert!(!want.bytes().any(|b| b.is_ascii_uppercase()), "{want}");
    match name.to_str() {
        Some(text) => caseless_key(&strip_hfs_ignorables(text)) == want,
        None => name.eq_ignore_ascii_case(want),
    }
}

/// [`name_key`]'s fold of UTF-8 `text`: NFD of the full case fold of NFD.
fn caseless_key(text: &str) -> String {
    if text.is_ascii() {
        return text.to_ascii_lowercase();
    }
    let nfd: String = text.nfd().collect();
    let folded: String = nfd.chars().default_case_fold().collect();
    // The second NFD only when the fold changed something (most scripts have no case).
    if folded == nfd {
        nfd
    } else {
        folded.nfd().collect()
    }
}

/// The key of `name`. On macOS, Unicode canonical caseless matching: NFD of the full case fold
/// (CaseFolding.txt statuses C and F, not Turkic) of NFD, the folding a case-insensitive APFS
/// volume documents. So `क़` as U+0958 and as U+0915 U+093C, `É` and `é`, `ß` and `ss` share a
/// key. A name that is not UTF-8 folds ASCII case only. Elsewhere the name itself: NTFS and Linux
/// file systems keep NFC and NFD names as two files.
pub(crate) fn name_key(name: &OsStr) -> Cow<'_, OsStr> {
    if !cfg!(target_os = "macos") {
        return Cow::Borrowed(name);
    }
    match name.to_str() {
        Some(text) if text.is_ascii() => {
            if text.bytes().any(|b| b.is_ascii_uppercase()) {
                Cow::Owned(OsString::from(text.to_ascii_lowercase()))
            } else {
                Cow::Borrowed(name)
            }
        }
        Some(text) => Cow::Owned(OsString::from(caseless_key(text))),
        None => Cow::Owned(name.to_ascii_lowercase()),
    }
}

/// The key of `path`: each component's [`name_key`], joined. Two paths are [`same`](super::same)
/// exactly when their keys are equal, so a set of keys answers membership in one lookup.
pub(crate) fn path_key(path: &Path) -> PathBuf {
    if !cfg!(target_os = "macos") {
        return path.to_owned();
    }
    path.components()
        .map(|c| name_key(c.as_os_str()))
        .fold(PathBuf::new(), |mut key, name| {
            key.push(name);
            key
        })
}

/// [`path_key`] of many paths that share folders, each folder's key made once.
#[derive(Default)]
pub(super) struct Keys {
    folders: HashMap<PathBuf, PathBuf>,
}

impl Keys {
    /// [`path_key`] of `path`.
    pub(super) fn of(&mut self, path: &Path) -> PathBuf {
        if !cfg!(target_os = "macos") {
            return path.to_owned();
        }
        let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
            return path_key(path);
        };
        let mut key = if let Some(key) = self.folders.get(folder) {
            key.clone()
        } else {
            let key = path_key(folder);
            self.folders.insert(folder.to_owned(), key.clone());
            key
        };
        key.push(name_key(name));
        key
    }
}

/// Whether `a` and `b` name the same entry: equal [`name_key`]s. On macOS two ASCII names
/// are compared without ASCII case alone, which is what their keys would do.
pub(super) fn same_name(a: &OsStr, b: &OsStr) -> bool {
    if cfg!(target_os = "macos") && a.is_ascii() && b.is_ascii() {
        return a.eq_ignore_ascii_case(b);
    }
    name_key(a) == name_key(b)
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use std::ffi::OsStr;

    use super::{hfs_ignorable, opens_as, strip_hfs_ignorables};

    #[test]
    fn a_name_with_an_ignorable_strips_and_folds_to_git() {
        let stripped = strip_hfs_ignorables(".g\u{200C}it");
        assert_eq!(stripped, ".git");
        assert!(stripped.eq_ignore_ascii_case(".GIT"));
        assert_eq!(
            strip_hfs_ignorables("\u{FEFF}.G\u{202E}I\u{206F}T\u{200F}"),
            ".GIT"
        );
    }

    #[test]
    fn each_range_strips_at_its_edges_and_not_beside_them() {
        for c in [
            '\u{200C}', '\u{200F}', '\u{202A}', '\u{202E}', '\u{206A}', '\u{206F}', '\u{FEFF}',
        ] {
            assert!(hfs_ignorable(c), "{c:?}");
            assert_eq!(strip_hfs_ignorables(&format!("a{c}b")), "ab", "{c:?}");
        }
        for c in [
            '\u{200B}', '\u{2010}', '\u{2029}', '\u{202F}', '\u{2069}', '\u{2070}', '\u{FEFE}',
            '\u{FF00}',
        ] {
            assert!(!hfs_ignorable(c), "{c:?}");
            let name = format!("a{c}b");
            assert_eq!(strip_hfs_ignorables(&name), name, "{c:?}");
        }
    }

    #[test]
    fn ordinary_names_are_borrowed_unchanged() {
        for name in [".git", "brief.txt", "नमस्ते.md", "قرارداد.pdf", "É", ""] {
            assert!(
                matches!(strip_hfs_ignorables(name), Cow::Borrowed(n) if n == name),
                "{name}"
            );
        }
    }

    /// The walk's git names on every OS: ignorables stripped, then the caseless fold, so the
    /// long s (U+017F) and the Kelvin sign (U+212A) fold to `s` and `k` as NTFS or APFS may.
    #[test]
    fn opens_as_strips_and_folds_on_every_os() {
        for (name, want) in [
            ("HEAD", "head"),
            ("He\u{200D}aD", "head"),
            ("objects", "objects"),
            ("OBJECT\u{17F}", "objects"),
            ("obj\u{FEFF}ectſ", "objects"),
            ("\u{212A}eep", "keep"),
            ("commonDir", "commondir"),
        ] {
            assert!(opens_as(OsStr::new(name), want), "{name:?}");
        }
        for (name, want) in [
            ("HEADS", "head"),
            ("object", "objects"),
            ("head\u{200B}", "head"),
        ] {
            assert!(!opens_as(OsStr::new(name), want), "{name:?}");
        }
    }
}
