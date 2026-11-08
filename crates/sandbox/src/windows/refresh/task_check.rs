//! [`ensure_task`]: a task registered again when it is missing or its definition differs from
//! what `register_task` would register now (`SystemTask::registered_as`). An elevated set-up
//! calls it for the set-up task (`late_steps::tasks`), so a lawyer whose task was deleted or
//! changed meets the prompt once, not on every set-up. Split from `task.rs` for its size.
#![expect(
    unsafe_code,
    reason = "FFI to the Task Scheduler's task definition interfaces"
)]

use windows::Win32::Security::{DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION};
use windows::Win32::System::TaskScheduler::{
    IExecAction, IRegisteredTask, TASK_INSTANCES_POLICY, TASK_INSTANCES_QUEUE,
    TASK_LOGON_SERVICE_ACCOUNT, TASK_LOGON_TYPE, TASK_RUNLEVEL_HIGHEST, TASK_RUNLEVEL_TYPE,
};
use windows::core::{BSTR, Interface};

use super::staging::as_read_back;
use super::task::{TaskError, register_task, with_task};
use super::windows_refresher::{EXE_NAME, sandbox_dir};
use super::{Registered, Starters, SystemTask, action_path};

/// Registers `spec` for `starters` unless it is there as `register_task` would register it
/// now: `Ok(true)` when it was registered again, `Ok(false)` when it was already current. A
/// definition that can't be read counts as one that differs.
pub fn ensure_task(spec: SystemTask, starters: &Starters) -> Result<bool, TaskError> {
    let path = action_path(&sandbox_dir().map_err(TaskError::Path)?.join(EXE_NAME));
    let sddl = as_read_back(&starters.sddl()).map_err(TaskError::Descriptor)?;
    let current = match with_task(spec, |task| Ok(read(task))) {
        Ok(Some(found)) => spec.registered_as(&found, starters, &path, &sddl),
        Ok(None) | Err(TaskError::NotRegistered) => false,
        Err(error) => return Err(error),
    };
    if current {
        return Ok(false);
    }
    register_task(spec, starters)?;
    Ok(true)
}

/// `task`'s definition as [`Registered`]; `None` when any of it can't be read.
fn read(task: &IRegisteredTask) -> Option<Registered> {
    let info = (OWNER_SECURITY_INFORMATION.0 | DACL_SECURITY_INFORMATION.0).cast_signed();
    let mut found = Registered::default();
    // SAFETY: every call is on a live interface the one before returned, each out-pointer live
    // for its call.
    unsafe {
        found.sddl = task.GetSecurityDescriptor(info).ok()?.to_string();
        let definition = task.Definition().ok()?;
        let actions = definition.Actions().ok()?;
        actions.Count(&raw mut found.actions).ok()?;
        let exec: IExecAction = actions.get_Item(1).ok()?.cast().ok()?;
        let (mut path, mut arguments) = (BSTR::new(), BSTR::new());
        exec.Path(&raw mut path).ok()?;
        exec.Arguments(&raw mut arguments).ok()?;
        found.path = path.to_string();
        found.arguments = arguments.to_string();
        let principal = definition.Principal().ok()?;
        let mut user = BSTR::new();
        principal.UserId(&raw mut user).ok()?;
        found.user = user.to_string();
        let (mut logon, mut level) = (TASK_LOGON_TYPE::default(), TASK_RUNLEVEL_TYPE::default());
        principal.LogonType(&raw mut logon).ok()?;
        principal.RunLevel(&raw mut level).ok()?;
        found.service_highest =
            logon == TASK_LOGON_SERVICE_ACCOUNT && level == TASK_RUNLEVEL_HIGHEST;
        let settings = definition.Settings().ok()?;
        let mut instances = TASK_INSTANCES_POLICY::default();
        settings.MultipleInstances(&raw mut instances).ok()?;
        found.queues = instances == TASK_INSTANCES_QUEUE;
        let mut limit = BSTR::new();
        settings.ExecutionTimeLimit(&raw mut limit).ok()?;
        found.limit = limit.to_string();
        definition
            .Triggers()
            .ok()?
            .Count(&raw mut found.triggers)
            .ok()?;
    }
    Some(found)
}
