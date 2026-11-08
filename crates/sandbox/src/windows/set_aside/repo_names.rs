//! The names that make a git repository, compared as git and the file systems a matter syncs
//! through would open them: the HFS+ ignorables stripped first (`strip_hfs_ignorables`, shared
//! with the macOS jail, since a matter may sync to a Mac), then the policy's fold (`name_key`)
//! and ASCII case, with the trailing dots and spaces Win32 drops from a path's last part
//! ignored; on Windows also as NTFS matches names ([`same`]).

use std::ffi::OsStr;

use super::scan::same;

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

/// Whether a folder's listing (each name, and whether it is a folder) is a git folder's, as git
/// tests one: a `HEAD` (a file or a link) beside `objects` and `refs` folders (a bare
/// repository), or beside a `commondir` file (a worktree's git folder).
pub(super) fn bare_layout<'a>(listed: impl Iterator<Item = (&'a OsStr, bool)>) -> bool {
    let (mut head, mut objects, mut refs, mut common) = (false, false, false, false);
    for (name, folder) in listed {
        head |= !folder && is_head(name);
        objects |= folder && opens_as(name, "objects");
        refs |= folder && opens_as(name, "refs");
        common |= !folder && opens_as(name, "commondir");
    }
    head && ((objects && refs) || common)
}
