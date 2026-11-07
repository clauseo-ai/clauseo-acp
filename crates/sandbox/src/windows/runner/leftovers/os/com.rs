//! Task Scheduler's and BITS's halves of the removal (Windows), over COM in this thread.
#![expect(unsafe_code, reason = "COM calls to Task Scheduler and BITS")]

use std::io;
use std::mem::ManuallyDrop;

use windows::Win32::Networking::BackgroundIntelligentTransferService::{
    BackgroundCopyManager, IBackgroundCopyJob, IBackgroundCopyManager,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED, CoCreateInstance,
    CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::TaskScheduler::{
    IRegisteredTask, ITaskFolder, ITaskService, TASK_ENUM_HIDDEN, TaskScheduler,
};
use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_I4};
use windows::core::BSTR;

use super::super::names_user;
use super::Item;
use crate::windows::acl::object::os_error;
use crate::windows::sids::Sid;

/// How deep the task folders are walked.
const FOLDER_DEPTH: usize = 16;
/// The most BITS jobs read.
const JOBS_CAP: usize = 10_000;

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

/// The tasks whose principal or author is `me` (or `account`), in every folder this user can
/// read: the root must be readable, a folder below it that isn't is passed over.
pub(super) fn tasks(me: &Sid, account: &str) -> io::Result<Vec<Item>> {
    // SAFETY: a plain in-process creation of the Task Scheduler's class.
    let service: ITaskService =
        unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER) }
            .map_err(|e| os_error(&e))?;
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

/// This user's BITS jobs (`EnumJobs(0)`: the caller's own).
pub(super) fn bits_jobs() -> io::Result<Vec<Item>> {
    // SAFETY: a plain out-of-process creation of BITS's manager.
    let manager: IBackgroundCopyManager =
        unsafe { CoCreateInstance(&BackgroundCopyManager, None, CLSCTX_LOCAL_SERVER) }
            .map_err(|e| os_error(&e))?;
    // SAFETY: a live manager; 0 lists this user's jobs alone.
    let jobs = unsafe { manager.EnumJobs(0) }.map_err(|e| os_error(&e))?;
    let mut items = Vec::new();
    while items.len() < JOBS_CAP {
        let mut one: [Option<IBackgroundCopyJob>; 1] = [None];
        let mut fetched = 0u32;
        // SAFETY: a live enumerator; `one` and `fetched` are live out-pointers.
        unsafe { jobs.Next(&mut one, &raw mut fetched) }.map_err(|e| os_error(&e))?;
        let [job] = one;
        match job {
            Some(job) if fetched == 1 => items.push(Item::Bits(job)),
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
