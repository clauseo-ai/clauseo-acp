//! The SYSTEM tasks `\Clauseo\SandboxRefresh` (ADR 0049, "Admin copy refresh") and
//! `\Clauseo\SandboxStop` ("Stopping the pool's processes"), through the Task Scheduler's COM
//! API (`ITaskService`), never `schtasks` text:
//!
//! - [`register_task`], which `--adopt` calls elevated for each of [`TASKS`](super::TASKS) per
//!   user, and a per-machine install's set-up for the stop task (`late_steps::stop_task`): the task
//!   runs as LOCAL SYSTEM, starts
//!   `<FOLDERID_ProgramFiles>\Clauseo\Sandbox\clauseo-sandbox-setup.exe` with its one argument
//!   (`--refresh`, `--stop`) and nothing else, on demand (the refresh also at the record owner's
//!   logon), needs no network, stops after its limit (5 minutes, 2 minutes), and never runs twice
//!   at once. Its descriptor ([`task_sddl`](super::task_sddl)) is owned by Administrators and lets
//!   SYSTEM and Administrators do anything and the owner read and execute it (`FRFX`), which
//!   starts it and ends a running one, nothing more; the folder `\Clauseo` gets the same owner
//!   and DACL, made with them, or set again when an administrator made it; one anyone else made
//!   is deleted with what is in it and made again (`task_folder.rs`).
//! - [`unregister_task`] and [`remove_staging`], which `--remove --uninstall` calls.
//! - [`request_admin_refresh`], which the app calls as the lawyer, unelevated, after an update,
//!   and [`request_pool_stop`]: each asks the Task Scheduler to run its task
//!   (`IRegisteredTask::Run`), which the owner's read entry allows.
#![expect(unsafe_code, reason = "FFI to COM and the Task Scheduler's interfaces")]

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, RPC_E_CHANGED_MODE, VARIANT_BOOL};
use windows::Win32::Security::OWNER_SECURITY_INFORMATION;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::TaskScheduler::{
    IExecAction, ILogonTrigger, ITaskFolder, ITaskService, TASK_ACTION_EXEC, TASK_CREATE_OR_UPDATE,
    TASK_ENUM_HIDDEN, TASK_INSTANCES_IGNORE_NEW, TASK_LOGON_SERVICE_ACCOUNT, TASK_RUNLEVEL_HIGHEST,
    TASK_TRIGGER_LOGON, TaskScheduler,
};
use windows::Win32::System::Variant::VARIANT;
use windows::core::{BSTR, HRESULT, Interface};

use super::task_folder::{Folders, prepare};
use super::windows_refresher::{EXE_NAME, SET_ASIDE_NAME, STAGED_NAME, sandbox_dir};
use super::{SystemTask, action_path, staging, task_sddl};
use crate::windows::sids::Sid;

/// The task's folder.
pub const TASK_FOLDER: &str = "\\Clauseo";
/// The folder's name under the root, for creating and deleting it.
const FOLDER_NAME: &str = "Clauseo";
/// LOCAL SYSTEM, as the task's principal.
const SYSTEM: &str = "S-1-5-18";
/// How many folders below `\Clauseo` a deletion of someone else's `\Clauseo` goes.
const FOLDER_DEPTH: usize = 8;

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
}

/// `call` failed with `error`, as a line for the log.
fn text(call: &'static str) -> impl Fn(windows::core::Error) -> String {
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
fn missing(error: &windows::core::Error) -> bool {
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

/// The folder `\Clauseo`, as `task_folder::prepare` leaves it: an administrator's is given
/// `sddl` (its owner and DACL) again; anyone else's is deleted with what is in it; a missing or
/// deleted one is made with `sddl`.
fn folder(service: &ITaskService, sddl: &str) -> Result<ITaskFolder, TaskError> {
    prepare(&mut Scheduler(service), sddl).map_err(TaskError::Folder)
}

/// The Task Scheduler's folders, for `task_folder::prepare`.
struct Scheduler<'a>(&'a ITaskService);

impl Scheduler<'_> {
    /// The root folder.
    fn root(&self) -> Result<ITaskFolder, String> {
        // SAFETY: a live service and a BSTR that outlives the call.
        unsafe { self.0.GetFolder(&BSTR::from("\\")) }.map_err(text("ITaskService::GetFolder"))
    }
}

/// Deletes every task in `folder` and each folder below it, `depth` folders down at most,
/// then the folders themselves, deepest first; `folder` itself is left.
fn empty(folder: &ITaskFolder, depth: usize) -> Result<(), String> {
    // SAFETY: a live folder; hidden tasks too.
    let tasks =
        unsafe { folder.GetTasks(TASK_ENUM_HIDDEN.0) }.map_err(text("ITaskFolder::GetTasks"))?;
    let mut names = Vec::new();
    // SAFETY: a live collection.
    let count = unsafe { tasks.Count() }.map_err(text("IRegisteredTaskCollection::Count"))?;
    for at in 1..=count {
        // SAFETY: a live collection and a 1-based index variant.
        let task = unsafe { tasks.get_Item(&VARIANT::from(at)) }
            .map_err(text("IRegisteredTaskCollection::Item"))?;
        // SAFETY: a live task.
        names.push(unsafe { task.Name() }.map_err(text("IRegisteredTask::Name"))?);
    }
    for name in names {
        // SAFETY: a live folder and a BSTR; flags 0.
        unsafe { folder.DeleteTask(&name, 0) }.map_err(text("ITaskFolder::DeleteTask"))?;
    }
    // SAFETY: a live folder; flags 0.
    let subs = unsafe { folder.GetFolders(0) }.map_err(text("ITaskFolder::GetFolders"))?;
    // SAFETY: a live collection.
    let count = unsafe { subs.Count() }.map_err(text("ITaskFolderCollection::Count"))?;
    let mut below = Vec::new();
    for at in 1..=count {
        // SAFETY: a live collection and a 1-based index variant.
        let sub = unsafe { subs.get_Item(&VARIANT::from(at)) }
            .map_err(text("ITaskFolderCollection::Item"))?;
        below.push(sub);
    }
    if !below.is_empty() && depth == 0 {
        return Err("the folder holds folders deeper than the deletion goes".to_owned());
    }
    for sub in below {
        empty(&sub, depth.saturating_sub(1))?;
        // SAFETY: a live folder.
        let name = unsafe { sub.Name() }.map_err(text("ITaskFolder::Name"))?;
        // SAFETY: a live folder and a BSTR naming its child; flags 0.
        unsafe { folder.DeleteFolder(&name, 0) }.map_err(text("ITaskFolder::DeleteFolder"))?;
    }
    Ok(())
}

impl Folders for Scheduler<'_> {
    type Folder = ITaskFolder;

    fn find(&mut self) -> Result<Option<ITaskFolder>, String> {
        // SAFETY: a live service and a BSTR that outlives the call.
        match unsafe { self.0.GetFolder(&BSTR::from(TASK_FOLDER)) } {
            Ok(folder) => Ok(Some(folder)),
            Err(error) if missing(&error) => Ok(None),
            Err(error) => Err(text("ITaskService::GetFolder")(error)),
        }
    }

    fn owner_sddl(&mut self, folder: &ITaskFolder) -> Result<String, String> {
        // SAFETY: a live folder; the owner alone.
        let sddl =
            unsafe { folder.GetSecurityDescriptor(OWNER_SECURITY_INFORMATION.0.cast_signed()) }
                .map_err(text("ITaskFolder::GetSecurityDescriptor"))?;
        Ok(sddl.to_string())
    }

    fn delete(&mut self, folder: ITaskFolder) -> Result<(), String> {
        empty(&folder, FOLDER_DEPTH)?;
        drop(folder);
        // SAFETY: a live root folder and a BSTR; flags 0.
        unsafe { self.root()?.DeleteFolder(&BSTR::from(FOLDER_NAME), 0) }
            .map_err(text("ITaskFolder::DeleteFolder"))
    }

    fn create(&mut self, sddl: &str) -> Result<ITaskFolder, String> {
        // SAFETY: a live root folder; the name and the VARIANT outlive the call.
        unsafe {
            self.root()?
                .CreateFolder(&BSTR::from(FOLDER_NAME), &VARIANT::from(sddl))
        }
        .map_err(text("ITaskFolder::CreateFolder"))
    }

    fn set(&mut self, folder: &ITaskFolder, sddl: &str) -> Result<(), String> {
        // SAFETY: a live folder and a BSTR; flags 0.
        unsafe { folder.SetSecurityDescriptor(&BSTR::from(sddl), 0) }
            .map_err(text("ITaskFolder::SetSecurityDescriptor"))
    }
}

/// Registers (or updates) the task `spec` names as the module says, for the record owner
/// `owner_sid`. Elevated: `--adopt` calls it for each task once the admin copy is locked in
/// place, before it writes the record's admin-copy values (a refresh before then exits 33,
/// changing nothing). On a per-machine install, the set-up calls it for the stop task once the
/// record names `owner_sid`; a task already there is replaced, descriptor included.
pub fn register_task(spec: SystemTask, owner_sid: &Sid) -> Result<(), TaskError> {
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
        exec.SetPath(&BSTR::from(action_path(&exe)))
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
/// per-machine install's `--remove` for the stop task.
pub fn unregister_task(task: SystemTask) -> Result<(), TaskError> {
    let _apartment = Apartment::enter()?;
    let service = service()?;
    // SAFETY: a live service and a BSTR that outlives the call.
    let folder = match unsafe { service.GetFolder(&BSTR::from(TASK_FOLDER)) } {
        Ok(folder) => folder,
        Err(error) if missing(&error) => return Ok(()),
        Err(error) => return Err(com("ITaskService::GetFolder")(error)),
    };
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

/// Deletes the refresh's staging folder (its two known files, then the folder, which is left
/// when anything else is in it). One that isn't there is `Ok`. Elevated: `--remove` calls it.
pub fn remove_staging() -> Result<(), String> {
    let staging_dir = sandbox_dir()?.join(crate::windows::admin_copy::STAGING);
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

/// Runs `task` now, as whoever calls it, through the owner's read entry: `Run` asks no access
/// of its own, and the Task Scheduler starts a task for a caller with read access.
fn run_task(task: SystemTask) -> Result<(), TaskError> {
    let _apartment = Apartment::enter()?;
    let service = service()?;
    let path = format!("{TASK_FOLDER}\\{}", task.name());
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
