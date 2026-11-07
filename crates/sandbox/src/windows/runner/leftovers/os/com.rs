//! Task Scheduler's half of the removal (Windows), over COM in this thread.
//!
//! Task Scheduler's class object comes straight from System32's `taskschd.dll`
//! (`DllGetClassObject`), so no per-user class lookup happens. Nothing here makes a BITS object
//! (`leftovers.rs` says why).
#![expect(unsafe_code, reason = "COM calls to Task Scheduler")]

use std::ffi::c_void;
use std::io;
use std::mem::{ManuallyDrop, transmute};
use std::ptr;

use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::System::Com::{
    COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize, IClassFactory,
};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::Win32::System::TaskScheduler::{
    IRegisteredTask, ITaskFolder, ITaskService, TASK_ENUM_HIDDEN, TaskScheduler,
};
use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_I4};
use windows::core::{BSTR, GUID, HRESULT, IUnknown, Interface, s, w};

use super::super::names_user;
use super::Item;
use crate::windows::acl::object::os_error;
use crate::windows::sids::Sid;

/// How deep the task folders are walked.
const FOLDER_DEPTH: usize = 16;

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
