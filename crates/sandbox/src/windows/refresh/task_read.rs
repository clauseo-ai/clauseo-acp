//! The SYSTEM tasks read back and put back on Windows, for `task_identity.rs`'s comparison:
//!
//! - [`replace_task`]: a per-user set-up's repair, elevated: the task registered for the owner
//!   by `--adopt`'s own registration (`register_task_at`, which creates or updates it in one
//!   call, descriptor included), running the admin copy at the path the set-up checked;
//! - [`OsTasks`]: the readiness check's reader, as the lawyer, behind
//!   [`ready_with_user_tasks`] (`UserSetup::state`) and [`tasks_repair`] (a launch's check
//!   before the pool user's logon, and its refusal for `AwaitingSweep`). It reads the three
//!   [`USER_TASKS`] over one connection ([`read_tasks`]), once per check: a COM connection to
//!   the Task Scheduler service, then for each task a `GetTask`, its definition and its
//!   descriptor, a few RPCs each (reasoned, not measured: no Windows here).

use std::path::{Path, PathBuf};

use super::staging::sddl_bytes;
use super::task::{TaskError, TaskRead, read_task, read_tasks};
use super::task_identity::{self, ReadError, TaskIdentity, TaskReader, ready_with_tasks};
use super::task_register::register_task_at;
use super::{SystemTask, USER_TASKS, task_sddl};
use crate::SetupState;
use crate::windows::admin_copy::{AdminCopyFs, admin_exe, os::WindowsAdminCopy};
use crate::windows::record::{admin_copy_owner, current_user_sid};
use crate::windows::setup::record_values::CopyOwner;
use crate::windows::sids::Sid;

/// A per-user set-up's repair of `task` for the record owner `owner`, elevated: registered, or
/// updated in place, as `--adopt` registers it, running `exe`, the path `action_exe` checked.
pub fn replace_task(task: SystemTask, owner: &Sid, exe: &Path) -> Result<(), TaskError> {
    register_task_at(task, owner, exe)
}

/// `task_sddl(owner)` made into a self-relative descriptor.
pub fn task_descriptor(owner: &Sid) -> Result<Vec<u8>, String> {
    sddl_bytes(&task_sddl(owner))
}

/// A read's error as `task_identity` takes it: a refused read is a changed task.
pub fn read_error(error: TaskError) -> ReadError {
    match error {
        TaskError::Com { hresult, .. } => ReadError::of(hresult, error.to_string()),
        other @ (TaskError::NotRegistered | TaskError::Path(_) | TaskError::Folder(_)) => {
            ReadError::Failed(other.to_string())
        }
    }
}

/// The readiness check's reader, as the lawyer: [`USER_TASKS`] read once, together.
#[derive(Default)]
pub struct OsTasks {
    /// [`USER_TASKS`] as [`read_tasks`] read them, once asked for.
    tasks: Option<Result<Vec<TaskRead>, TaskError>>,
}

impl TaskReader for OsTasks {
    fn copy_owner(&mut self) -> Result<Option<CopyOwner>, String> {
        let text = admin_copy_owner().map_err(|error| error.to_string())?;
        text.map(|text| {
            CopyOwner::parse(&text).ok_or_else(|| "AdminCopyOwner is malformed".to_owned())
        })
        .transpose()
    }

    fn user(&mut self) -> Result<Sid, String> {
        current_user_sid().map_err(|error| error.to_string())
    }

    fn exe(&mut self) -> Result<PathBuf, String> {
        let program_files = WindowsAdminCopy
            .program_files()
            .map_err(|error| error.to_string())?;
        Ok(admin_exe(&program_files))
    }

    fn found(&mut self, task: SystemTask) -> Result<Option<TaskIdentity>, ReadError> {
        let Some(at) = USER_TASKS.iter().position(|known| *known == task) else {
            return read_task(task).map_err(read_error);
        };
        let tasks = self.tasks.get_or_insert_with(|| read_tasks(&USER_TASKS));
        match tasks {
            Ok(read) => read
                .get(at)
                .cloned()
                .unwrap_or(Ok(None))
                .map_err(read_error),
            Err(error) => Err(read_error(error.clone())),
        }
    }

    fn descriptor(&mut self, owner: &Sid) -> Result<Vec<u8>, String> {
        task_descriptor(owner)
    }
}

/// `UserSetup::state`'s last step (`task_identity::ready_with_tasks`) on this machine.
pub fn ready_with_user_tasks(state: SetupState) -> SetupState {
    ready_with_tasks(state, &mut OsTasks::default())
}

/// What a launch says about the stop and sweep tasks before the pool user's logon, and when it
/// is refused for `AwaitingSweep`, as the lawyer reads them (`task_identity::tasks_repair`):
/// `None` while both are the set-up's. The refresh task never gates a launch.
pub fn tasks_repair() -> Option<&'static str> {
    task_identity::tasks_repair(&mut OsTasks::default())
}
