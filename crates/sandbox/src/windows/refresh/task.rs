//! The SYSTEM tasks `\Clauseo\SandboxRefresh` (ADR 0049, "Admin copy refresh"),
//! `\Clauseo\SandboxStop` ("Stopping the pool's processes") and `\Clauseo\SandboxSweep` ("The
//! pool's tasks and BITS jobs"), through the Task Scheduler's COM API (`ITaskService`), never
//! `schtasks` text:
//!
//! - [`register_task`](super::register_task) (`task_register.rs`), which `--adopt` calls
//!   elevated for each of [`TASKS`](super::TASKS) per user, a per-user set-up's repair for each
//!   of [`USER_TASKS`](super::USER_TASKS), and a per-machine install's set-up for the stop and
//!   sweep tasks
//!   ([`POOL_TASKS`](super::POOL_TASKS), `late_steps::stop_task`): the task runs as LOCAL
//!   SYSTEM, starts `<FOLDERID_ProgramFiles>\Clauseo\Sandbox\clauseo-sandbox-setup.exe` with its
//!   one argument (`--refresh`, `--stop`, `--sweep-tasks`) and nothing else, on demand (the
//!   refresh also at the record owner's logon), needs no network, stops after its limit (5
//!   minutes, 2 minutes, 2 minutes), and never runs twice at once: a sweep asked for while one
//!   runs is queued after it, any other run is dropped. Its descriptor
//!   ([`task_sddl`](super::task_sddl)) is owned by Administrators and lets SYSTEM and
//!   Administrators do anything and the owner read and execute it (`FRFX`), which
//!   starts it and ends a running one, nothing more. Its definition carries the same
//!   descriptor (the XML's `<SecurityDescriptor>`), and it is registered with
//!   `TASK_DONT_ADD_PRINCIPAL_ACE` ([`REGISTER_FLAGS`](super::REGISTER_FLAGS)), so Windows adds
//!   no entry of its own.
//!   The folder `\Clauseo` gets the same owner and DACL, made with them, or set again when an
//!   administrator made it; one anyone else made is deleted with what is in it and made again
//!   (`task_folder.rs`).
//! - [`read_task`]: a registered task as `task_identity.rs` compares it, its principal, actions,
//!   triggers and descriptor, for a per-user set-up's repair and the readiness check
//!   (`task_read.rs`), with [`read_tasks`] for several over one connection.
//! - [`unregister_task`], which `--remove --uninstall` calls (it deletes staging itself,
//!   taking it back if need be: `setup::admin_remove`).
//! - [`request_admin_refresh`], which the app calls as the lawyer, unelevated, after an update,
//!   [`request_pool_stop`], and [`request_pool_sweep`], which the outer runner calls as the
//!   lawyer at every session's end: each asks the Task Scheduler to run its task
//!   (`IRegisteredTask::Run`), which the owner's read-and-execute entry (`FRFX`) allows. Nothing
//!   waits for a run here: the record's `StopSequence` and `SweepSequence` say what completed.
#![expect(unsafe_code, reason = "FFI to COM and the Task Scheduler's interfaces")]

use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, VARIANT_BOOL};
use windows::Win32::Security::{DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::TaskScheduler::{
    IExecAction, ILogonTrigger, IRegisteredTask, ITaskFolder, ITaskService, TASK_ACTION_EXEC,
    TASK_ACTION_TYPE, TASK_ENUM_HIDDEN, TASK_INSTANCES_POLICY, TASK_RUNLEVEL_TYPE,
    TASK_TRIGGER_LOGON, TASK_TRIGGER_TYPE2, TaskScheduler,
};
use windows::Win32::System::Variant::{VARIANT, VT_BSTR};
use windows::core::{BSTR, Interface};

use super::SystemTask;
use super::last_run::{LastRun, last_run};
use super::staging::sddl_bytes;
use super::task_folder::{Folders, prepare};
use super::task_identity::{
    OnBattery, TaskAction, TaskIdentity, TaskSettings, TaskTrigger, not_found, principal_sid,
};
use crate::windows::setup::sweep::windows_sweeper::lookup_account;

/// The task's folder.
pub const TASK_FOLDER: &str = "\\Clauseo";
/// The folder's name under the root, for creating and deleting it.
const FOLDER_NAME: &str = "Clauseo";
/// LOCAL SYSTEM, as the task's principal.
pub(super) const SYSTEM: &str = "S-1-5-18";
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
pub(super) fn com(call: &'static str) -> impl Fn(windows::core::Error) -> TaskError {
    move |error| TaskError::Com {
        call,
        hresult: error.code().0.cast_unsigned(),
    }
}

/// Whether `error` says the folder or task isn't there: `ERROR_FILE_NOT_FOUND`, or
/// `ERROR_PATH_NOT_FOUND` for a task whose folder `\Clauseo` is gone (`task_identity::not_found`).
fn missing(error: &windows::core::Error) -> bool {
    not_found(error.code().0.cast_unsigned())
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
pub(super) fn folder(service: &ITaskService, sddl: &str) -> Result<ITaskFolder, TaskError> {
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

/// Deletes `task`, then the folder `\Clauseo` if nothing else is in it. A task or folder that
/// isn't there is `Ok`. Elevated: `--remove --uninstall` calls it for each task, and a
/// per-machine install's `--remove` for the stop and sweep tasks.
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

/// Asks the Task Scheduler to run `\Clauseo\SandboxStop` now, as whoever calls it (the outer
/// runner or the app, as the lawyer, unelevated): whether the run was queued. A failure is
/// logged with its cause; `false` with nothing else to do, as the task is the only way to stop a
/// pool user's process that refuses the runner. The run stops every process of the pool's users
/// and marks them for a profile reset (`setup::stop`); its outcome is its exit code (`StopExit`),
/// its Event Log line and, when it completed, the record's `StopSequence`. Blocks on COM calls:
/// call it off the UI thread.
///
/// Who calls it: the outer runner, when a sweep of its session found an escapee
/// (`runner::end`), or a logon it refuses for a marker awaiting a stop (`runner::unclean`); and
/// the desktop once at start-up for an unclean marker (`runner::start_up_requests`).
pub fn request_pool_stop() -> bool {
    match run_task(SystemTask::Stop) {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(%error, "the sandbox's stop task couldn't be started");
            false
        }
    }
}

/// Asks the Task Scheduler to run `\Clauseo\SandboxSweep` now, as whoever calls it: the outer
/// runner, as the lawyer, at every session's end, and a launch or the desktop's start for a marker
/// still waiting for a sweep. The task deletes the pool users' scheduled tasks and cancels their
/// BITS jobs, and kills nothing (`setup::sweep`). A run asked for while one runs is queued after
/// it, so the run asked for begins after the ask. Whether it ended clean is read from the record's
/// `SweepSequence`, never from the task's result (`runner::marker`). Blocks on COM calls.
pub fn request_pool_sweep() -> Result<(), TaskError> {
    run_task(SystemTask::Sweep)
}

/// Runs `task` now, as whoever calls it, through the owner's read-and-execute entry (`FRFX`,
/// `rules::task_sddl`): the Task Scheduler starts a task only for a caller with execute access,
/// and refused the owner holding `FR` alone (CI run 42416074600).
fn run_task(task: SystemTask) -> Result<(), TaskError> {
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
fn with_task<T>(
    task: SystemTask,
    then: impl FnOnce(&IRegisteredTask) -> Result<T, TaskError>,
) -> Result<T, TaskError> {
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
    then(&task)
}

/// One task as [`read_tasks`] reads it: `None` when it isn't registered.
pub type TaskRead = Result<Option<TaskIdentity>, TaskError>;

/// `task` as registered in `\Clauseo`, `None` when it isn't, read as whoever calls it.
pub fn read_task(task: SystemTask) -> Result<Option<TaskIdentity>, TaskError> {
    read_tasks(&[task])?
        .pop()
        .unwrap_or(Err(TaskError::NotRegistered))
}

/// Each of `tasks` as [`read_task`] reads it, over one connection to the Task Scheduler: the
/// readiness check reads the stop and sweep tasks together. `Err` when there is no connection.
pub fn read_tasks(tasks: &[SystemTask]) -> Result<Vec<TaskRead>, TaskError> {
    let _apartment = Apartment::enter()?;
    let service = service()?;
    // SAFETY: a live service and a BSTR that outlives the call.
    let root =
        unsafe { service.GetFolder(&BSTR::from("\\")) }.map_err(com("ITaskService::GetFolder"))?;
    let read = |task: &SystemTask| {
        let path = format!("{TASK_FOLDER}\\{}", task.name());
        // SAFETY: a live root folder and a BSTR.
        match unsafe { root.GetTask(&BSTR::from(path.as_str())) } {
            Ok(task) => identity(&task).map(Some),
            Err(error) if missing(&error) => Ok(None),
            Err(error) => Err(com("ITaskFolder::GetTask")(error)),
        }
    };
    Ok(tasks.iter().map(read).collect())
}

/// The value `read` writes through its out-pointer; `call` names it when it fails.
fn out<T: Default>(
    call: &'static str,
    read: impl FnOnce(*mut T) -> windows::core::Result<()>,
) -> Result<T, TaskError> {
    let mut value = T::default();
    read(&raw mut value).map_err(com(call))?;
    Ok(value)
}

/// What makes `task` the set-up's, read from its definition, its state and its descriptor; the
/// principal resolved to a SID (`task_identity::principal_sid`, `LookupAccountNameW`).
fn identity(task: &IRegisteredTask) -> Result<TaskIdentity, TaskError> {
    // SAFETY: each call is on a live interface the one before returned, each out-pointer live
    // for its call (`out`); the collections are read by 1-based index within their counts; the
    // descriptor's VARIANT is read as a string only when its type says it holds one.
    unsafe {
        let definition = task
            .Definition()
            .map_err(com("IRegisteredTask::Definition"))?;
        let principal = definition.Principal().map_err(com("Principal"))?;
        let user: BSTR = out("IPrincipal::UserId", |at| principal.UserId(at))?;
        let level: TASK_RUNLEVEL_TYPE = out("RunLevel", |at| principal.RunLevel(at))?;
        let collection = definition.Actions().map_err(com("Actions"))?;
        let count = out("IActionCollection::Count", |at| collection.Count(at))?;
        let mut actions = Vec::new();
        for at in 1..=count {
            let action = collection
                .get_Item(at)
                .map_err(com("IActionCollection::Item"))?;
            let kind: TASK_ACTION_TYPE = out("IAction::Type", |at| action.Type(at))?;
            if kind != TASK_ACTION_EXEC {
                actions.push(TaskAction::Other);
                continue;
            }
            let exec: IExecAction = action.cast().map_err(com("IExecAction"))?;
            let path: BSTR = out("IExecAction::Path", |at| exec.Path(at))?;
            let arguments: BSTR = out("IExecAction::Arguments", |at| exec.Arguments(at))?;
            actions.push(TaskAction::Exec {
                path: path.to_string(),
                arguments: arguments.to_string(),
            });
        }
        let collection = definition.Triggers().map_err(com("Triggers"))?;
        let count = out("ITriggerCollection::Count", |at| collection.Count(at))?;
        let mut triggers = Vec::new();
        for at in 1..=count {
            let trigger = collection
                .get_Item(at)
                .map_err(com("ITriggerCollection::Item"))?;
            let kind: TASK_TRIGGER_TYPE2 = out("ITrigger::Type", |at| trigger.Type(at))?;
            if kind != TASK_TRIGGER_LOGON {
                triggers.push(TaskTrigger::Other(kind.0));
                continue;
            }
            let logon: ILogonTrigger = trigger.cast().map_err(com("ILogonTrigger"))?;
            let user: BSTR = out("ILogonTrigger::UserId", |at| logon.UserId(at))?;
            // By SID, as the principal: Windows may write the user back as `DOMAIN\user`.
            let lookup = |name: &str| lookup_account(name).ok();
            triggers.push(TaskTrigger::Logon(principal_sid(&user.to_string(), lookup)));
        }
        let settings = definition.Settings().map_err(com("Settings"))?;
        let flag =
            |call: &'static str, read: &dyn Fn(*mut VARIANT_BOOL) -> windows::core::Result<()>| {
                out(call, read).map(VARIANT_BOOL::as_bool)
            };
        let instances: TASK_INSTANCES_POLICY =
            out("MultipleInstances", |at| settings.MultipleInstances(at))?;
        let settings = TaskSettings {
            enabled: task
                .Enabled()
                .map_err(com("IRegisteredTask::Enabled"))?
                .as_bool(),
            demand_start: flag("AllowDemandStart", &|at| settings.AllowDemandStart(at))?,
            on_battery: OnBattery::of(
                flag("DisallowStartIfOnBatteries", &|at| {
                    settings.DisallowStartIfOnBatteries(at)
                })?,
                flag("StopIfGoingOnBatteries", &|at| {
                    settings.StopIfGoingOnBatteries(at)
                })?,
            ),
            instances: instances.0,
        };
        let info = (OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION).0;
        let sddl = task
            .GetSecurityDescriptor(info.cast_signed())
            .map_err(com("IRegisteredTask::GetSecurityDescriptor"))?;
        let descriptor = sddl_bytes(&sddl.to_string()).map_err(TaskError::Path)?;
        let info = definition
            .RegistrationInfo()
            .map_err(com("RegistrationInfo"))?;
        let carried: VARIANT = out("IRegistrationInfo::SecurityDescriptor", |at| {
            info.SecurityDescriptor(at)
        })?;
        // A string only: an empty VARIANT is a definition that carries no descriptor.
        let carried = (carried.vt() == VT_BSTR)
            .then(|| carried.Anonymous.Anonymous.Anonymous.bstrVal.to_string());
        let definition_descriptor = carried
            .filter(|text| !text.trim().is_empty())
            .and_then(|text| sddl_bytes(&text).ok());
        let lookup = |name: &str| lookup_account(name).ok();
        Ok(TaskIdentity {
            principal: principal_sid(&user.to_string(), lookup),
            run_level: level.0,
            actions,
            triggers,
            settings,
            descriptor,
            definition_descriptor,
        })
    }
}
