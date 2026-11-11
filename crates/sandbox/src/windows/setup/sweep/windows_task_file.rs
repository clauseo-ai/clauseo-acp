//! The task file's read for [`Sweeper::read_task_file`]: the system folder
//! (`SHGetKnownFolderPath`, `FOLDERID_System`, never the environment: `windows_sweeper.rs`)
//! opened as itself, then `Tasks` and each folder below it opened by handle relative to the one
//! above (`acl::object`: `NtCreateFile` with `OBJ_DONT_REPARSE` and `FILE_OPEN_REPARSE_POINT`, as
//! `admin_copy/lock_tree_os.rs` walks), each refused if a reparse point or not a folder; then
//! the file read through its own handle (`Object::read_child_secured`: a plain file with one
//! name, not a reparse point, at most the cap), with its owner and DACL read through that same
//! handle for `file_trust.rs`. Each folder is opened for its attributes alone, the file for
//! reading and `READ_CONTROL`: no open asks for a right to write. The file is opened sharing
//! reads alone, so a write handle the pool user opened before the DACL changed can't write it
//! during the read; while one is open the read fails (a sharing violation), and the task is 56.
//! `SeBackupPrivilege` is enabled for the opens alone and put back after (`acl::privilege`),
//! with backup intent on each open, so a file whose descriptor shuts SYSTEM out is still read.
//! Errors name the step, never a folder or task.
//!
//! [`Sweeper::read_task_file`]: super::Sweeper::read_task_file

use std::path::Path;

use windows::Win32::Security::{DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION};

use super::{Failed, TaskFile, file_trust};
use crate::windows::acl::object::Object;
use crate::windows::acl::privilege::Privileges;

/// What the read enables.
const BACKUP: [&str; 1] = ["SeBackupPrivilege"];

/// The task file whose parts below `Tasks` are `parts` (the folders, then the task's name),
/// in the system folder `system`, at most `cap` bytes, and its security, as the module says.
pub(super) fn read(system: &Path, parts: &[String], cap: usize) -> Result<TaskFile, Failed> {
    let (name, folders) = parts
        .split_last()
        .ok_or_else(|| "the task file has no name".to_owned())?;
    let _backup = Privileges::enable(&BACKUP).map_err(|error| error.to_string())?;
    let mut folder = Object::open_for(system, 0)
        .map_err(|error| format!("the system folder couldn't be opened ({error})"))?;
    if folder.reparse() || !folder.folder {
        return Err("the system folder isn't a plain folder".to_owned());
    }
    for part in std::iter::once("Tasks").chain(folders.iter().map(String::as_str)) {
        let next = folder.open_child_for(&wide(part), 0).map_err(|error| {
            format!("a folder on the task file's path couldn't be opened ({error})")
        })?;
        if next.reparse() || !next.folder {
            return Err("a folder on the task file's path isn't a plain folder".to_owned());
        }
        folder = next;
    }
    let info = OWNER_SECURITY_INFORMATION.0 | DACL_SECURITY_INFORMATION.0;
    let (bytes, descriptor) = match folder.read_child_secured(&wide(name), cap, info) {
        Ok(Some(read)) => read,
        Ok(None) => return Err("the task file is missing".to_owned()),
        Err(error) => return Err(format!("the task file couldn't be read ({error})")),
    };
    let security = file_trust::file_security(&descriptor)?;
    Ok(TaskFile { bytes, security })
}

/// `part` in UTF-16, with no NUL.
fn wide(part: &str) -> Vec<u16> {
    part.encode_utf16().collect()
}
