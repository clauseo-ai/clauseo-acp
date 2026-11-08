//! The folder `\Clauseo` on Windows: [`Folders`] over `ITaskService`, for
//! `task_folder::prepare`, and the deletion of someone else's folder with what is in it. Split
//! from `task.rs` for its size.
#![expect(unsafe_code, reason = "FFI to the Task Scheduler's folder interfaces")]

use windows::Win32::Security::OWNER_SECURITY_INFORMATION;
use windows::Win32::System::TaskScheduler::{ITaskFolder, ITaskService, TASK_ENUM_HIDDEN};
use windows::Win32::System::Variant::VARIANT;
use windows::core::BSTR;

use super::task::{TaskError, missing, text};
use super::task_folder::{Folders, prepare};

/// How many folders below `\Clauseo` a deletion of someone else's `\Clauseo` goes.
const FOLDER_DEPTH: usize = 8;

/// The folder `\<name>` (`\Clauseo` but in the tests), as `task_folder::prepare` leaves it:
/// an administrator's is given `FOLDER_SDDL` (its owner and DACL) again; anyone else's is
/// deleted with what is in it; a missing or deleted one is made with `FOLDER_SDDL`.
pub(super) fn folder(service: &ITaskService, name: &str) -> Result<ITaskFolder, TaskError> {
    prepare(&mut Scheduler { service, name }).map_err(TaskError::Folder)
}

/// The Task Scheduler's folders, for `task_folder::prepare`: the one named `name` under the root.
struct Scheduler<'a> {
    service: &'a ITaskService,
    name: &'a str,
}

impl Scheduler<'_> {
    /// The root folder.
    fn root(&self) -> Result<ITaskFolder, String> {
        // SAFETY: a live service and a BSTR that outlives the call.
        unsafe { self.service.GetFolder(&BSTR::from("\\")) }
            .map_err(text("ITaskService::GetFolder"))
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
        let path = format!("\\{}", self.name);
        // SAFETY: a live service and a BSTR that outlives the call.
        match unsafe { self.service.GetFolder(&BSTR::from(path.as_str())) } {
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
        unsafe { self.root()?.DeleteFolder(&BSTR::from(self.name), 0) }
            .map_err(text("ITaskFolder::DeleteFolder"))
    }

    fn create(&mut self, sddl: &str) -> Result<ITaskFolder, String> {
        // SAFETY: a live root folder; the name and the VARIANT outlive the call.
        unsafe {
            self.root()?
                .CreateFolder(&BSTR::from(self.name), &VARIANT::from(sddl))
        }
        .map_err(text("ITaskFolder::CreateFolder"))
    }

    fn set(&mut self, folder: &ITaskFolder, sddl: &str) -> Result<(), String> {
        // SAFETY: a live folder and a BSTR; flags 0.
        unsafe { folder.SetSecurityDescriptor(&BSTR::from(sddl), 0) }
            .map_err(text("ITaskFolder::SetSecurityDescriptor"))
    }
}
