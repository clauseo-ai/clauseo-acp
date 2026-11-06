//! The clean of Claude's `projects` folder in a matter's state folder (`super::clean`), walked
//! by handle: the session may swap a project folder for a link to elsewhere between the clean's
//! look and its removal, so no path below `projects` is resolved again by name.
//!
//! - Unix: `projects` and each project folder are opened `O_DIRECTORY | O_NOFOLLOW` (a link
//!   there fails the open and is removed as itself), each entry's kind is read with `fstatat`
//!   and `AT_SYMLINK_NOFOLLOW` on the folder's handle, a file or link is removed with
//!   `unlinkat` on that handle, and a folder to go is removed with `remove_dir_all` (which never
//!   follows a link inside it) on its name under the handle's folder.
//! - Windows: each folder is opened with `FILE_FLAG_OPEN_REPARSE_POINT` and refused (removed as
//!   itself) when it is a reparse point, then listed by name; the swap between that check and
//!   the listing is the named residual.
//!
//! What stays in each project: its `*.jsonl` transcripts with one name each; with `live` (a
//! session of the agent runs in the matter) also its session folders (tool results,
//! subagents), never `memory`.

use std::io;
use std::path::Path;

/// Whether a project's entry `name`, a folder, stays: with `live`, a session folder.
fn folder_stays(name: &[u8], live: bool) -> bool {
    live && name != b"memory"
}

/// Whether a project's entry `name`, a plain file, stays: a transcript.
fn file_stays(name: &[u8]) -> bool {
    name.ends_with(b".jsonl")
}

#[cfg(unix)]
pub(super) fn clean(projects: &Path, live: bool) -> io::Result<()> {
    use nix::dir::Dir;
    use nix::errno::Errno;
    use nix::fcntl::{AtFlags, OFlag};
    use nix::sys::stat::{Mode, SFlag, fstatat};
    use nix::unistd::{UnlinkatFlags, unlinkat};
    use std::ffi::{CString, OsStr};
    use std::os::unix::ffi::OsStrExt as _;

    let flags = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
    let names = |dir: &mut Dir| -> Vec<CString> {
        dir.iter()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_owned())
            .filter(|name| name.as_bytes() != b"." && name.as_bytes() != b"..")
            .collect()
    };
    let kind = |dir: &Dir, name: &CString| -> io::Result<SFlag> {
        let stat = fstatat(dir, name.as_c_str(), AtFlags::AT_SYMLINK_NOFOLLOW)?;
        Ok(SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT)
    };
    let mut root = match Dir::open(projects, flags, Mode::empty()) {
        Ok(root) => root,
        // Swapped for a link or a file since the look: removed as itself.
        Err(Errno::ELOOP | Errno::ENOTDIR) => return crate::write::remove_link(projects),
        Err(error) => return Err(error.into()),
    };
    for project in names(&mut root) {
        if kind(&root, &project)? != SFlag::S_IFDIR {
            unlinkat(&root, project.as_c_str(), UnlinkatFlags::NoRemoveDir)?;
            continue;
        }
        let mut folder = match Dir::openat(&root, project.as_c_str(), flags, Mode::empty()) {
            Ok(folder) => folder,
            Err(Errno::ELOOP | Errno::ENOTDIR) => {
                unlinkat(&root, project.as_c_str(), UnlinkatFlags::NoRemoveDir)?;
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        for item in names(&mut folder) {
            let bytes = item.as_bytes();
            let item_kind = kind(&folder, &item)?;
            if item_kind == SFlag::S_IFDIR {
                if !folder_stays(bytes, live) {
                    let path = projects
                        .join(OsStr::from_bytes(project.as_bytes()))
                        .join(OsStr::from_bytes(bytes));
                    crate::write::remove_tree(&path)?;
                }
                continue;
            }
            let one_name = item_kind == SFlag::S_IFREG
                && fstatat(&folder, item.as_c_str(), AtFlags::AT_SYMLINK_NOFOLLOW)?.st_nlink == 1;
            if !(file_stays(bytes) && one_name) {
                unlinkat(&folder, item.as_c_str(), UnlinkatFlags::NoRemoveDir)?;
            }
        }
    }
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn clean(projects: &Path, live: bool) -> io::Result<()> {
    if !real_folder(projects)? {
        return crate::write::remove_link(projects);
    }
    for project in std::fs::read_dir(projects)? {
        let project = project?;
        let path = project.path();
        if !project.file_type()?.is_dir() || !real_folder(&path)? {
            super::remove_entry(&path, project.file_type()?)?;
            continue;
        }
        for item in std::fs::read_dir(&path)? {
            let item = item?;
            let kind = item.file_type()?;
            let name = item.file_name();
            let bytes = name.as_encoded_bytes();
            let stays = if kind.is_dir() {
                folder_stays(bytes, live)
            } else {
                kind.is_file() && file_stays(bytes) && !super::has_second_name(&item.path())?
            };
            if !stays {
                super::remove_entry(&item.path(), kind)?;
            }
        }
    }
    Ok(())
}

/// Whether `path` opens as a folder that is no reparse point (`FILE_FLAG_OPEN_REPARSE_POINT`,
/// so a link or junction is opened as itself).
#[cfg(not(unix))]
fn real_folder(path: &Path) -> io::Result<bool> {
    let file = crate::write::open_read_no_follow(path)?;
    let meta = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        /// `FILE_ATTRIBUTE_REPARSE_POINT`.
        const REPARSE: u32 = 0x400;
        Ok(meta.is_dir() && meta.file_attributes() & REPARSE == 0)
    }
    #[cfg(not(windows))]
    Ok(meta.is_dir())
}
