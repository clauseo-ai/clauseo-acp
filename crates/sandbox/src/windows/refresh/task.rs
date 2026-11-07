//! The SYSTEM task `\Clauseo\SandboxRefresh` (ADR 0049, "Admin copy refresh"), through the Task
//! Scheduler's COM API (`ITaskService`), never `schtasks` text:
//!
//! - [`register_refresh_task`], which `--adopt` calls elevated: the task runs as LOCAL SYSTEM,
//!   starts `<FOLDERID_ProgramFiles>\Clauseo\Sandbox\clauseo-sandbox-setup.exe` with
//!   `--refresh` and nothing else, at the record owner's logon and on demand, needs no network,
//!   stops after 5 minutes, and never runs twice at once. Its descriptor
//!   ([`task_sddl`](super::task_sddl)) lets SYSTEM and Administrators do anything and the owner
//!   read and run it, nothing more; the folder `\Clauseo` gets the same.
//! - [`unregister_refresh_task`] and [`remove_staging`], which `--remove` calls.
//! - [`request_admin_refresh`], which the app calls as the lawyer, unelevated, after an update:
//!   it asks the Task Scheduler to run the task (`IRegisteredTask::Run`), which the owner's
//!   read-and-run entry allows.
#![expect(unsafe_code, reason = "FFI to COM and the Task Scheduler's interfaces")]

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, RPC_E_CHANGED_MODE, VARIANT_BOOL};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::TaskScheduler::{
    IExecAction, ILogonTrigger, ITaskFolder, ITaskService, TASK_ACTION_EXEC, TASK_CREATE_OR_UPDATE,
    TASK_INSTANCES_IGNORE_NEW, TASK_LOGON_SERVICE_ACCOUNT, TASK_RUNLEVEL_HIGHEST,
    TASK_TRIGGER_LOGON, TaskScheduler,
};
use windows::Win32::System::Variant::VARIANT;
use windows::core::{BSTR, HRESULT, Interface};

use super::windows_refresher::{EXE_NAME, SET_ASIDE_NAME, STAGED_NAME, sandbox_dir};
use super::{EXECUTION_LIMIT, REFRESH_ARG, staging, task_sddl};
use crate::windows::sids::Sid;

/// The task's folder.
pub const TASK_FOLDER: &str = "\\Clauseo";
/// The folder's name under the root, for creating and deleting it.
const FOLDER_NAME: &str = "Clauseo";
/// The task's name in it.
pub const TASK_NAME: &str = "SandboxRefresh";
/// LOCAL SYSTEM, as the task's principal.
const SYSTEM: &str = "S-1-5-18";

/// Why a task call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TaskError {
    /// The task isn't registered.
    #[error("the task {TASK_FOLDER}\\{TASK_NAME} isn't registered")]
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
}

/// `call` failed with `error`.
fn com(call: &'static str) -> impl Fn(windows::core::Error) -> TaskError {
    move |error| TaskError::Com {
        call,
        hresult: error.code().0.cast_unsigned(),
    }
}

/// Whether `error` says the folder or task isn't there.
fn missing(error: &windows::core::Error) -> bool {
    error.code() == HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0)
}

/// COM on this thread for the guard's life: uninitialised on drop when this call initialised
/// it; a thread already in another apartment keeps it and is used as it is.
struct Apartment(bool);

impl Apartment {
    /// Joins the multithreaded apartment, or uses the thread's.
    fn enter() -> Result<Apartment, TaskError> {
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
fn service() -> Result<ITaskService, TaskError> {
    // SAFETY: a registered in-process class and its interface; no outer object.
    let service: ITaskService =
        unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER) }
            .map_err(com("CoCreateInstance(TaskScheduler)"))?;
    let none = VARIANT::default();
    // SAFETY: empty VARIANT values: this machine, this process's user.
    unsafe { service.Connect(&none, &none, &none, &none) }.map_err(com("ITaskService::Connect"))?;
    Ok(service)
}

/// The folder `\Clauseo`, made with `sddl` when it is missing, given `sddl` when it is there.
fn folder(service: &ITaskService, sddl: &str) -> Result<ITaskFolder, TaskError> {
    // SAFETY: a live service and a BSTR that outlives the call.
    match unsafe { service.GetFolder(&BSTR::from(TASK_FOLDER)) } {
        Ok(folder) => {
            // SAFETY: a live folder and a BSTR; flags 0.
            unsafe { folder.SetSecurityDescriptor(&BSTR::from(sddl), 0) }
                .map_err(com("ITaskFolder::SetSecurityDescriptor"))?;
            Ok(folder)
        }
        Err(error) if missing(&error) => {
            // SAFETY: as above.
            let root = unsafe { service.GetFolder(&BSTR::from("\\")) }
                .map_err(com("ITaskService::GetFolder"))?;
            // SAFETY: a live root folder; the name and the VARIANT outlive the call.
            unsafe { root.CreateFolder(&BSTR::from(FOLDER_NAME), &VARIANT::from(sddl)) }
                .map_err(com("ITaskFolder::CreateFolder"))
        }
        Err(error) => Err(com("ITaskService::GetFolder")(error)),
    }
}

/// Registers (or updates) `\Clauseo\SandboxRefresh` as the module says, for the record owner
/// `owner_sid`. Elevated: `--adopt` calls it once the admin copy and the record are in place.
pub fn register_refresh_task(owner_sid: &Sid) -> Result<(), TaskError> {
    let exe = sandbox_dir().map_err(TaskError::Path)?.join(EXE_NAME);
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
        info.SetDescription(&BSTR::from(
            "Keeps the admin copy of Clauseo's sandbox set-up program current.",
        ))
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
            .SetExecutionTimeLimit(&BSTR::from(EXECUTION_LIMIT))
            .map_err(com("SetExecutionTimeLimit"))?;
        settings
            .SetMultipleInstances(TASK_INSTANCES_IGNORE_NEW)
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
        let trigger = task.Triggers().map_err(com("Triggers"))?;
        let trigger = trigger
            .Create(TASK_TRIGGER_LOGON)
            .map_err(com("ITriggerCollection::Create"))?;
        let logon: ILogonTrigger = trigger.cast().map_err(com("ILogonTrigger"))?;
        logon
            .SetUserId(&BSTR::from(owner_sid.as_str()))
            .map_err(com("ILogonTrigger::SetUserId"))?;
        let action = task.Actions().map_err(com("Actions"))?;
        let action = action
            .Create(TASK_ACTION_EXEC)
            .map_err(com("IActionCollection::Create"))?;
        let exec: IExecAction = action.cast().map_err(com("IExecAction"))?;
        exec.SetPath(&BSTR::from(exe.as_os_str().to_string_lossy().as_ref()))
            .map_err(com("IExecAction::SetPath"))?;
        exec.SetArguments(&BSTR::from(REFRESH_ARG))
            .map_err(com("IExecAction::SetArguments"))?;
        folder
            .RegisterTaskDefinition(
                &BSTR::from(TASK_NAME),
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

/// Deletes `\Clauseo\SandboxRefresh`, then the folder `\Clauseo` if nothing else is in it. A
/// task or folder that isn't there is `Ok`. Elevated: `--remove` calls it.
pub fn unregister_refresh_task() -> Result<(), TaskError> {
    let _apartment = Apartment::enter()?;
    let service = service()?;
    // SAFETY: a live service and a BSTR that outlives the call.
    let folder = match unsafe { service.GetFolder(&BSTR::from(TASK_FOLDER)) } {
        Ok(folder) => folder,
        Err(error) if missing(&error) => return Ok(()),
        Err(error) => return Err(com("ITaskService::GetFolder")(error)),
    };
    // SAFETY: a live folder and a BSTR; flags 0.
    match unsafe { folder.DeleteTask(&BSTR::from(TASK_NAME), 0) } {
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

/// Deletes the refresh's staging folder (its two known files, then the folder, which is left
/// when anything else is in it). One that isn't there is `Ok`. Elevated: `--remove` calls it.
pub fn remove_staging() -> Result<(), String> {
    let staging_dir = sandbox_dir()?.join("staging");
    for name in [STAGED_NAME, SET_ASIDE_NAME] {
        staging::delete(&staging_dir.join(name))?;
    }
    staging::remove_dir(&staging_dir)
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
    let _apartment = Apartment::enter()?;
    let service = service()?;
    let path = format!("{TASK_FOLDER}\\{TASK_NAME}");
    // SAFETY: a live service and a BSTR that outlives the call.
    let root =
        unsafe { service.GetFolder(&BSTR::from("\\")) }.map_err(com("ITaskService::GetFolder"))?;
    // SAFETY: a live root folder and a BSTR.
    let task = match unsafe { root.GetTask(&BSTR::from(path.as_str())) } {
        Ok(task) => task,
        Err(error) if missing(&error) => return Err(TaskError::NotRegistered),
        Err(error) => return Err(com("ITaskFolder::GetTask")(error)),
    };
    // SAFETY: a live task and an empty VARIANT: no parameters.
    unsafe { task.Run(&VARIANT::default()) }.map_err(com("IRegisteredTask::Run"))?;
    Ok(())
}
