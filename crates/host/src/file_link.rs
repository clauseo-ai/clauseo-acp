//! A prompt's file as the agent sees it: refused unless it lies in the matter folder, then
//! sent as a `file://` URI.

use std::fmt::Write as _;
use std::path::Path;

/// Whether `path` lies in `folder` once both resolve (symlinks and `..` followed). A path
/// that does not resolve is outside.
pub(crate) fn inside(path: &Path, folder: &Path) -> bool {
    match (dunce::canonicalize(path), dunce::canonicalize(folder)) {
        (Ok(path), Ok(folder)) => path.starts_with(folder),
        _ => false,
    }
}

/// The `file://` URI of the absolute `path` (RFC 8089), percent-encoded by RFC 3986.
pub(crate) fn file_uri(path: &Path) -> String {
    uri_of(&path.to_string_lossy(), cfg!(windows))
}

/// The URI of `path`'s text. Unreserved bytes and `/` stay; every other byte of its UTF-8 is
/// `%XX`. With `windows`, `\` is a separator and a drive path `C:\x` gives `file:///C:/x`.
fn uri_of(path: &str, windows: bool) -> String {
    let path = if windows {
        path.replace('\\', "/")
    } else {
        path.to_owned()
    };
    let drive = windows
        && path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && path.as_bytes().get(1) == Some(&b':');
    let mut uri = String::from(if drive { "file:///" } else { "file://" });
    for (at, byte) in path.bytes().enumerate() {
        let kept = byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/')
            || (drive && at == 1);
        if kept {
            uri.push(char::from(byte));
        } else {
            // Writing to a String never fails.
            let _infallible = write!(uri, "%{byte:02X}");
        }
    }
    uri
}

#[cfg(test)]
mod tests;
