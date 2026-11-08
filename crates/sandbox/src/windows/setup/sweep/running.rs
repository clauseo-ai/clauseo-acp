//! A pool task's running instances, stopped before the task is deleted (step 4): deleting a
//! task ends nothing, so an instance a pool user started (at its trigger, or on demand) would
//! run on after the sweep. The running instances are listed (`ITaskService::GetRunningTasks`,
//! hidden ones included), and each whose task's path is this task's is stopped
//! (`IRunningTask::Stop`). The Task Scheduler stops it; the sweep opens no process.
//!
//! An instance whose path can't be read may be this task's: it counts as not stopped, as does a
//! list that can't be read, a folder whose path can't be, and a stop that fails. Each is 56; the
//! task is still deleted, so its triggers go whatever happened to its instances.

use super::{Failed, Sweeper, Swept};

/// Stops every running instance of the task `name` in `folder`, counted in `swept`.
pub(super) fn stop_instances<S: Sweeper>(
    fs: &mut S,
    folder: &S::Folder,
    name: &str,
    swept: &mut Swept,
) {
    let path = match fs.folder_path(folder) {
        Ok(folder) => task_path(&folder, name),
        Err(why) => return not_stopped(swept, why),
    };
    let running = match fs.running_tasks() {
        Ok(running) => running,
        Err(why) => return not_stopped(swept, why),
    };
    for (at, instance) in running.into_iter().enumerate() {
        match instance {
            Ok(of) if same_path(&of, &path) => match fs.stop_running(at) {
                Ok(()) => swept.instances_stopped += 1,
                Err(why) => not_stopped(swept, why),
            },
            Ok(_) => {}
            Err(why) => not_stopped(swept, why),
        }
    }
}

/// One instance not stopped, or one that may be this task's and wasn't, because of `why`.
fn not_stopped(swept: &mut Swept, why: Failed) {
    swept.instances_not_stopped += 1;
    swept.note(why);
}

/// The task `name`'s path in the folder whose path is `folder`: `\name` in the root, else
/// `\A\B\name`, as `IRunningTask::Path` gives it.
fn task_path(folder: &str, name: &str) -> String {
    if folder.ends_with('\\') {
        format!("{folder}{name}")
    } else {
        format!("{folder}\\{name}")
    }
}

/// Whether two task paths are the same task's: the Task Scheduler's paths ignore case.
fn same_path(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}
