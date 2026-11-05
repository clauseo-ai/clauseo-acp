//! The one path form tier B compares in ([`verbatim`]).

use std::borrow::Cow;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// `path` in the verbatim `\\?\` form, whatever its length, by `folder::verbatim_wide`'s
/// rules: kept when it has the prefix, `\\.\` turned into `\\?\`, `\\server\share` made
/// `\\?\UNC\server\share`, and `\\?\` put before anything else. So a final path, which is
/// always verbatim, compares with a policy path, which is plain (S15 `PR3b` round 5: `dunce`'s
/// plain form kept a path past `MAX_PATH` verbatim, which then matched nothing). A Windows path
/// written with `/` separators (`C:/x/y`, `//server/share`) has them turned into `\` first:
/// after `\\?\` nothing is parsed, so a `/` would stay inside one name (round 7, VM run 3).
/// Pure text, the same on every OS; a path that isn't Windows-shaped (a Unix path in the tests) is
/// left as it is under the prefix. A name that isn't Unicode is kept as it is, except in a device
/// or share path, which is read lossily.
pub(in crate::windows) fn verbatim(path: &Path) -> PathBuf {
    let path = backslashed(path);
    let bytes = path.as_os_str().as_encoded_bytes();
    if bytes.starts_with(br"\\?\") {
        return path.into_owned();
    }
    if !bytes.starts_with(br"\\") {
        let mut wide = OsString::from(r"\\?\");
        wide.push(path.as_os_str());
        return PathBuf::from(wide);
    }
    let text = path.as_os_str().to_string_lossy();
    PathBuf::from(match text.strip_prefix(r"\\.\") {
        Some(rest) => format!(r"\\?\{rest}"),
        None => format!(r"\\?\UNC\{}", text.get(2..).unwrap_or_default()),
    })
}

/// `path` with `/` turned into `\` when it is a Windows path not yet verbatim: one that starts
/// with a drive letter and a colon, or with two separators (a share or device path). Anything
/// else is kept as it is.
fn backslashed(path: &Path) -> Cow<'_, Path> {
    let bytes = path.as_os_str().as_encoded_bytes();
    let drive = matches!(bytes, [letter, b':', ..] if letter.is_ascii_alphabetic());
    let sep = |b: u8| b == b'/' || b == b'\\';
    let share =
        matches!(bytes, [a, b, rest @ ..] if sep(*a) && sep(*b) && rest.first() != Some(&b'?'));
    if !(drive || share) || !bytes.contains(&b'/') {
        return Cow::Borrowed(path);
    }
    match path.to_str() {
        Some(text) => Cow::Owned(PathBuf::from(text.replace('/', r"\"))),
        // Not Unicode: kept as it is, so it compares only with itself.
        None => Cow::Borrowed(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round 7 (VM run 3): `/` separators become `\` when the prefix is added, so std parses
    /// the parts rather than reading one verbatim name.
    #[test]
    fn forward_slashes_become_backslashes_under_the_prefix() {
        let v = |s: &str| verbatim(Path::new(s)).to_string_lossy().into_owned();
        assert_eq!(v(r"C:/Users/a/.claude"), r"\\?\C:\Users\a\.claude");
        assert_eq!(v(r"C:\Users/a"), r"\\?\C:\Users\a");
        assert_eq!(v(r"//srv/share/x"), r"\\?\UNC\srv\share\x");
        assert_eq!(
            v(r"\\?\C:\a/b"),
            r"\\?\C:\a/b",
            "a verbatim path is kept exactly"
        );
        assert_eq!(v(r"C:\a\b"), r"\\?\C:\a\b");
        assert_eq!(v(r"\\.\C:\a"), r"\\?\C:\a");
        assert_eq!(
            v("/tmp/x"),
            r"\\?\/tmp/x",
            "a Unix path is left under the prefix"
        );
    }
}
