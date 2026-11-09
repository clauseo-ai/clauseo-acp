//! The no-prompt heal of a per-user install's SYSTEM tasks: `--refresh`, run by
//! `\Clauseo\SandboxRefresh` as LOCAL SYSTEM, puts back a stop, sweep or refresh task that is
//! missing or changed ([`keep_tasks`]), so a lawyer who can't answer a UAC prompt isn't left
//! refused. The shipped per-user install never registered `SandboxSweep`, and a
//! launch now refuses without it; the launch and the readiness check ask for a refresh run
//! first (`runner::unclean`, `task_read::ready_with_user_tasks`).
//!
//! SYSTEM writing tasks on the lawyer's behalf is a privilege boundary, held as follows:
//!
//! - **Fixed tasks only:** [`USER_TASKS`], a compile-time list of the three; no name comes from
//!   anywhere else.
//! - **Identity from SYSTEM's own record:** the owner is the record's `AdminCopyOwner`
//!   (`HKLM\SOFTWARE\Clauseo\Sandbox`, which only administrators and SYSTEM can write), read by
//!   the refresh itself; the principal is LOCAL SYSTEM. Nothing the lawyer can write is read: no
//!   file in their profile, no environment, nothing under `HKCU`.
//! - **The fixed descriptor only:** `rules::task_sddl`'s template with that owner.
//! - **The hash-verified admin copy only:** the action's program is the admin copy at its fixed
//!   path, once it passes the whole of `admin_copy::check`, its recorded `AdminCopySha256`
//!   included ([`TaskKeeper::exe`]); a copy that fails registers nothing.
//! - **No input from the trigger:** `--refresh` takes no further argument (`setup::args`), the
//!   task is started with no parameters (`IRegisteredTask::Run` with an empty value), and the
//!   refresh reads no environment.
//!
//! No Windows call here; tested on every OS.

use std::path::{Path, PathBuf};

use super::Refresher;
use super::task_identity::{AccountName, Differs, ReadError, TaskCheck, TaskIdentity, check};
use super::{SystemTask, USER_TASKS};
use crate::windows::setup::record_values::CopyOwner;
use crate::windows::sids::Sid;

/// What [`keep_tasks`] needs, as SYSTEM: `windows_refresher.rs` on Windows, faked in the tests.
#[cfg(any(windows, test))]
pub(super) trait TaskKeeper {
    /// The admin copy's fixed path, once it passes the whole of `admin_copy::check` (its folders,
    /// its folder's entries and its recorded `AdminCopySha256`); else why not.
    fn exe(&mut self) -> Result<PathBuf, String>;
    /// `task` as registered, `None` when it isn't.
    fn found(&mut self, task: SystemTask) -> Result<Option<TaskIdentity>, ReadError>;
    /// What `register_task` registers as `task` for `owner`, running `exe`.
    fn expected(
        &mut self,
        task: SystemTask,
        owner: &Sid,
        exe: &Path,
    ) -> Result<TaskIdentity, String>;
    /// Registers `task` for `owner`, or updates it in place, running `exe` (`register_task_at`).
    fn register(&mut self, task: SystemTask, owner: &Sid, exe: &Path) -> Result<(), String>;
    /// Sets `task`'s own descriptor to `task_sddl(owner)` (`set_task_descriptor`).
    fn set_descriptor(&mut self, task: SystemTask, owner: &Sid) -> Result<(), String>;
    /// The account `sid` names, looked up locally.
    fn account_name(&mut self, sid: &Sid) -> Result<Option<AccountName>, String>;
}

/// Puts back each of [`USER_TASKS`] for `owner` (the record's `AdminCopyOwner`) that is missing
/// or changed, as `setup::user_tasks` does for a repair: compared by meaning, registered or
/// updated in place, and its own descriptor set when an in-place update kept an older one. The
/// names put back; `Err` names what couldn't be. A copy that fails its check registers nothing.
#[cfg(any(windows, test))]
pub(super) fn keep_tasks(
    keeper: &mut dyn TaskKeeper,
    owner: &Sid,
) -> Result<Vec<&'static str>, String> {
    let exe = keeper
        .exe()
        .map_err(|why| format!("no task was put back: the admin copy failed its check ({why})"))?;
    let mut put_back = Vec::new();
    let mut failed = Vec::new();
    for task in USER_TASKS {
        let name = task.name();
        let expected = match keeper.expected(task, owner, &exe) {
            Ok(expected) => expected,
            Err(why) => {
                failed.push(format!("{name}: {why}"));
                continue;
            }
        };
        match compare(keeper, task, &expected) {
            Ok(TaskCheck::Same) => continue,
            Ok(TaskCheck::Missing | TaskCheck::Differs(_)) => {}
            Err(why) => {
                failed.push(format!("{name} can't be read ({why})"));
                continue;
            }
        }
        if let Err(why) = keeper.register(task, owner, &exe) {
            failed.push(format!("{name}: {why}"));
            continue;
        }
        if compare(keeper, task, &expected) == Ok(TaskCheck::Differs(Differs::Descriptor))
            && let Err(why) = keeper.set_descriptor(task, owner)
        {
            failed.push(format!("{name}: {why}"));
            continue;
        }
        put_back.push(name);
    }
    if failed.is_empty() {
        Ok(put_back)
    } else {
        Err(failed.join("; "))
    }
}

/// The refresh's last step with the set-up lock held, whatever the copy's outcome: on a
/// per-user install, [`Refresher::keep_tasks`] for the record's owner, its outcome logged. A
/// per-machine install's set-up keeps its own tasks.
pub(super) fn kept_by(fs: &mut impl Refresher, owner: &CopyOwner) {
    let CopyOwner::User(owner) = owner else {
        return;
    };
    match fs.keep_tasks(owner) {
        Ok(put) if put.is_empty() => {}
        Ok(put) => fs.log(
            false,
            &format!("put back the sandbox's tasks {}", put.join(", ")),
        ),
        Err(why) => fs.log(
            true,
            &format!("the sandbox's tasks weren't all put back: {why}"),
        ),
    }
}

/// `task` as registered against `expected`, names looked up locally.
#[cfg(any(windows, test))]
fn compare(
    keeper: &mut dyn TaskKeeper,
    task: SystemTask,
    expected: &TaskIdentity,
) -> Result<TaskCheck, String> {
    let found = keeper.found(task);
    let mut name_of = |sid: &Sid| keeper.account_name(sid);
    check(found.as_ref().map(Option::as_ref), expected, &mut name_of)
}
