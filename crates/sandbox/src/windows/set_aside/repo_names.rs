//! The names that make a git repository, compared as git and the file systems a matter syncs
//! through would open them: the HFS+ ignorables stripped first (`strip_hfs_ignorables`, shared
//! with the macOS jail, since a matter may sync to a Mac), then the policy's fold (`name_key`)
//! and ASCII case, with the trailing dots and spaces Win32 drops from a path's last part
//! ignored; on Windows also as NTFS matches names ([`same`]).

use std::ffi::OsStr;

use super::scan::{Kind, same};

/// The name git opens for its folder, or for the file that names it.
const GIT: &str = ".git";
/// The file a git folder or a bare repository holds its current branch in.
const HEAD: &str = "HEAD";

/// Whether `name` is `want` (ASCII) as git would open it; see the module's docs.
pub(super) fn opens_as(name: &OsStr, want: &str) -> bool {
    let text = name.to_string_lossy();
    let stripped = crate::strip_hfs_ignorables(&text);
    let trimmed = OsStr::new(stripped.trim_end_matches(['.', ' ']));
    crate::policy::name_key(trimmed).eq_ignore_ascii_case(want) || same(name, OsStr::new(want))
}

/// Whether `name` opens as `.git`.
pub(crate) fn is_git(name: &OsStr) -> bool {
    opens_as(name, GIT)
}

/// Whether `name` opens as `HEAD`.
pub(super) fn is_head(name: &OsStr) -> bool {
    opens_as(name, HEAD)
}

/// Whether `name` opens as `.git` only once stripped or with its trailing dots or spaces
/// dropped: a name the scan's own `.git` probe doesn't open (NTFS keeps `.g<U+200C>it` apart),
/// so the scan takes it from the listing.
pub(super) fn git_lookalike(name: &OsStr) -> bool {
    is_git(name) && !name.eq_ignore_ascii_case(GIT) && !same(name, OsStr::new(GIT))
}

/// What git reads from the folder a `commondir` names, as well as its git folder: its
/// `config` and `hooks` would run code as the lawyer, the rest make it a repository.
pub(super) const COMMON_NAMES: [&str; 6] =
    ["config", "hooks", "objects", "refs", "packed-refs", "info"];

/// Whether `name` opens as `commondir`.
pub(super) fn is_commondir(name: &OsStr) -> bool {
    opens_as(name, "commondir")
}

/// Whether a folder's listing (each name, and what the listing says it is) is a git folder's,
/// as git tests one: a `HEAD` (a file or a link) beside `objects` and `refs` (folders, or links
/// git would follow to folders) for a bare repository, or beside a `commondir` file for a
/// worktree's git folder.
pub(super) fn bare_layout<'a>(listed: impl Iterator<Item = (&'a OsStr, Kind)>) -> bool {
    let (mut head, mut objects, mut refs, mut common) = (false, false, false, false);
    for (name, kind) in listed {
        let folder = kind != Kind::Other;
        head |= kind != Kind::Folder && is_head(name);
        objects |= folder && opens_as(name, "objects");
        refs |= folder && opens_as(name, "refs");
        common |= kind != Kind::Folder && is_commondir(name);
    }
    head && ((objects && refs) || common)
}
