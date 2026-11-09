//! [`register_task`]: the one registration of a SYSTEM task in `\Clauseo` (`task.rs` says what
//! the task is), for `--adopt`, a per-machine install's set-up and a per-user set-up's repair
//! (`task_read::replace_task`) alike, so each writes the same definition. The descriptor
//! (`task_sddl`) goes three ways at once: the folder's, the task's (`RegisterTaskDefinition`'s
//! `sddl`), and the definition's own (`IRegistrationInfo::SecurityDescriptor`, the XML's
//! `<SecurityDescriptor>`). The flags are [`REGISTER_FLAGS`]: create or update, with
//! `TASK_DONT_ADD_PRINCIPAL_ACE`, so Windows adds no `(A;;FR;;;SY)` of its own.
#![expect(unsafe_code, reason = "FFI to COM and the Task Scheduler's interfaces")]

use std::path::Path;

use windows::Win32::Foundation::VARIANT_BOOL;
use windows::Win32::System::TaskScheduler::{
    IExecAction, ILogonTrigger, TASK_ACTION_EXEC, TASK_CREATE_OR_UPDATE,
    TASK_DONT_ADD_PRINCIPAL_ACE, TASK_INSTANCES_IGNORE_NEW, TASK_INSTANCES_QUEUE,
    TASK_LOGON_SERVICE_ACCOUNT, TASK_RUNLEVEL_HIGHEST, TASK_TRIGGER_LOGON,
};
use windows::Win32::System::Variant::VARIANT;
use windows::core::{BSTR, Interface};

use super::task::{Apartment, SYSTEM, TaskError, com, folder, service};
use super::windows_refresher::{EXE_NAME, sandbox_dir};
use super::{REGISTER_FLAGS, SystemTask, action_path, task_sddl};
use crate::windows::sids::Sid;

// `rules::REGISTER_FLAGS` spells out the Task Scheduler's own values, for the tests on every OS.
const _: () = assert!(REGISTER_FLAGS == TASK_CREATE_OR_UPDATE.0 | TASK_DONT_ADD_PRINCIPAL_ACE.0);

/// Registers (or updates) the task `spec` names as the module says, for the record owner
/// `owner_sid`. Elevated: `--adopt` calls it for each task once the admin copy is locked in
/// place, before it writes the record's admin-copy values (a refresh before then exits 33,
/// changing nothing). On a per-machine install, the set-up calls it for the stop and sweep tasks
/// once the record names `owner_sid`; a task already there is replaced, descriptor included.
pub fn register_task(spec: SystemTask, owner_sid: &Sid) -> Result<(), TaskError> {
    let exe = sandbox_dir().map_err(TaskError::Path)?.join(EXE_NAME);
    register_task_at(spec, owner_sid, &exe)
}

/// [`register_task`] with `exe` as the action's program: a per-user set-up passes the path it
/// checked (`task_identity::action_exe`), so the path checked is the path registered.
pub fn register_task_at(spec: SystemTask, owner_sid: &Sid, exe: &Path) -> Result<(), TaskError> {
    let sddl = task_sddl(owner_sid);
    let _apartment = Apartment::enter()?;
    let service = service()?;
    let folder = folder(&service, &sddl)?;
    // SAFETY: each call below is on a live interface the one before returned, with BSTRs and
    // VARIANT values that outlive it; flags 0 for NewTask.
    unsafe {
        let task = service.NewTask(0).map_err(com("ITaskService::NewTask"))?;
        let info = task.RegistrationInfo().map_err(com("RegistrationInfo"))?;
        info.SetAuthor(&BSTR::from("Clauseo"))
            .map_err(com("SetAuthor"))?;
        info.SetDescription(&BSTR::from(spec.description()))
            .map_err(com("SetDescription"))?;
        // The XML's `<SecurityDescriptor>`, so the definition carries the descriptor it was
        // registered with whatever the folder held, and reads back the same every time.
        info.SetSecurityDescriptor(&VARIANT::from(sddl.as_str()))
            .map_err(com("IRegistrationInfo::SetSecurityDescriptor"))?;
        let principal = task.Principal().map_err(com("Principal"))?;
        principal
            .SetUserId(&BSTR::from(SYSTEM))
            .map_err(com("IPrincipal::SetUserId"))?;
        principal
            .SetLogonType(TASK_LOGON_SERVICE_ACCOUNT)
            .map_err(com("SetLogonType"))?;
        principal
            .SetRunLevel(TASK_RUNLEVEL_HIGHEST)
            .map_err(com("SetRunLevel"))?;
        let settings = task.Settings().map_err(com("Settings"))?;
        let no = VARIANT_BOOL::from(false);
        let yes = VARIANT_BOOL::from(true);
        settings
            .SetRunOnlyIfNetworkAvailable(no)
            .map_err(com("SetRunOnlyIfNetworkAvailable"))?;
        settings
            .SetExecutionTimeLimit(&BSTR::from(spec.execution_limit()))
            .map_err(com("SetExecutionTimeLimit"))?;
        let instances = if spec.queues() {
            TASK_INSTANCES_QUEUE
        } else {
            TASK_INSTANCES_IGNORE_NEW
        };
        settings
            .SetMultipleInstances(instances)
            .map_err(com("SetMultipleInstances"))?;
        settings
            .SetAllowDemandStart(yes)
            .map_err(com("SetAllowDemandStart"))?;
        settings
            .SetDisallowStartIfOnBatteries(no)
            .map_err(com("SetDisallowStartIfOnBatteries"))?;
        settings
            .SetStopIfGoingOnBatteries(no)
            .map_err(com("SetStopIfGoingOnBatteries"))?;
        settings
            .SetEnabled(yes)
            .map_err(com("ITaskSettings::SetEnabled"))?;
        if spec.at_logon() {
            let trigger = task.Triggers().map_err(com("Triggers"))?;
            let trigger = trigger
                .Create(TASK_TRIGGER_LOGON)
                .map_err(com("ITriggerCollection::Create"))?;
            let logon: ILogonTrigger = trigger.cast().map_err(com("ILogonTrigger"))?;
            logon
                .SetUserId(&BSTR::from(owner_sid.as_str()))
                .map_err(com("ILogonTrigger::SetUserId"))?;
        }
        let action = task.Actions().map_err(com("Actions"))?;
        let action = action
            .Create(TASK_ACTION_EXEC)
            .map_err(com("IActionCollection::Create"))?;
        let exec: IExecAction = action.cast().map_err(com("IExecAction"))?;
        // Quoted: the path has a space (`Program Files`), and a command line split there would
        // name another program. Every folder on it is also under %ProgramFiles%, which only
        // administrators can write.
        exec.SetPath(&BSTR::from(action_path(exe)))
            .map_err(com("IExecAction::SetPath"))?;
        exec.SetArguments(&BSTR::from(spec.argument()))
            .map_err(com("IExecAction::SetArguments"))?;
        folder
            .RegisterTaskDefinition(
                &BSTR::from(spec.name()),
                &task,
                REGISTER_FLAGS,
                &VARIANT::from(SYSTEM),
                &VARIANT::default(),
                TASK_LOGON_SERVICE_ACCOUNT,
                &VARIANT::from(sddl.as_str()),
            )
            .map_err(com("ITaskFolder::RegisterTaskDefinition"))?;
    }
    Ok(())
}
