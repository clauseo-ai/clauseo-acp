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
use super::task::{
    TaskError, TaskRead, read_task, read_tasks, request_admin_refresh, request_pool_stop,
    request_pool_sweep,
};
use super::task_call::{TASK_CALL_LIMIT, repair_within, within};
use super::task_identity::action_exe;
use super::task_identity::{
    self, AccountName, ReadError, TASK_CHANGED, TASK_MISSING, TaskIdentity, TaskReader,
    ready_with_tasks,
};
use super::task_keep::TaskKeeper;
use super::task_register::{register_task_at, set_task_descriptor};
use super::{SystemTask, USER_TASKS, task_sddl};
use crate::SetupState;
use crate::windows::admin_copy::{AdminCopyFs, admin_exe, os::WindowsAdminCopy};
use crate::windows::record::{admin_copy_owner, current_user_sid};
use crate::windows::setup::accounts::lookup_sid;
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

    fn account_name(&mut self, sid: &Sid) -> Result<Option<AccountName>, String> {
        local_account_name(sid)
    }
}

/// [`task_keep`]'s keeper as LOCAL SYSTEM, inside `--refresh`: the admin copy checked whole,
/// the tasks read and registered through the Task Scheduler at their fixed names.
pub(super) struct OsKeeper;

impl TaskKeeper for OsKeeper {
    fn exe(&mut self) -> Result<PathBuf, String> {
        action_exe(&WindowsAdminCopy).map_err(|blocked| blocked.to_string())
    }

    fn found(&mut self, task: SystemTask) -> Result<Option<TaskIdentity>, ReadError> {
        read_task(task).map_err(read_error)
    }

    fn expected(
        &mut self,
        task: SystemTask,
        owner: &Sid,
        exe: &Path,
    ) -> Result<TaskIdentity, String> {
        Ok(TaskIdentity::expected(
            task,
            owner,
            exe,
            task_descriptor(owner)?,
        ))
    }

    fn register(&mut self, task: SystemTask, owner: &Sid, exe: &Path) -> Result<(), String> {
        register_task_at(task, owner, exe).map_err(|error| error.to_string())
    }

    fn set_descriptor(&mut self, task: SystemTask, owner: &Sid) -> Result<(), String> {
        heal_task_descriptor(task, owner).map_err(|error| error.to_string())
    }

    fn account_name(&mut self, sid: &Sid) -> Result<Option<AccountName>, String> {
        local_account_name(sid)
    }
}

/// The account `sid` names, looked up locally (`LookupAccountSidW`), for `same_account`.
pub fn local_account_name(sid: &Sid) -> Result<Option<AccountName>, String> {
    let found = lookup_sid(sid).map_err(|error| error.to_string())?;
    Ok(found.map(|(name, domain, _use)| (name, domain)))
}

/// A per-user set-up's repair of `task`'s own descriptor, elevated, when registering it left an
/// older one in place (an earlier build's task updated in place keeps its descriptor): set to
/// `task_sddl(owner)` through `IRegisteredTask::SetSecurityDescriptor`, nothing deleted.
pub fn heal_task_descriptor(task: SystemTask, owner: &Sid) -> Result<(), TaskError> {
    set_task_descriptor(task, &task_sddl(owner))
}

/// `UserSetup::state`'s last step (`task_identity::ready_with_tasks`) on this machine.
/// A stop or sweep task missing or changed also asks for a refresh run, which puts it back
/// without a prompt (`task_keep`); the next check then reads it back. Nothing waits for it.
pub fn ready_with_user_tasks(state: SetupState) -> SetupState {
    let state = ready_with_tasks(state, &mut OsTasks::default());
    if let SetupState::Failed(words) = &state
        && (words == TASK_MISSING || words == TASK_CHANGED)
    {
        launch_admin_refresh();
    }
    state
}

/// What a launch says about the stop and sweep tasks before the pool user's logon, and when it
/// is refused for `AwaitingSweep`, as the lawyer reads them (`task_identity::tasks_repair`):
/// `None` while both are the set-up's. The refresh task never gates a launch.
pub fn tasks_repair() -> Option<&'static str> {
    task_identity::tasks_repair(&mut OsTasks::default())
}

/// `request_admin_refresh` for a launch or the readiness check, within [`TASK_CALL_LIMIT`]:
/// whether the run was asked for. Its `--refresh` puts the tasks back (`task_keep`).
pub fn launch_admin_refresh() -> bool {
    let asked = within(TASK_CALL_LIMIT, || {
        request_admin_refresh().map_err(|e| e.to_string())
    });
    match asked.and_then(|asked| asked) {
        Ok(()) => true,
        Err(why) => {
            tracing::warn!(%why, "the sandbox's refresh task couldn't be started");
            false
        }
    }
}

/// [`tasks_repair`] for a launch, within [`TASK_CALL_LIMIT`]: couldn't-check when the Task
/// Scheduler (or a local name lookup) didn't answer by then.
pub fn launch_tasks_repair() -> Option<&'static str> {
    repair_within(TASK_CALL_LIMIT, tasks_repair)
}

/// `request_pool_stop` for a launch or a session's end, within [`TASK_CALL_LIMIT`]: `false`
/// when it didn't answer by then.
pub fn launch_pool_stop() -> bool {
    within(TASK_CALL_LIMIT, request_pool_stop).unwrap_or_else(|why| {
        tracing::warn!(%why, "the sandbox's stop task couldn't be started");
        false
    })
}

/// `request_pool_sweep` for a launch or a session's end, within [`TASK_CALL_LIMIT`].
pub fn launch_pool_sweep() -> Result<(), String> {
    within(TASK_CALL_LIMIT, || {
        request_pool_sweep().map_err(|error| error.to_string())
    })?
}
