//! [`ensure_task`]: a task registered again when it is missing or its definition differs from
//! what `register_task` would register now (`SystemTask::registered_as`). An elevated set-up
//! calls it for the set-up task (`late_steps::tasks`), so a lawyer whose task was deleted or
//! changed meets the prompt once, not on every set-up. Split from `task.rs` for its size.
#![expect(
    unsafe_code,
    reason = "FFI to the Task Scheduler's task definition interfaces"
)]

use std::path::Path;

use windows::Win32::Foundation::VARIANT_BOOL;
use windows::Win32::Security::{DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION};
use windows::Win32::System::TaskScheduler::{
    IExecAction, IRegisteredTask, TASK_INSTANCES_POLICY, TASK_LOGON_SERVICE_ACCOUNT,
    TASK_LOGON_TYPE, TASK_RUNLEVEL_HIGHEST, TASK_RUNLEVEL_TYPE,
};
use windows::core::{BSTR, Interface};

use super::staging::as_read_back;
use super::task::{FOLDER_NAME, TaskError, register_in, with_task_in};
use super::windows_refresher::{EXE_NAME, sandbox_dir};
use super::{Ensured, Registered, Starters, SystemTask, action_path};

/// Registers `spec` for `starters` unless it is there as `register_task` would register it
/// now: [`Ensured::Registered`] when it was registered again, [`Ensured::ReEnabled`] when the
/// one there was disabled, [`Ensured::Current`] when it was already current. A definition that
/// can't be read counts as one that differs.
pub fn ensure_task(spec: SystemTask, starters: &Starters) -> Result<Ensured, TaskError> {
    let exe = sandbox_dir().map_err(TaskError::Path)?.join(EXE_NAME);
    ensure_in(FOLDER_NAME, spec, starters, &exe)
}

/// [`ensure_task`] in the folder `\<folder_name>`, its action `exe`: the tests' own folder and
/// program, the production ones otherwise.
pub(super) fn ensure_in(
    folder_name: &str,
    spec: SystemTask,
    starters: &Starters,
    exe: &Path,
) -> Result<Ensured, TaskError> {
    let path = action_path(exe);
    let sddl = as_read_back(&starters.sddl()).map_err(TaskError::Descriptor)?;
    let (current, disabled) = match with_task_in(folder_name, spec, |task| Ok(read(task))) {
        Ok(Some(found)) => (
            spec.registered_as(&found, starters, &path, &sddl),
            !found.enabled,
        ),
        Ok(None) | Err(TaskError::NotRegistered) => (false, false),
        Err(error) => return Err(error),
    };
    if current {
        return Ok(Ensured::Current);
    }
    register_in(folder_name, spec, starters, exe)?;
    Ok(if disabled {
        Ensured::ReEnabled
    } else {
        Ensured::Registered
    })
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
        let (mut path, mut arguments, mut working) = (BSTR::new(), BSTR::new(), BSTR::new());
        exec.Path(&raw mut path).ok()?;
        exec.Arguments(&raw mut arguments).ok()?;
        exec.WorkingDirectory(&raw mut working).ok()?;
        found.path = path.to_string();
        found.arguments = arguments.to_string();
        found.working_dir = working.to_string();
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
        found.instances = instances.0;
        let (mut enabled, mut hidden) = (VARIANT_BOOL::default(), VARIANT_BOOL::default());
        settings.Enabled(&raw mut enabled).ok()?;
        settings.Hidden(&raw mut hidden).ok()?;
        found.enabled = enabled.as_bool();
        found.hidden = hidden.as_bool();
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
