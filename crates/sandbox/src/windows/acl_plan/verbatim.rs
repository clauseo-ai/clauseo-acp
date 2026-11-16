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
pub(crate) fn verbatim(path: &Path) -> PathBuf {
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

/// `path` with this machine's own admin share in its drive form: `\\?\UNC\localhost\c$\x` (or
/// `127.0.0.1`, `.`, any case) is `\\?\C:\x`, as `dunce::canonicalize` names a link or a `gitdir:`
/// line written through the loopback share. Only a one-letter `X$` share on those three
/// servers; any other path, another server's included, comes back as it is. Pure text.
pub(crate) fn local_share_as_drive(path: PathBuf) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path;
    };
    let Some(rest) = text
        .get(..8)
        .filter(|head| head.eq_ignore_ascii_case(r"\\?\UNC\"))
        .and_then(|_| text.get(8..))
    else {
        return path;
    };
    let (server, rest) = rest.split_once('\\').unwrap_or((rest, ""));
    let local = ["localhost", "127.0.0.1", "."];
    if !local.iter().any(|name| server.eq_ignore_ascii_case(name)) {
        return path;
    }
    let (share, rest) = rest.split_once('\\').unwrap_or((rest, ""));
    let mut chars = share.chars();
    match (chars.next(), chars.next(), chars.next()) {
        (Some(drive), Some('$'), None) if drive.is_ascii_alphabetic() => {
            PathBuf::from(format!(r"\\?\{}:\{rest}", drive.to_ascii_uppercase()))
        }
        _ => path,
    }
}

/// `path` in the one form [`crate::policy::under`] compares when either side is verbatim:
/// [`verbatim`], this machine's admin share read as its drive ([`local_share_as_drive`]).
pub(crate) fn compared(path: &Path) -> PathBuf {
    local_share_as_drive(verbatim(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This machine's admin share reads as its drive; another server's, another share, or a
    /// share that is not one letter and `$` stays as it is.
    #[test]
    fn the_local_admin_share_is_read_as_its_drive() {
        for (local, drive) in [
            (r"\\?\UNC\localhost\c$\m\realgit", r"\\?\C:\m\realgit"),
            (r"\\?\unc\LOCALHOST\C$\m", r"\\?\C:\m"),
            (r"\\?\UNC\127.0.0.1\d$\x", r"\\?\D:\x"),
            (r"\\?\UNC\.\c$\m", r"\\?\C:\m"),
            (r"\\?\UNC\localhost\c$", r"\\?\C:\"),
        ] {
            assert_eq!(
                local_share_as_drive(local.into()),
                PathBuf::from(drive),
                "{local}"
            );
        }
        for other in [
            r"\\?\UNC\server\c$\m",
            r"\\?\UNC\localhost.evil\c$\m",
            r"\\?\UNC\localhost\share\m",
            r"\\?\UNC\localhost\cd$\m",
            r"\\?\UNC\localhost\1$\m",
            r"\\?\UNC\[::1]\c$\m",
            r"\\server\c$\m",
            r"\\?\C:\m",
        ] {
            assert_eq!(
                local_share_as_drive(other.into()),
                PathBuf::from(other),
                "{other}"
            );
        }
        assert_eq!(
            compared(Path::new(r"\\localhost\c$\m")),
            PathBuf::from(r"\\?\C:\m")
        );
    }

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
