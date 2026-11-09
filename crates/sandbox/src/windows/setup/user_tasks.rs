//! A per-user install's SYSTEM tasks `\Clauseo\SandboxStop` and `\Clauseo\SandboxSweep`, as
//! the set-up keeps them (`late_steps::stop_task`). `--adopt` registers them when the installer
//! runs; nothing else did, so a task deleted or changed later stayed so, and every second launch
//! was refused for `AwaitingSweep` while Settings said ready. The app's repair runs this
//! set-up, elevated through the admin copy, which now puts them back:
//!
//! 1. the program they run, the admin copy, must pass `admin_copy::check_chain` with every
//!    folder above it up to the volume root (`Machine::task_exe`). A SYSTEM task running a file
//!    the lawyer could change would run the lawyer's code as SYSTEM, so a chain that fails
//!    registers nothing and fails the set-up with the reason;
//! 2. each task is read back and compared by identity (`task_identity::check`): the principal,
//!    the program, its argument, the triggers and the descriptor. The same task is left; a
//!    missing one is registered; one of the name that differs in any of them is deleted, then
//!    registered, in this elevated process (`Machine::replace_task`), by `--adopt`'s own
//!    registration (`refresh::register_task`).
//!
//! Any failure is [`Step::StopTask`]. A per-machine install's set-up replaces both tasks on
//! every run instead, as before (`Machine::register_stop_task`).

use super::create::Failed;
use super::machine::Machine;
use super::outcome::{Step, cause_of};
use crate::windows::refresh::POOL_TASKS;
use crate::windows::refresh::task_identity::{TaskCheck, check};
use crate::windows::sids::Sid;

/// Puts back each of the per-user install's stop and sweep tasks for the owner `owner` that is
/// missing or differs, as the module says.
pub(super) fn ensure(machine: &mut impl Machine, owner: &Sid) -> Result<(), Failed> {
    let failed = |why: String| {
        (
            Step::StopTask,
            format!("the tasks SandboxStop and SandboxSweep: {why}"),
        )
    };
    let exe = machine.task_exe().map_err(|error| {
        failed(format!(
            "not registered, since the program they would run as SYSTEM isn't limited to \
             administrators ({})",
            cause_of(&error)
        ))
    })?;
    for task in POOL_TASKS {
        let name = task.name();
        let expected = machine
            .task_expected(task, owner, &exe)
            .map_err(|error| failed(format!("{name}: {}", cause_of(&error))))?;
        let found = machine
            .task_found(task)
            .map_err(|error| failed(format!("{name} can't be read: {}", cause_of(&error))))?;
        let existed = match check(found.as_ref(), &expected) {
            TaskCheck::Same => continue,
            TaskCheck::Missing => {
                tracing::warn!(task = name, "a sandbox task is missing; registering it");
                false
            }
            TaskCheck::Differs(part) => {
                let part = part.name();
                tracing::warn!(
                    task = name,
                    part,
                    "a sandbox task isn't the set-up's; replacing it"
                );
                true
            }
        };
        machine
            .replace_task(task, owner, existed)
            .map_err(|error| failed(format!("{name}: {}", cause_of(&error))))?;
    }
    Ok(())
}
