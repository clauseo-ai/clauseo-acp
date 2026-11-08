//! The pool's tasks' running instances, stopped once the walk deleted those tasks and the BITS
//! step cancelled the pool's jobs (`sweep.rs`): deleting a task ends nothing, so an instance a
//! pool user started (at its trigger, or on demand) would run on after the sweep. Stopping them
//! before the delete would leave one started in between, so the order is delete, cancel, stop.
//! The running instances are listed once (`ITaskService::GetRunningTasks`, hidden ones
//! included), and each whose task's path is one of the pool tasks' is stopped
//! (`IRunningTask::Stop`). The Task Scheduler stops it; the sweep opens no process. A process
//! an instance started may outlive it: the final stop after the sweep ends it
//! (`runner::marker`).
//!
//! An instance whose path can't be read may be a pool task's: it counts as not stopped, as does
//! a list that can't be read, a pool task whose folder's path couldn't be read, and a stop that
//! fails. Each is 56.

use super::{Failed, Sweeper, Swept};

/// Stops every running instance of the pool's tasks the walk found (`swept.pool_paths`),
/// counted in `swept`. With none found, the instances aren't listed.
pub(super) fn stop_instances<S: Sweeper>(fs: &mut S, swept: &mut Swept) {
    let mut paths = Vec::new();
    for path in std::mem::take(&mut swept.pool_paths) {
        match path {
            Ok(path) => paths.push(path),
            Err(why) => not_stopped(swept, why),
        }
    }
    if paths.is_empty() {
        return;
    }
    let running = match fs.running_tasks() {
        Ok(running) => running,
        Err(why) => return not_stopped(swept, why),
    };
    for (at, instance) in running.into_iter().enumerate() {
        match instance {
            Ok(of) if paths.iter().any(|path| same_path(&of, path)) => match fs.stop_running(at) {
                Ok(()) => swept.instances_stopped += 1,
                Err(why) => not_stopped(swept, why),
            },
            Ok(_) => {}
            Err(why) => not_stopped(swept, why),
        }
    }
}

/// One instance not stopped, or one that may be a pool task's and wasn't, because of `why`.
fn not_stopped(swept: &mut Swept, why: Failed) {
    swept.instances_not_stopped += 1;
    swept.note(why);
}

/// The task `name`'s path in the folder whose path is `folder`: `\name` in the root, else
/// `\A\B\name`, as `IRunningTask::Path` gives it.
pub(super) fn task_path(folder: &str, name: &str) -> String {
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
