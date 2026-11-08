//! The SYSTEM tasks `\Clauseo\SandboxRefresh` (ADR 0049, "Admin copy refresh"),
//! `\Clauseo\SandboxStop` ("Stopping the pool's processes") and `\Clauseo\SandboxSetUp`
//! (`setup::serve`), through the Task Scheduler's COM API (`ITaskService`), never `schtasks`
//! text:
//!
//! - [`register_task`], which plain `--adopt` calls elevated for each of [`TASKS`](super::TASKS)
//!   per user, and every set-up with an admin copy for the owner it records
//!   (`late_steps::tasks`): both per user, the stop task alone per machine. The task runs as
//!   LOCAL SYSTEM, starts
//!   `<FOLDERID_ProgramFiles>\Clauseo\Sandbox\clauseo-sandbox-setup.exe` with its one argument
//!   (`--refresh`, `--stop`) and nothing else, on demand (the refresh also at the record owner's
//!   logon), needs no network, stops after its limit (5 minutes, 2 minutes), and never runs twice
//!   at once. Its descriptor ([`Starters::sddl`]) is owned by Administrators and lets
//!   SYSTEM and Administrators do anything and the owner read and execute it (`FRFX`), which
//!   starts it and ends a running one, nothing more. The set-up task (`--serve-set-up`, 30
//!   minutes) is registered by `--adopt --place-only` for the lawyer and by a per-machine
//!   `--adopt` for every interactive user ([`Starters::Interactive`]); its runs queue rather
//!   than being dropped. The folder `\Clauseo` gets its own fixed owner and DACL
//!   ([`FOLDER_SDDL`](super::FOLDER_SDDL)), made with them, or set again when an
//!   administrator made it; one anyone else made is deleted with what is in it and made again
//!   (`task_folder.rs`).
//! - [`deny_set_up_to`], which a per-machine set-up calls once the pool's group exists: the
//!   set-up task's descriptor again, the group refused first.
//! - [`unregister_task`], which `--remove --uninstall` calls (it deletes staging itself,
//!   taking it back if need be: `setup::admin_remove`). A running set-up is ended first.
//! - [`request_admin_refresh`], which the app calls as the lawyer, unelevated, after an update,
//!   and [`request_pool_stop`]: each asks the Task Scheduler to run its task
//!   (`IRegisteredTask::Run`), which the owner's read-and-execute entry (`FRFX`) allows.
#![expect(unsafe_code, reason = "FFI to COM and the Task Scheduler's interfaces")]

use std::path::Path;

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, RPC_E_CHANGED_MODE, VARIANT_BOOL};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::TaskScheduler::{
    IExecAction, ILogonTrigger, IRegisteredTask, ITaskService, TASK_ACTION_EXEC,
    TASK_CREATE_OR_UPDATE, TASK_INSTANCES_IGNORE_NEW, TASK_INSTANCES_QUEUE,
    TASK_LOGON_SERVICE_ACCOUNT, TASK_RUNLEVEL_HIGHEST, TASK_TRIGGER_LOGON, TaskScheduler,
};
use windows::Win32::System::Variant::VARIANT;
use windows::core::{BSTR, HRESULT, Interface};

use super::last_run::{LastRun, last_run};
use super::task_scheduler::folder;
use super::windows_refresher::{EXE_NAME, sandbox_dir};
use super::{Starters, SystemTask, action_path};
use crate::windows::sids::Sid;

/// The task's folder.
pub const TASK_FOLDER: &str = "\\Clauseo";
/// The folder's name under the root, for creating and deleting it.
const FOLDER_NAME: &str = "Clauseo";
/// LOCAL SYSTEM, as the task's principal.
const SYSTEM: &str = "S-1-5-18";

/// Why a task call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TaskError {
    /// The task isn't registered.
    #[error("the task isn't registered in {TASK_FOLDER}")]
    NotRegistered,
    /// A COM or Task Scheduler call failed.
    #[error("{call} failed ({hresult:#010x})")]
    Com {
        /// The call.
        call: &'static str,
        /// Its HRESULT.
        hresult: u32,
    },
    /// The admin copy's path couldn't be found.
    #[error("the admin copy's path: {0}")]
    Path(String),
    /// The folder couldn't be made ready (`task_folder::prepare`).
    #[error("the folder {TASK_FOLDER}: {0}")]
    Folder(String),
    /// A task's descriptor couldn't be written out as Windows reads it back (`ensure_task`).
    #[error("the task's descriptor: {0}")]
    Descriptor(String),
}

/// `call` failed with `error`, as a line for the log.
pub(super) fn text(call: &'static str) -> impl Fn(windows::core::Error) -> String {
    move |error| com(call)(error).to_string()
}

/// `call` failed with `error`.
fn com(call: &'static str) -> impl Fn(windows::core::Error) -> TaskError {
    move |error| TaskError::Com {
        call,
        hresult: error.code().0.cast_unsigned(),
    }
}

/// Whether `error` says the folder or task isn't there.
pub(super) fn missing(error: &windows::core::Error) -> bool {
    error.code() == HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0)
}

/// COM on this thread for the guard's life: uninitialised on drop when this call initialised
/// it; a thread already in another apartment keeps it and is used as it is.
pub(super) struct Apartment(bool);

impl Apartment {
    /// Joins the multithreaded apartment, or uses the thread's.
    pub(super) fn enter() -> Result<Apartment, TaskError> {
        // SAFETY: no reserved pointer; undone on drop when it succeeded.
        let entered = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if entered == RPC_E_CHANGED_MODE {
            return Ok(Apartment(false));
        }
        entered.ok().map_err(com("CoInitializeEx"))?;
        Ok(Apartment(true))
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances this guard's successful CoInitializeEx, on the same thread.
            unsafe { CoUninitialize() };
        }
    }
}

/// The Task Scheduler on this machine, connected as this process's user.
pub(super) fn service() -> Result<ITaskService, TaskError> {
    // SAFETY: a registered in-process class and its interface; no outer object.
    let service: ITaskService =
        unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER) }
            .map_err(com("CoCreateInstance(TaskScheduler)"))?;
    let none = VARIANT::default();
    // SAFETY: empty VARIANT values: this machine, this process's user.
    unsafe { service.Connect(&none, &none, &none, &none) }.map_err(com("ITaskService::Connect"))?;
    Ok(service)
}

/// Registers (or updates) the task `spec` names as the module says, startable by `starters`.
/// Elevated: `--adopt` calls it for each task once the admin copy is locked in place, before
/// it writes the record's admin-copy values (a refresh before then exits 33, changing
/// nothing). On a per-machine install, the set-up calls it for the stop task once the record
/// names the owner; a task already there is replaced, descriptor included.
pub fn register_task(spec: SystemTask, starters: &Starters) -> Result<(), TaskError> {
    let exe = sandbox_dir().map_err(TaskError::Path)?.join(EXE_NAME);
    register_in(FOLDER_NAME, spec, starters, &exe)
}

/// [`register_task`] into the folder `\<folder_name>`, its action `exe`: the tests' own
/// folder and program, the production steps otherwise.
pub(super) fn register_in(
    folder_name: &str,
    spec: SystemTask,
    starters: &Starters,
    exe: &Path,
) -> Result<(), TaskError> {
    let sddl = starters.sddl();
    let _apartment = Apartment::enter()?;
    let service = service()?;
    let folder = folder(&service, folder_name)?;
    // SAFETY: each call below is on a live interface the one before returned, with BSTRs and
    // VARIANT values that outlive it; flags 0 for NewTask.
    unsafe {
        let task = service.NewTask(0).map_err(com("ITaskService::NewTask"))?;
        let info = task.RegistrationInfo().map_err(com("RegistrationInfo"))?;
        info.SetAuthor(&BSTR::from("Clauseo"))
            .map_err(com("SetAuthor"))?;
        info.SetDescription(&BSTR::from(spec.description()))
            .map_err(com("SetDescription"))?;
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
        if spec.at_logon()
            && let Some(owner_sid) = starters.owner()
        {
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
                TASK_CREATE_OR_UPDATE.0,
                &VARIANT::from(SYSTEM),
                &VARIANT::default(),
                TASK_LOGON_SERVICE_ACCOUNT,
                &VARIANT::from(sddl.as_str()),
            )
            .map_err(com("ITaskFolder::RegisterTaskDefinition"))?;
    }
    Ok(())
}

/// Deletes `task`, then the folder `\Clauseo` if nothing else is in it. A task or folder that
/// isn't there is `Ok`. Elevated: `--remove --uninstall` calls it for each task, and a
/// per-machine install's `--remove` for the stop task. The set-up task's running instance is
/// ended first: deleting a task leaves its run going, which would set up a pool the removal
/// is taking down.
pub fn unregister_task(task: SystemTask) -> Result<(), TaskError> {
    let _apartment = Apartment::enter()?;
    let service = service()?;
    // SAFETY: a live service and a BSTR that outlives the call.
    let folder = match unsafe { service.GetFolder(&BSTR::from(TASK_FOLDER)) } {
        Ok(folder) => folder,
        Err(error) if missing(&error) => return Ok(()),
        Err(error) => return Err(com("ITaskService::GetFolder")(error)),
    };
    if task == SystemTask::SetUp {
        // SAFETY: a live folder and a BSTR that outlives the call.
        match unsafe { folder.GetTask(&BSTR::from(task.name())) } {
            Ok(registered) => {
                // SAFETY: a live task; flags 0.
                unsafe { registered.Stop(0) }.map_err(com("IRegisteredTask::Stop"))?;
            }
            Err(error) if missing(&error) => {}
            Err(error) => return Err(com("ITaskFolder::GetTask")(error)),
        }
    }
    // SAFETY: a live folder and a BSTR; flags 0.
    match unsafe { folder.DeleteTask(&BSTR::from(task.name()), 0) } {
        Ok(()) => {}
        Err(error) if missing(&error) => {}
        Err(error) => return Err(com("ITaskFolder::DeleteTask")(error)),
    }
    // SAFETY: as above, on the root folder.
    let root =
        unsafe { service.GetFolder(&BSTR::from("\\")) }.map_err(com("ITaskService::GetFolder"))?;
    // A folder still holding someone else's task stays; deleting it fails and is ignored.
    // SAFETY: a live root folder and a BSTR; flags 0.
    let _kept_if_not_empty = unsafe { root.DeleteFolder(&BSTR::from(FOLDER_NAME), 0) };
    Ok(())
}

/// Asks the Task Scheduler to run `\Clauseo\SandboxRefresh` now, as whoever calls it: the app
/// calls it as the lawyer, unelevated. It returns once the run is queued, not when it ends; the
/// run's outcome is its exit code (the task's last result, `RefreshExit`) and its Event Log
/// line. Blocks on COM calls: call it off the UI thread. [`TaskError::NotRegistered`] means
/// this machine has no admin copy (tier B isn't set up, or PR 2's `--adopt` hasn't run).
///
/// Where the desktop calls it (a follow-up): once the updater's new install is running, that is
/// at start-up, when the install folder's `clauseo-sandbox-setup.exe` has a SHA-256 other than
/// the record's `AdminCopySha256` (both readable by the lawyer); the task's logon trigger is
/// the fallback. A run with nothing new to copy ends at once with `Done`.
pub fn request_admin_refresh() -> Result<(), TaskError> {
    run_task(SystemTask::Refresh)
}

/// Asks the Task Scheduler to run `\Clauseo\SandboxStop` now, as whoever calls it (the app,
/// as the lawyer, unelevated): whether the run was queued. A failure is logged with its cause;
/// `false` with nothing else to do, as the task is the only way to stop a pool user's process
/// that refuses the runner. The run stops every process of the pool's users and marks them for
/// a profile reset (`setup::stop`); its outcome is its exit code (`StopExit`) and its Event Log
/// line. Blocks on COM calls: call it off the UI thread.
///
/// Who calls it (a follow-up, with the runner's sweeps of #649): the runner, when a pool user's
/// process survived its sweep, and the desktop once at start-up after an unclean session.
pub fn request_pool_stop() -> bool {
    match run_task(SystemTask::Stop) {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(%error, "the sandbox's stop task couldn't be started");
            false
        }
    }
}

/// Gives the set-up task `\Clauseo\SandboxSetUp` its per-machine descriptor again with
/// `group` refused first (`Starters::Interactive`), through
/// `IRegisteredTask::SetSecurityDescriptor` rather than a new registration, so a run in progress
/// (the set-up that made the group) goes on.
/// A per-machine set-up calls it, as SYSTEM or elevated, once the record names the group, and
/// every re-run again. [`TaskError::NotRegistered`] when the install has no set-up task.
pub fn deny_set_up_to(group: &Sid) -> Result<(), TaskError> {
    set_starters_in(
        FOLDER_NAME,
        SystemTask::SetUp,
        &Starters::Interactive {
            denied: Some(group.clone()),
        },
    )
}

/// [`deny_set_up_to`]'s descriptor change for any task and folder, for the tests' folder.
pub(super) fn set_starters_in(
    folder_name: &str,
    task: SystemTask,
    starters: &Starters,
) -> Result<(), TaskError> {
    let sddl = starters.sddl();
    with_task_in(folder_name, task, |task| {
        // SAFETY: a live task and a BSTR that outlives the call; flags 0.
        unsafe { task.SetSecurityDescriptor(&BSTR::from(sddl.as_str()), 0) }
            .map_err(com("IRegisteredTask::SetSecurityDescriptor"))
    })
}

/// Runs `task` now, as whoever calls it, through the read-and-execute entry its descriptor gives
/// the caller (`FRFX`, `rules::Starters`): the Task Scheduler starts a task only for a caller
/// with execute access, and refused the owner holding `FR` alone (CI run 42416074600). The app
/// starts the set-up task through it (`windows::setup_task`).
pub(crate) fn run_task(task: SystemTask) -> Result<(), TaskError> {
    with_task(task, |task| {
        // SAFETY: a live task and an empty VARIANT: no parameters.
        unsafe { task.Run(&VARIANT::default()) }.map_err(com("IRegisteredTask::Run"))?;
        Ok(())
    })
}

/// The refresh task's last run: its last result and last run time
/// (`IRegisteredTask::LastTaskResult`, `LastRunTime`), read as whoever calls it, through the
/// path [`request_admin_refresh`] takes; the desktop logs it at start-up (`last_run.rs`).
/// [`TaskError::NotRegistered`] as there. Blocks on COM calls: call it off the UI thread.
pub fn last_refresh_run() -> Result<LastRun, TaskError> {
    with_task(SystemTask::Refresh, |task| {
        // SAFETY: a live task; both read a value into memory the call owns.
        let code =
            unsafe { task.LastTaskResult() }.map_err(com("IRegisteredTask::LastTaskResult"))?;
        // SAFETY: as above.
        let at = unsafe { task.LastRunTime() }.map_err(com("IRegisteredTask::LastRunTime"))?;
        Ok(last_run(code, at))
    })
}

/// Runs `then` on the registered `task`, inside a COM apartment that outlives it.
pub(super) fn with_task<T>(
    task: SystemTask,
    then: impl FnOnce(&IRegisteredTask) -> Result<T, TaskError>,
) -> Result<T, TaskError> {
    with_task_in(FOLDER_NAME, task, then)
}

/// [`with_task`] in the folder `\<folder_name>`.
fn with_task_in<T>(
    folder_name: &str,
    task: SystemTask,
    then: impl FnOnce(&IRegisteredTask) -> Result<T, TaskError>,
) -> Result<T, TaskError> {
    let _apartment = Apartment::enter()?;
    let service = service()?;
    let path = format!("\\{folder_name}\\{}", task.name());
    // SAFETY: a live service and a BSTR that outlives the call.
    let root =
        unsafe { service.GetFolder(&BSTR::from("\\")) }.map_err(com("ITaskService::GetFolder"))?;
    // SAFETY: a live root folder and a BSTR.
    let task = match unsafe { root.GetTask(&BSTR::from(path.as_str())) } {
        Ok(task) => task,
        Err(error) if missing(&error) => return Err(TaskError::NotRegistered),
        Err(error) => return Err(com("ITaskFolder::GetTask")(error)),
    };
    then(&task)
}
