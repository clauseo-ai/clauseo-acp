//! Paths as strings, joined and split by the rules of the OS the import is reading for. The
//! `Path` methods follow the host's rules, so they are not used on a path of the other OS.

use super::{Env, Os};

/// The separator between the parts of a path on `os`.
pub(crate) fn sep(os: Os) -> char {
    match os {
        Os::Mac => '/',
        Os::Windows => '\\',
    }
}

/// `base` and `rel` joined with the OS's separator. `rel` may use `/`; on Windows it is
/// rewritten to `\`.
pub(crate) fn join(os: Os, base: &str, rel: &str) -> String {
    let sep = sep(os);
    let rel = match os {
        Os::Mac => rel.to_owned(),
        Os::Windows => rel.replace('/', "\\"),
    };
    let base = base.trim_end_matches(sep);
    format!("{base}{sep}{rel}")
}

/// Whether `p` is absolute on `os`: rooted on Mac; a drive letter and a separator, or a UNC
/// share, on Windows.
pub(crate) fn is_absolute(os: Os, p: &str) -> bool {
    match os {
        Os::Mac => p.starts_with('/'),
        Os::Windows => {
            let bytes = p.as_bytes();
            p.starts_with("\\\\")
                || (bytes.len() >= 3
                    && bytes.first().is_some_and(u8::is_ascii_alphabetic)
                    && bytes.get(1) == Some(&b':')
                    && matches!(bytes.get(2), Some(b'\\' | b'/')))
        }
    }
}

/// Whether `p` starts with two separators: a UNC share, a network path, or a device path (`\\?\`,
/// `\\.\`). Refused on both OSes, since a probe of one can reach a server.
pub(crate) fn is_network(p: &str) -> bool {
    p.starts_with(r"\\") || p.starts_with("//")
}

/// Whether `p` is absolute on `os` and not a network path: a folder the search may use.
pub(crate) fn is_local_absolute(os: Os, p: &str) -> bool {
    is_absolute(os, p) && !is_network(p)
}

/// The last part of `p` on `os`.
pub(crate) fn file_name(os: Os, p: &str) -> &str {
    let last = match os {
        Os::Mac => p.rsplit('/').next(),
        Os::Windows => p.rsplit(['\\', '/']).next(),
    };
    last.unwrap_or(p)
}

/// The variable `name`, when it is set and not empty.
pub(crate) fn var(env: &Env<'_>, name: &str) -> Option<String> {
    (env.vars)(name).filter(|value| !value.is_empty())
}

/// The home folder as a string.
pub(crate) fn home(env: &Env<'_>) -> String {
    env.home.to_string_lossy().into_owned()
}
