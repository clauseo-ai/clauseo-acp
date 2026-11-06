//! The clean of Claude's `projects` folder in a matter's state folder off Unix
//! (`super::clean_by_path`; Unix walks by handle, `handle.rs`). Each folder is opened with
//! `FILE_FLAG_OPEN_REPARSE_POINT` and, when it is a reparse point (a link or junction), removed
//! as itself and never descended into; a folder to go is removed with `remove_dir_all`, which
//! removes a reparse point inside as itself and never follows it. The swap of a folder between
//! that check and its listing is the named residual.
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
