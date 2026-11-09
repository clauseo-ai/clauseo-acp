//! A per-user install's SYSTEM tasks `\Clauseo\SandboxStop` and `\Clauseo\SandboxSweep`, as
//! the set-up keeps them (`late_steps::stop_task`). `--adopt` registers them when the installer
//! runs; nothing else did, so a task deleted or changed later stayed so, and every second launch
//! was refused for `AwaitingSweep` while Settings said ready. The app's repair runs this
//! set-up, elevated through the admin copy, which now puts them back:
//!
//! 1. the program they run, the admin copy, must pass the whole of `admin_copy::check`
//!    (`Machine::task_exe`): its folders up to the volume root, its folder's entries and its
//!    recorded hash. A SYSTEM task running a file the lawyer could change would run the lawyer's
//!    code as SYSTEM, so a copy that fails registers nothing;
//! 2. each task is read back and compared by meaning (`task_identity::check`): the principal's
//!    SID and run level, the program, its argument, the triggers, whether and how it runs, and
//!    the descriptor. The same task is left; a missing or changed one, or one this process may
//!    not read, is registered, or updated in place, by `--adopt`'s own registration, with the
//!    path step 1 checked (`Machine::replace_task`, `refresh::register_task_at`).
//!
//! None of this fails the set-up or rolls the pool back: a task that can't be put back is
//! logged and named in the report (`SetupReport::tasks`), the other task is still tried, and the
//! readiness check then says not ready, so the next repair tries again. A per-machine install's
//! set-up replaces both tasks on every run instead, as before (`Machine::register_stop_task`).

use super::machine::Machine;
use super::outcome::cause_of;
use crate::windows::refresh::POOL_TASKS;
use crate::windows::refresh::task_identity::{TaskCheck, check};
use crate::windows::sids::Sid;

/// Puts back each of the per-user install's stop and sweep tasks for the owner `owner` that is
/// missing or changed, as the module says: `Err` names what couldn't be put back, and why.
pub(super) fn ensure(machine: &mut impl Machine, owner: &Sid) -> Result<(), String> {
    let exe = machine.task_exe().map_err(|error| {
        let why = format!(
            "the tasks SandboxStop and SandboxSweep weren't registered, since the program they \
             would run as SYSTEM failed its check ({})",
            cause_of(&error)
        );
        tracing::error!(%why, "the set-up left the sandbox's tasks as they were");
        why
    })?;
    let mut failed = Vec::new();
    for task in POOL_TASKS {
        let name = task.name();
        let checked = machine
            .task_expected(task, owner, &exe)
            .map_err(|error| cause_of(&error))
            .and_then(|expected| {
                let found = machine.task_found(task);
                check(found.as_ref().map(Option::as_ref), &expected)
            });
        let put_back = match checked {
            Ok(TaskCheck::Same) => continue,
            Ok(TaskCheck::Missing) => {
                tracing::warn!(task = name, "a sandbox task is missing; registering it");
                machine.replace_task(task, owner, &exe)
            }
            Ok(TaskCheck::Differs(part)) => {
                tracing::warn!(
                    task = name,
                    ?part,
                    "a sandbox task is changed; registering it"
                );
                machine.replace_task(task, owner, &exe)
            }
            Err(why) => {
                failed.push(format!("{name} can't be read ({why})"));
                continue;
            }
        };
        if let Err(error) = put_back {
            failed.push(format!("{name}: {}", cause_of(&error)));
        }
    }
    if failed.is_empty() {
        return Ok(());
    }
    let why = format!(
        "the sandbox's tasks weren't all put back: {}",
        failed.join("; ")
    );
    tracing::error!(%why, "the set-up left a sandbox task as it was");
    Err(why)
}
