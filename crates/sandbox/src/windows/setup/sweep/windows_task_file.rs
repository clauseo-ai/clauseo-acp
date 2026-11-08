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
//! The owner of a task file the cache holds no task for ([`Sweeper::task_file_owner`]) is
//! read the same way: the folders opened alike, the file opened by handle for `READ_CONTROL`
//! alone, refused if a reparse point or a folder, and its owner read through that handle.
//!
//! [`Sweeper::read_task_file`]: super::Sweeper::read_task_file
//! [`Sweeper::task_file_owner`]: super::Sweeper::task_file_owner

use std::path::Path;

use windows::Win32::Security::{DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION};
use windows::Win32::Storage::FileSystem::READ_CONTROL;

use super::{Failed, TaskFile, file_trust, task_file};
use crate::windows::acl::object::Object;
use crate::windows::acl::privilege::Privileges;
use crate::windows::acl_bytes::owner_sid;
use crate::windows::sids::Sid;

/// What the read enables.
const BACKUP: [&str; 1] = ["SeBackupPrivilege"];

/// The task file whose parts below `Tasks` are `parts` (the folders, then the task's name),
/// in the system folder `system`, at most `cap` bytes, and its security, as the module says.
pub(super) fn read(system: &Path, parts: &[String], cap: usize) -> Result<TaskFile, Failed> {
    let (name, folders) = parts
        .split_last()
        .ok_or_else(|| "the task file has no name".to_owned())?;
    let _backup = Privileges::enable(&BACKUP).map_err(|error| error.to_string())?;
    let folder = folder_of(system, folders)?;
    let info = OWNER_SECURITY_INFORMATION.0 | DACL_SECURITY_INFORMATION.0;
    let (bytes, descriptor) = match folder.read_child_secured(&wide(name), cap, info) {
        Ok(Some(read)) => read,
        Ok(None) => return Err("the task file is missing".to_owned()),
        Err(error) => return Err(format!("the task file couldn't be read ({error})")),
    };
    let security = file_trust::file_security(&descriptor)?;
    Ok(TaskFile { bytes, security })
}

/// The owner of the task file whose parts below `Tasks` are `parts`, in the system folder
/// `system`, as the module says.
pub(super) fn owner(system: &Path, parts: &[String]) -> Result<Sid, Failed> {
    let (name, folders) = parts
        .split_last()
        .ok_or_else(|| "the task file has no name".to_owned())?;
    let _backup = Privileges::enable(&BACKUP).map_err(|error| error.to_string())?;
    let folder = folder_of(system, folders)?;
    let file = folder
        .open_child_for(&wide(name), READ_CONTROL.0)
        .map_err(|error| format!("the task file couldn't be opened ({error})"))?;
    if file.reparse() || file.folder {
        return Err("the task file isn't a plain file".to_owned());
    }
    let descriptor = file
        .security(OWNER_SECURITY_INFORMATION.0)
        .map_err(|error| format!("the task file's owner couldn't be read ({error})"))?;
    owner_sid(&descriptor).ok_or_else(|| "the task file's descriptor names no owner".to_owned())
}

/// Whether the folder `path` (`\A\B`) below `Tasks`, in the system folder `system`, is there:
/// each part opened by handle as the module says; `Ok(false)` once one isn't found.
pub(super) fn folder_present(system: &Path, path: &str) -> Result<bool, Failed> {
    let (parent, name) = path
        .rsplit_once('\\')
        .ok_or_else(|| "the folder's path doesn't start at the root".to_owned())?;
    let parent = if parent.is_empty() { "\\" } else { parent };
    let parts = task_file::parts(parent, name)?;
    let _backup = Privileges::enable(&BACKUP).map_err(|error| error.to_string())?;
    match folder_of(system, &parts) {
        Ok(_) => Ok(true),
        Err(Missing::Gone) => Ok(false),
        Err(Missing::Failed(why)) => Err(why),
    }
}

/// Why [`folder_of`] gave no folder.
enum Missing {
    /// A part wasn't found.
    Gone,
    /// Anything else.
    Failed(Failed),
}

impl From<Missing> for Failed {
    fn from(missing: Missing) -> Failed {
        match missing {
            Missing::Gone => "a folder on the task file's path is gone".to_owned(),
            Missing::Failed(why) => why,
        }
    }
}

/// The folder `folders` name below `Tasks` in the system folder `system`, each opened by
/// handle as the module says.
fn folder_of(system: &Path, folders: &[String]) -> Result<Object, Missing> {
    let mut folder = Object::open_for(system, 0).map_err(|error| {
        Missing::Failed(format!("the system folder couldn't be opened ({error})"))
    })?;
    if folder.reparse() || !folder.folder {
        let why = "the system folder isn't a plain folder".to_owned();
        return Err(Missing::Failed(why));
    }
    for part in std::iter::once("Tasks").chain(folders.iter().map(String::as_str)) {
        let next = match folder.open_child_for(&wide(part), 0) {
            Ok(next) => next,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(Missing::Gone);
            }
            Err(error) => {
                return Err(Missing::Failed(format!(
                    "a folder on the task file's path couldn't be opened ({error})"
                )));
            }
        };
        if next.reparse() || !next.folder {
            let why = "a folder on the task file's path isn't a plain folder".to_owned();
            return Err(Missing::Failed(why));
        }
        folder = next;
    }
    Ok(folder)
}

/// `part` in UTF-16, with no NUL.
fn wide(part: &str) -> Vec<u16> {
    part.encode_utf16().collect()
}
