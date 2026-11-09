//! The SYSTEM tasks read back and put back on Windows, for `task_identity.rs`'s comparison:
//!
//! - [`read_task`] (`task.rs`): a registered task's principal, actions, triggers and
//!   descriptor, read as whoever calls it (the lawyer reads through the owner's `FRFX` entry,
//!   whose read includes `READ_CONTROL`; the set-up reads elevated);
//! - [`replace_task`]: a per-user set-up's repair, elevated, in one step: a task that differs
//!   is deleted ([`delete_task`](super::task::delete_task)), then registered by `--adopt`'s own
//!   [`register_task`]; a missing one is registered;
//! - [`OsTasks`]: the readiness check's reader, as the lawyer, behind
//!   [`ready_with_pool_tasks`] (`UserSetup::state`) and [`sweep_task_needs_repair`] (a launch
//!   refused for `AwaitingSweep`).

use std::path::PathBuf;

use super::staging::as_read_back;
use super::task::{Apartment, TaskError, delete_task, read_task, register_task, service};
use super::task_identity::{TaskIdentity, TaskReader, needs_repair, ready_with_tasks};
use super::{SystemTask, task_sddl};
use crate::SetupState;
use crate::windows::admin_copy::{AdminCopyFs, admin_exe, os::WindowsAdminCopy};
use crate::windows::record::{admin_copy_owner, current_user_sid};
use crate::windows::setup::record_values::CopyOwner;
use crate::windows::sids::Sid;

/// A per-user set-up's repair of `task` for the record owner `owner`, elevated: deleted first
/// when `existed` (a task of the name that isn't the set-up's), then registered as `--adopt`
/// registers it ([`register_task`]).
pub fn replace_task(task: SystemTask, owner: &Sid, existed: bool) -> Result<(), TaskError> {
    if existed {
        let _apartment = Apartment::enter()?;
        delete_task(&service()?, task)?;
    }
    register_task(task, owner)
}

/// `task_sddl(owner)` as the Task Scheduler writes it back.
pub fn task_sddl_read_back(owner: &Sid) -> Result<String, String> {
    as_read_back(&task_sddl(owner))
}

/// The readiness check's reader, as the lawyer.
pub struct OsTasks;

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

    fn found(&mut self, task: SystemTask) -> Result<Option<TaskIdentity>, String> {
        read_task(task).map_err(|error| error.to_string())
    }

    fn sddl(&mut self, owner: &Sid) -> Result<String, String> {
        task_sddl_read_back(owner)
    }
}

/// `UserSetup::state`'s last step (`task_identity::ready_with_tasks`) on this machine.
pub fn ready_with_pool_tasks(state: SetupState) -> SetupState {
    ready_with_tasks(state, &mut OsTasks)
}

/// Whether `\Clauseo\SandboxSweep` is missing or isn't the set-up's on a per-user install, as
/// the lawyer reads it; `false` when it can't be told.
pub fn sweep_task_needs_repair() -> bool {
    needs_repair(&mut OsTasks, SystemTask::Sweep).unwrap_or_else(|error| {
        tracing::warn!(%error, "the sandbox's sweep task can't be checked");
        false
    })
}
