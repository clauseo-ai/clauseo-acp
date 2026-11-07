//! Task Scheduler's and BITS's halves of the removal (Windows), over COM in this thread.
//!
//! Task Scheduler's class object comes straight from System32's `taskschd.dll`
//! (`DllGetClassObject`), so no per-user class lookup happens. BITS is an out-of-process server,
//! created as usual.
#![expect(unsafe_code, reason = "COM calls to Task Scheduler and BITS")]

use std::ffi::c_void;
use std::io;
use std::mem::{ManuallyDrop, transmute};
use std::ptr;

use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::Networking::BackgroundIntelligentTransferService::{
    BackgroundCopyManager, IBackgroundCopyJob, IBackgroundCopyManager,
};
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoSetProxyBlanket,
    CoUninitialize, EOAC_NONE, IClassFactory, RPC_C_AUTHN_LEVEL_PKT_PRIVACY,
    RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::Win32::System::TaskScheduler::{
    IRegisteredTask, ITaskFolder, ITaskService, TASK_ENUM_HIDDEN, TaskScheduler,
};
use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_I4};
use windows::core::{BSTR, GUID, HRESULT, IUnknown, Interface, PCWSTR, s, w};

use super::super::names_user;
use super::Item;
use crate::windows::acl::object::os_error;
use crate::windows::sids::Sid;

/// How deep the task folders are walked.
const FOLDER_DEPTH: usize = 16;
/// The most BITS jobs read.
const JOBS_CAP: usize = 10_000;
/// `RPC_C_AUTHN_WINNT`: NTLM or Kerberos, as for any local COM call.
const AUTHN_WINNT: u32 = 10;
/// `RPC_C_AUTHZ_NONE`.
const AUTHZ_NONE: u32 = 0;

/// COM, initialised for this thread while it lives.
pub(super) struct Com(bool);

impl Com {
    pub(super) fn new() -> Com {
        // SAFETY: no reserved pointer; undone on drop when it took.
        Com(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances the CoInitializeEx that took, on the same thread.
            unsafe { CoUninitialize() };
        }
    }
}

/// `DllGetClassObject`: the class object for `rclsid`, and `ppv` for its `riid` interface.
type ClassObjectFn =
    unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;

/// `taskschd.dll` from System32, loaded. Its class object is code in the module, and the service
/// and the folders made from it live on after `tasks()` returns: the module is freed only when
/// this drops, after the removal's last delete.
pub(super) struct Library(HMODULE);

impl Library {
    /// Loads `taskschd.dll` from System32 alone.
    pub(super) fn load() -> io::Result<Library> {
        // SAFETY: a literal name, searched for in System32 alone; no file handle.
        let module =
            unsafe { LoadLibraryExW(w!("taskschd.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
                .map_err(|e| os_error(&e))?;
        Ok(Library(module))
    }

    /// The Task Scheduler's service, made from the module's own class object: no class is looked
    /// up in the registry, where a per-user one could stand in for it.
    fn task_service(&self) -> io::Result<ITaskService> {
        // SAFETY: a literal name in the module, which is loaded while `self` lives.
        let export = unsafe { GetProcAddress(self.0, s!("DllGetClassObject")) }
            .ok_or_else(io::Error::last_os_error)?;
        // SAFETY: the export is `DllGetClassObject`, with the signature of `ClassObjectFn`.
        let class_object: ClassObjectFn = unsafe { transmute(export) };
        let mut ppv = ptr::null_mut();
        // SAFETY: the GUIDs are live, and `ppv` is a live out-pointer.
        unsafe { class_object(&TaskScheduler, &IClassFactory::IID, &raw mut ppv) }
            .ok()
            .map_err(|e| os_error(&e))?;
        if ppv.is_null() {
            return Err(io::Error::other("no class factory"));
        }
        // SAFETY: a success hands over one reference to the factory, which this takes.
        let factory: IClassFactory = unsafe { IClassFactory::from_raw(ppv) };
        // SAFETY: a live factory, with no outer object.
        unsafe { factory.CreateInstance::<Option<&IUnknown>, ITaskService>(None) }
            .map_err(|e| os_error(&e))
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: loaded by `load` and freed once, here.
        let _freed = unsafe { FreeLibrary(self.0) };
    }
}

/// The tasks whose principal or author is `me` (or `account`), in every folder this user can
/// read: the root must be readable, a folder below it that isn't is passed over. The service
/// comes from `library`'s class object (see the module doc).
pub(super) fn tasks(me: &Sid, account: &str, library: &Library) -> io::Result<Vec<Item>> {
    let service = library.task_service()?;
    let none = VARIANT::default();
    // SAFETY: empty variants: this computer, as this user.
    unsafe { service.Connect(&none, &none, &none, &none) }.map_err(|e| os_error(&e))?;
    // SAFETY: a live BSTR naming the root folder.
    let root = unsafe { service.GetFolder(&BSTR::from("\\")) }.map_err(|e| os_error(&e))?;
    let mut items = Vec::new();
    folder_tasks(&root, me, account, &mut items)?;
    let mut folders = vec![(root, 0usize)];
    while let Some((folder, depth)) = folders.pop() {
        if depth > 0 && folder_tasks(&folder, me, account, &mut items).is_err() {
            continue;
        }
        if depth >= FOLDER_DEPTH {
            continue;
        }
        // SAFETY: a live folder; no flags.
        let Ok(subs) = (unsafe { folder.GetFolders(0) }) else {
            continue;
        };
        // SAFETY: a live collection.
        let count = unsafe { subs.Count() }.unwrap_or(0);
        for at in 1..=count {
            // SAFETY: a live collection and a 1-based index variant.
            if let Ok(sub) = unsafe { subs.get_Item(&index(at)) } {
                folders.push((sub, depth + 1));
            }
        }
    }
    Ok(items)
}

/// `folder`'s tasks that are `me`'s, hidden ones included.
fn folder_tasks(
    folder: &ITaskFolder,
    me: &Sid,
    account: &str,
    items: &mut Vec<Item>,
) -> io::Result<()> {
    // SAFETY: a live folder.
    let tasks = unsafe { folder.GetTasks(TASK_ENUM_HIDDEN.0) }.map_err(|e| os_error(&e))?;
    // SAFETY: a live collection.
    let count = unsafe { tasks.Count() }.map_err(|e| os_error(&e))?;
    for at in 1..=count {
        // SAFETY: a live collection and a 1-based index variant.
        let Ok(task) = (unsafe { tasks.get_Item(&index(at)) }) else {
            continue;
        };
        if !is_mine(&task, me, account) {
            continue;
        }
        // SAFETY: a live task.
        if let Ok(name) = unsafe { task.Name() } {
            items.push(Item::Task {
                folder: folder.clone(),
                name,
            });
        }
    }
    Ok(())
}

/// Whether the task's principal or author names `me`; one whose definition can't be read isn't.
fn is_mine(task: &IRegisteredTask, me: &Sid, account: &str) -> bool {
    // SAFETY: a live task.
    let Ok(definition) = (unsafe { task.Definition() }) else {
        return false;
    };
    let mut named = Vec::new();
    // SAFETY: a live definition; each BSTR is a live out-pointer.
    unsafe {
        if let Ok(principal) = definition.Principal() {
            let mut user = BSTR::new();
            if principal.UserId(&raw mut user).is_ok() {
                named.push(user.to_string());
            }
        }
        if let Ok(info) = definition.RegistrationInfo() {
            let mut author = BSTR::new();
            if info.Author(&raw mut author).is_ok() {
                named.push(author.to_string());
            }
        }
    }
    named.iter().any(|named| names_user(named, me, account))
}

/// Deletes the task `name` from `folder`.
pub(super) fn delete_task(folder: &ITaskFolder, name: &BSTR) -> io::Result<()> {
    // SAFETY: a live folder and name; no flags.
    unsafe { folder.DeleteTask(name, 0) }.map_err(|e| os_error(&e))
}

/// Lets the server impersonate this user on calls through `proxy`, and returns it. COM's default
/// level is identify, with which BITS can't act as a job's owner: `Cancel` fails with
/// `ERROR_NOT_LOGGED_ON`. A proxy reached through another (the enumerator, each job) starts with
/// the default again, so each is set before its first call. The process's own COM security
/// (`CoInitializeSecurity`) is left alone: Task Scheduler works with it as it is.
pub(super) fn impersonating<T: Interface>(proxy: T) -> io::Result<T> {
    let raw = proxy.as_raw();
    // SAFETY: `raw` is `proxy`'s live pointer, borrowed for the call while `proxy` lives.
    let unknown = unsafe { IUnknown::from_raw_borrowed(&raw) }
        .ok_or_else(|| io::Error::other("a null COM proxy"))?;
    // SAFETY: a live proxy; no server principal and no auth identity: this process's own.
    unsafe {
        CoSetProxyBlanket(
            unknown,
            AUTHN_WINNT,
            AUTHZ_NONE,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_PKT_PRIVACY,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )
    }
    .map_err(|e| os_error(&e))?;
    Ok(proxy)
}

/// BITS's manager, impersonating this user.
pub(super) fn manager() -> io::Result<IBackgroundCopyManager> {
    // SAFETY: a plain out-of-process creation of BITS's manager.
    let manager: IBackgroundCopyManager =
        unsafe { CoCreateInstance(&BackgroundCopyManager, None, CLSCTX_LOCAL_SERVER) }
            .map_err(|e| os_error(&e))?;
    impersonating(manager)
}

/// This user's BITS jobs (`EnumJobs(0)`: the caller's own), each impersonating this user.
pub(super) fn bits_jobs() -> io::Result<Vec<Item>> {
    let manager = manager()?;
    // SAFETY: a live manager; 0 lists this user's jobs alone.
    let jobs = unsafe { manager.EnumJobs(0) }.map_err(|e| os_error(&e))?;
    let jobs = impersonating(jobs)?;
    let mut items = Vec::new();
    while items.len() < JOBS_CAP {
        let mut one: [Option<IBackgroundCopyJob>; 1] = [None];
        let mut fetched = 0u32;
        // SAFETY: a live enumerator; `one` and `fetched` are live out-pointers.
        unsafe { jobs.Next(&mut one, &raw mut fetched) }.map_err(|e| os_error(&e))?;
        let [job] = one;
        match job {
            Some(job) if fetched == 1 => items.push(Item::Bits(impersonating(job)?)),
            _ => break,
        }
    }
    Ok(items)
}

/// Cancels `job`, which removes it.
pub(super) fn cancel_job(job: &IBackgroundCopyJob) -> io::Result<()> {
    // SAFETY: a live job.
    unsafe { job.Cancel() }.map_err(|e| os_error(&e))
}

/// A `VT_I4` variant holding `at`: a collection's 1-based index.
fn index(at: i32) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_I4,
                Anonymous: VARIANT_0_0_0 { lVal: at },
                ..VARIANT_0_0::default()
            }),
        },
    }
}
