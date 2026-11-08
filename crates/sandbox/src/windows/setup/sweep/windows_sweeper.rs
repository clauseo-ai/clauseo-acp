//! The sweep's real [`Sweeper`]: the gate's reads (`pool_steps::windows_gate`), BITS
//! (`pool_steps::windows_bits`), the Task Scheduler's COM API (`ITaskService`, connected as this
//! process, LOCAL SYSTEM under the task; a task's owner from `GetSecurityDescriptor`), a task's
//! file (`windows_task_file.rs`), the task files' and the Task Scheduler cache's listings
//! (`windows_listing.rs`), `LookupAccountNameW`, the record's lock and values
//! (`pool_steps::windows_record`), and the Event Log.
#![expect(
    unsafe_code,
    reason = "COM calls to the Task Scheduler, LookupAccountNameW, SHGetKnownFolderPath and \
              CompareStringOrdinal"
)]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;
use std::path::PathBuf;

use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_NONE_MAPPED};
use windows::Win32::Globalization::{COMPARESTRING_RESULT, CSTR_EQUAL, CompareStringOrdinal};
use windows::Win32::Security::{
    LookupAccountNameW, OWNER_SECURITY_INFORMATION, PSID, SID_NAME_USE,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::System::TaskScheduler::{
    IRegisteredTask, IRunningTask, ITaskFolder, ITaskService, TASK_ENUM_HIDDEN, TaskScheduler,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::{FOLDERID_System, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
use windows::core::{BSTR, HSTRING, PCWSTR, PWSTR};

use super::{
    Failed, Listing, Principal, SweepExit, Sweeper, TASK_FILE_CAP, Task, TaskFile, sweep,
    windows_listing, windows_task_file,
};
use crate::windows::refresh::event_log;
use crate::windows::setup::accounts::{self, Kind};
use crate::windows::setup::pool_steps::windows_bits::{Com, WindowsBits};
use crate::windows::setup::pool_steps::windows_record::{self, RecordLock};
use crate::windows::setup::pool_steps::{Bits, Gate, Pool, Record, windows_gate};
use crate::windows::setup::preflight::sid_text;
use crate::windows::sids::Sid;

/// `--sweep-tasks`: [`sweep`] over a [`WindowsSweeper`].
pub(crate) fn run() -> SweepExit {
    sweep(&mut WindowsSweeper::new())
}

/// The machine the sweep acts on.
pub(crate) struct WindowsSweeper {
    /// The set-up lock, while held.
    lock: RecordLock,
    /// BITS, for [`Bits`].
    bits: WindowsBits,
    /// The Task Scheduler, once connected.
    service: Option<ITaskService>,
    /// The last list of running task instances, `None` where one couldn't be read.
    running: Vec<Option<IRunningTask>>,
    /// COM on this thread; the last field, so it is uninitialised after every interface above.
    _com: Com,
}

impl WindowsSweeper {
    /// The machine, with COM on this thread and nothing connected yet.
    pub(super) fn new() -> WindowsSweeper {
        WindowsSweeper {
            lock: RecordLock::default(),
            bits: WindowsBits::default(),
            service: None,
            running: Vec::new(),
            _com: Com::new(),
        }
    }

    /// The Task Scheduler, connected on first use.
    fn service(&mut self) -> Result<ITaskService, Failed> {
        if let Some(service) = &self.service {
            return Ok(service.clone());
        }
        // SAFETY: a registered in-process class and its interface; no outer object.
        let service: ITaskService =
            unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER) }
                .map_err(failed("CoCreateInstance(TaskScheduler)"))?;
        let none = VARIANT::default();
        // SAFETY: empty VARIANT values: this machine, this process's user.
        unsafe { service.Connect(&none, &none, &none, &none) }
            .map_err(failed("ITaskService::Connect"))?;
        self.service = Some(service.clone());
        Ok(service)
    }
}

/// `call` failed with an error, as a line for the log: the call and its HRESULT, no name.
fn failed(call: &'static str) -> impl Fn(windows::core::Error) -> Failed {
    move |error| format!("{call} failed ({:#010x})", error.code().0)
}

impl Gate for WindowsSweeper {
    fn is_system(&mut self) -> Result<bool, Failed> {
        windows_gate::is_system()
    }

    fn current_exe(&mut self) -> Result<PathBuf, Failed> {
        windows_gate::current_exe()
    }

    fn admin_copy(&mut self) -> Result<PathBuf, Failed> {
        windows_gate::admin_copy()
    }

    fn pool(&mut self) -> Result<Option<Pool>, Failed> {
        windows_gate::pool()
    }

    fn check_pool(&mut self, pool: &Pool) -> Result<(), Failed> {
        windows_gate::check_pool(pool)
    }
}

impl Bits for WindowsSweeper {
    fn bits_jobs(&mut self) -> Result<Vec<Option<Sid>>, Failed> {
        self.bits.bits_jobs()
    }

    fn cancel_job(&mut self, at: usize) -> Result<(), Failed> {
        self.bits.cancel_job(at)
    }
}

impl Record for WindowsSweeper {
    fn lock(&mut self) -> Result<bool, Failed> {
        self.lock.lock()
    }

    fn unlock(&mut self) {
        self.lock.unlock();
    }

    fn read_value(&mut self, name: &'static str) -> Result<Option<u64>, Failed> {
        windows_record::read_value(name)
    }

    fn write_value(&mut self, name: &'static str, value: u64) -> Result<(), Failed> {
        windows_record::write_value(name, value)
    }

    fn now(&mut self) -> u64 {
        windows_record::now()
    }
}

impl Sweeper for WindowsSweeper {
    type Folder = ITaskFolder;

    fn root(&mut self) -> Result<ITaskFolder, Failed> {
        let service = self.service()?;
        // SAFETY: a live service and a BSTR that outlives the call.
        unsafe { service.GetFolder(&BSTR::from("\\")) }.map_err(failed("ITaskService::GetFolder"))
    }

    fn subfolders(
        &mut self,
        folder: &ITaskFolder,
    ) -> Result<Vec<Result<ITaskFolder, Failed>>, Failed> {
        // SAFETY: a live folder; flags 0.
        let subs = unsafe { folder.GetFolders(0) }.map_err(failed("ITaskFolder::GetFolders"))?;
        // SAFETY: a live collection.
        let count = unsafe { subs.Count() }.map_err(failed("ITaskFolderCollection::Count"))?;
        let mut below = Vec::new();
        for at in 1..=count {
            // SAFETY: a live collection and a 1-based index variant.
            let sub = unsafe { subs.get_Item(&VARIANT::from(at)) }
                .map_err(failed("ITaskFolderCollection::Item"));
            below.push(sub);
        }
        Ok(below)
    }

    fn tasks(&mut self, folder: &ITaskFolder) -> Result<Vec<Task>, Failed> {
        // SAFETY: a live folder; hidden tasks too.
        let tasks = unsafe { folder.GetTasks(TASK_ENUM_HIDDEN.0) }
            .map_err(failed("ITaskFolder::GetTasks"))?;
        // SAFETY: a live collection.
        let count = unsafe { tasks.Count() }.map_err(failed("IRegisteredTaskCollection::Count"))?;
        let mut read = Vec::new();
        for at in 1..=count {
            // SAFETY: a live collection and a 1-based index variant.
            let task = unsafe { tasks.get_Item(&VARIANT::from(at)) }
                .map_err(failed("IRegisteredTaskCollection::Item"));
            read.push(match task {
                Ok(task) => read_task(&task),
                Err(why) => Task {
                    name: String::new(),
                    principal: Err(why),
                },
            });
        }
        Ok(read)
    }

    fn lookup_account(&mut self, name: &str) -> Result<Sid, Failed> {
        lookup_account(name)
    }

    fn task_sddl(&mut self, folder: &ITaskFolder, name: &str) -> Result<String, Failed> {
        // SAFETY: a live folder and a BSTR that outlives the call.
        let task =
            unsafe { folder.GetTask(&BSTR::from(name)) }.map_err(failed("ITaskFolder::GetTask"))?;
        let owner = i32::try_from(OWNER_SECURITY_INFORMATION.0).unwrap_or(i32::MAX);
        // SAFETY: a live task; the owner's part alone.
        let sddl = unsafe { task.GetSecurityDescriptor(owner) }
            .map_err(failed("IRegisteredTask::GetSecurityDescriptor"))?;
        Ok(sddl.to_string())
    }

    fn account_name(&mut self, sid: &Sid) -> Result<String, Failed> {
        match accounts::name_of(sid, Kind::User) {
            Ok(Some(name)) => Ok(name),
            Ok(None) => Err("no local account has the SID".to_owned()),
            Err(error) => Err(error.to_string()),
        }
    }

    fn computer_names(&mut self) -> Vec<String> {
        accounts::computer_names()
    }

    fn folder_path(&mut self, folder: &ITaskFolder) -> Result<String, Failed> {
        // SAFETY: a live folder.
        let path = unsafe { folder.Path() }.map_err(failed("ITaskFolder::Path"))?;
        Ok(path.to_string())
    }

    fn read_task_file(&mut self, parts: &[String]) -> Result<TaskFile, Failed> {
        windows_task_file::read(&system_folder()?, parts, TASK_FILE_CAP)
    }

    fn running_tasks(&mut self) -> Result<Vec<Result<String, Failed>>, Failed> {
        self.running.clear();
        let service = self.service()?;
        // SAFETY: a live service; hidden tasks' instances too.
        let list = unsafe { service.GetRunningTasks(TASK_ENUM_HIDDEN.0) }
            .map_err(failed("ITaskService::GetRunningTasks"))?;
        // SAFETY: a live collection.
        let count = unsafe { list.Count() }.map_err(failed("IRunningTaskCollection::Count"))?;
        let mut read = Vec::new();
        for at in 1..=count {
            // SAFETY: a live collection and a 1-based index variant.
            let instance = unsafe { list.get_Item(&VARIANT::from(at)) }
                .map_err(failed("IRunningTaskCollection::Item"));
            match instance {
                Ok(instance) => {
                    // SAFETY: a live instance.
                    let path = unsafe { instance.Path() }.map_err(failed("IRunningTask::Path"));
                    read.push(path.map(|path| path.to_string()));
                    self.running.push(Some(instance));
                }
                Err(why) => {
                    read.push(Err(why));
                    self.running.push(None);
                }
            }
        }
        Ok(read)
    }

    fn stop_running(&mut self, at: usize) -> Result<(), Failed> {
        let Some(Some(instance)) = self.running.get(at) else {
            return Err("the running instance isn't in the last list".to_owned());
        };
        // SAFETY: a live instance.
        unsafe { instance.Stop() }.map_err(failed("IRunningTask::Stop"))
    }

    fn same_task_path(&mut self, a: &str, b: &str) -> Result<bool, Failed> {
        same_task_path(a, b)
    }

    fn task_file_paths(&mut self) -> Result<Listing, Failed> {
        windows_listing::task_files(&system_folder()?)
    }

    fn task_cache_paths(&mut self) -> Result<Listing, Failed> {
        windows_listing::task_cache()
    }

    fn task_file_owner(&mut self, parts: &[String]) -> Result<Sid, Failed> {
        windows_task_file::owner(&system_folder()?, parts)
    }

    fn task_cache_owner(&mut self, path: &str) -> Result<Sid, Failed> {
        windows_listing::task_cache_owner(path)
    }

    fn delete_task(&mut self, folder: &ITaskFolder, name: &str) -> Result<(), Failed> {
        // SAFETY: a live folder and a BSTR that outlives the call; flags 0.
        unsafe { folder.DeleteTask(&BSTR::from(name), 0) }
            .map_err(failed("ITaskFolder::DeleteTask"))
    }

    fn log(&mut self, error: bool, line: &str) {
        event_log(error, line);
    }
}

/// Whether `a` and `b` are one task's path: `CompareStringOrdinal` with case ignored, as
/// [`Sweeper::same_task_path`] says; `Err` when the call fails (it returns 0).
pub(super) fn same_task_path(a: &str, b: &str) -> Result<bool, Failed> {
    let a: Vec<u16> = a.encode_utf16().collect();
    let b: Vec<u16> = b.encode_utf16().collect();
    // SAFETY: both slices are valid for their lengths, and the call only reads them.
    let compared = unsafe { CompareStringOrdinal(&a, &b, true) };
    match compared {
        CSTR_EQUAL => Ok(true),
        COMPARESTRING_RESULT(0) => Err("CompareStringOrdinal failed".to_owned()),
        _ => Ok(false),
    }
}

/// `task`'s name and its definition's principal, as [`Task`] says.
fn read_task(task: &IRegisteredTask) -> Task {
    // SAFETY: a live task.
    match unsafe { task.Name() } {
        Ok(name) => Task {
            name: name.to_string(),
            principal: principal(task),
        },
        Err(error) => Task {
            name: String::new(),
            principal: Err(failed("IRegisteredTask::Name")(error)),
        },
    }
}

/// `task`'s principal: its definition's `UserId` and `GroupId`.
fn principal(task: &IRegisteredTask) -> Result<Principal, Failed> {
    // SAFETY: a live task.
    let definition = unsafe { task.Definition() }.map_err(failed("IRegisteredTask::Definition"))?;
    // SAFETY: a live definition.
    let principal =
        unsafe { definition.Principal() }.map_err(failed("ITaskDefinition::Principal"))?;
    let mut user = BSTR::new();
    // SAFETY: a live principal; `user` is a live out-pointer, freed with it.
    unsafe { principal.UserId(&raw mut user) }.map_err(failed("IPrincipal::UserId"))?;
    let mut group = BSTR::new();
    // SAFETY: as above.
    unsafe { principal.GroupId(&raw mut group) }.map_err(failed("IPrincipal::GroupId"))?;
    Ok(Principal {
        user_id: user.to_string(),
        group_id: group.to_string(),
    })
}

/// The SID of the account or group `name`, as `LookupAccountNameW` resolves it on this machine
/// (a local, built-in, well-known or domain name). The error names the call, never `name`.
fn lookup_account(name: &str) -> Result<Sid, Failed> {
    let name = HSTRING::from(name);
    let (mut sid_size, mut domain_size) = (0u32, 0u32);
    let mut kind = SID_NAME_USE::default();
    // SAFETY: the name is NUL-terminated and outlives the call; no buffers ask only for the
    // sizes, written to `sid_size` and `domain_size`.
    let sized = unsafe {
        LookupAccountNameW(
            PCWSTR::null(),
            &name,
            None,
            &raw mut sid_size,
            None,
            &raw mut domain_size,
            &raw mut kind,
        )
    };
    match sized {
        Err(error) if error.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult() => {}
        Err(error) if error.code() == ERROR_NONE_MAPPED.to_hresult() => {
            return Err("LookupAccountNameW found no such account".to_owned());
        }
        Err(error) => return Err(failed("LookupAccountNameW")(error)),
        Ok(()) => return Err("LookupAccountNameW gave no size".to_owned()),
    }
    // u64 words, so the SID buffer is aligned.
    let mut sid = vec![0u64; usize::try_from(sid_size).unwrap_or(0).div_ceil(8).max(1)];
    let mut domain = vec![0u16; usize::try_from(domain_size).unwrap_or(0).max(1)];
    // SAFETY: as above; `sid` is writable for `sid_size` bytes and `domain` for `domain_size`
    // characters.
    unsafe {
        LookupAccountNameW(
            PCWSTR::null(),
            &name,
            Some(PSID(sid.as_mut_ptr().cast())),
            &raw mut sid_size,
            Some(PWSTR(domain.as_mut_ptr())),
            &raw mut domain_size,
            &raw mut kind,
        )
    }
    .map_err(failed("LookupAccountNameW"))?;
    // SAFETY: the call wrote a valid SID into `sid`, which is live.
    match unsafe { sid_text(PSID(sid.as_mut_ptr().cast())) } {
        Ok(Some(sid)) => Ok(sid),
        Ok(None) => Err("LookupAccountNameW gave no SID".to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// `%SystemRoot%\System32` as Windows knows it (`FOLDERID_System`), never from the environment.
fn system_folder() -> Result<PathBuf, Failed> {
    let folder = FOLDERID_System;
    // SAFETY: `folder` is a live GUID for the call; no token is passed (the machine's folder).
    // The string returned is owned by `Allocated` at once.
    let path = unsafe { SHGetKnownFolderPath(&raw const folder, KF_FLAG_DEFAULT, None) }
        .map_err(failed("SHGetKnownFolderPath"))?;
    let path = Allocated(path);
    // SAFETY: on success the call wrote a NUL-terminated string, alive until `path` drops.
    let units = unsafe { path.0.as_wide() };
    if units.is_empty() {
        return Err("Windows gave no system folder".to_owned());
    }
    Ok(PathBuf::from(OsString::from_wide(units)))
}

/// A string `SHGetKnownFolderPath` allocated, freed with `CoTaskMemFree` on drop.
struct Allocated(PWSTR);

impl Drop for Allocated {
    fn drop(&mut self) {
        // SAFETY: the pointer came from SHGetKnownFolderPath and is freed once, here.
        unsafe { CoTaskMemFree(Some(self.0.0.cast_const().cast())) };
    }
}
