//! A per-user install's SYSTEM tasks `\Clauseo\SandboxRefresh`, `\Clauseo\SandboxStop` and
//! `\Clauseo\SandboxSweep` (`refresh::USER_TASKS`), as the set-up keeps them
//! (`late_steps::stop_task`). `--adopt` registers them when the installer runs; nothing else
//! did, so a task deleted or changed later stayed so, and every second launch was refused for
//! `AwaitingSweep` while Settings said ready. The app's repair runs this set-up, elevated
//! through the admin copy, which now puts them back:
//!
//! 1. the program they run, the admin copy, must pass the whole of `admin_copy::check`
//!    (`Machine::task_exe`): its folders up to the volume root, its folder's entries and its
//!    recorded hash. A SYSTEM task running a file the lawyer could change would run the lawyer's
//!    code as SYSTEM, so a copy that fails registers nothing;
//! 2. each task is read back and compared by meaning (`task_identity::check`): the principal's
//!    SID and run level, the program, its argument, the triggers, whether and how it runs, and
//!    the descriptor. The same task is left; a missing or changed one, or one this process may
//!    not read, is registered, or updated in place, by `--adopt`'s own registration, with the
//!    path step 1 checked (`Machine::replace_task`, `refresh::register_task_at`). An earlier
//!    build's task updated in place keeps its own descriptor; when the compare still finds it
//!    differs, it is set (`Machine::heal_task_descriptor`,
//!    `IRegisteredTask::SetSecurityDescriptor`), which Administrators' full control allows this
//!    elevated run.
//!
//! None of this rolls the pool back: a task that can't be put back is logged and named in the
//! report (`SetupReport::tasks`), and the other tasks are still tried. The run's result then isn't
//! a success (`outcome::exit_of`: exit 3 at `Step::StopTask`, in `TASKS_NOT_PUT_BACK`'s words),
//! and the readiness check says not ready, so the next repair tries again. A per-machine install's
//! set-up replaces both tasks on every run instead, as before (`Machine::register_stop_task`).

use super::machine::Machine;
use super::outcome::cause_of;
use crate::windows::refresh::SystemTask;
use crate::windows::refresh::USER_TASKS;
use crate::windows::refresh::task_identity::{Differs, TaskCheck, TaskIdentity, check};
use crate::windows::sids::Sid;

/// Puts back each of the per-user install's tasks ([`USER_TASKS`]) for the owner `owner` that is
/// missing or changed, as the module says: `Err` names what couldn't be put back, and why.
pub(super) fn ensure(machine: &mut impl Machine, owner: &Sid) -> Result<(), String> {
    let exe = machine.task_exe().map_err(|error| {
        let why = format!(
            "the tasks SandboxRefresh, SandboxStop and SandboxSweep weren't registered, since \
             the program they would run as SYSTEM failed its check ({})",
            cause_of(&error)
        );
        tracing::error!(%why, "the set-up left the sandbox's tasks as they were");
        why
    })?;
    let mut failed = Vec::new();
    for task in USER_TASKS {
        let name = task.name();
        let expected = match machine.task_expected(task, owner, &exe) {
            Ok(expected) => expected,
            Err(error) => {
                failed.push(format!("{name} can't be read ({})", cause_of(&error)));
                continue;
            }
        };
        match compare(machine, task, &expected) {
            Ok(TaskCheck::Same) => continue,
            Ok(TaskCheck::Missing) => {
                tracing::warn!(task = name, "a sandbox task is missing; registering it");
            }
            Ok(TaskCheck::Differs(part)) => {
                tracing::warn!(
                    task = name,
                    ?part,
                    "a sandbox task is changed; registering it"
                );
            }
            Err(why) => {
                failed.push(format!("{name} can't be read ({why})"));
                continue;
            }
        }
        if let Err(error) = machine.replace_task(task, owner, &exe) {
            failed.push(format!("{name}: {}", cause_of(&error)));
            continue;
        }
        // An earlier build's task, updated in place, keeps its own descriptor (the VM: no `P`,
        // and Windows' `SY FR`): set it, nothing deleted.
        if compare(machine, task, &expected) == Ok(TaskCheck::Differs(Differs::Descriptor)) {
            tracing::warn!(
                task = name,
                "a sandbox task kept an older descriptor; setting it"
            );
            if let Err(error) = machine.heal_task_descriptor(task, owner) {
                failed.push(format!("{name}: {}", cause_of(&error)));
            }
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

/// `task` as registered against `expected`, names looked up locally (`task_identity::check`).
fn compare(
    machine: &mut impl Machine,
    task: SystemTask,
    expected: &TaskIdentity,
) -> Result<TaskCheck, String> {
    let found = machine.task_found(task);
    let mut name_of = |sid: &Sid| machine.task_account_name(sid);
    check(found.as_ref().map(Option::as_ref), expected, &mut name_of)
}
