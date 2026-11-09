//! Whether a SYSTEM task in `\Clauseo` is the one `register_task` makes, compared by identity,
//! never by name alone: the principal (LOCAL SYSTEM), the one action's program and argument,
//! the triggers and the task's descriptor must all be what it registers for the record owner.
//! A task of the right name with anything else (a tampered action, an extra trigger, another
//! descriptor) differs, and a per-user set-up replaces it (`setup::user_tasks`).
//!
//! The same comparison is the readiness check's (`UserSetup::state`, [`ready_with_tasks`]) and
//! the `AwaitingSweep` refusal's ([`needs_repair`]): on a per-user install, a stop or sweep task
//! that is missing or differs makes the set-up not ready, in [`TASK_MISSING`]'s words, so the
//! app's repair runs the set-up, which puts it back. A per-machine install is left as it was:
//! its set-up replaces both tasks on every run (`late_steps::stop_task`).
//!
//! Before a per-user set-up registers anything, the action's program, the admin copy, is held
//! to `admin_copy::check_chain` ([`action_exe`]): the file and every folder above it up to the
//! volume root owned by SYSTEM, Administrators or `TrustedInstaller`, and changeable by no one
//! else. A SYSTEM task that runs a file the lawyer could change would run the lawyer's code as
//! SYSTEM, so a chain that fails registers nothing. No Windows call here; tested on every OS.

use std::path::{Path, PathBuf};

use super::rules::{action_path, same_path, sddl_matches, task_sddl};
use super::{POOL_TASKS, SystemTask};
use crate::windows::admin_copy::{AdminCopyFs, admin_exe, check_chain};
use crate::windows::setup::record_values::CopyOwner;
use crate::windows::sids::Sid;
use crate::{Blocked, SetupState};

/// What the readiness check and a launch refused for `AwaitingSweep` say while a per-user
/// install's stop or sweep task is missing or isn't the one the set-up registers.
pub const TASK_MISSING: &str = "Clauseo's clean-up task is missing. Repair Clauseo to put it back.";

/// LOCAL SYSTEM, as `register_task` names the principal.
const SYSTEM: &str = "S-1-5-18";
/// The other names the Task Scheduler may give LOCAL SYSTEM when it reads a principal back.
const SYSTEM_NAMES: [&str; 4] = [SYSTEM, "SYSTEM", "NT AUTHORITY\\SYSTEM", "LocalSystem"];

/// One action of a registered task: a program with its arguments, or another kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskAction {
    /// `TASK_ACTION_EXEC`: the program as written (quoted or not) and its arguments.
    Exec {
        /// `IExecAction::Path`.
        path: String,
        /// `IExecAction::Arguments`.
        arguments: String,
    },
    /// Any other action (an e-mail, a message box, a COM handler).
    Other,
}

/// One trigger of a registered task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskTrigger {
    /// `TASK_TRIGGER_LOGON`, for the user it names (empty: anyone's logon).
    Logon(String),
    /// Any other trigger, by its `TASK_TRIGGER_TYPE2`.
    Other(i32),
}

/// What makes a task the one `register_task` registers, as read back from the Task Scheduler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskIdentity {
    /// The principal's user (`IPrincipal::UserId`).
    pub principal: String,
    /// Every action, in order.
    pub actions: Vec<TaskAction>,
    /// Every trigger, in order.
    pub triggers: Vec<TaskTrigger>,
    /// The task's owner and DACL, as SDDL.
    pub sddl: String,
}

impl TaskIdentity {
    /// What `register_task` registers as `task` for the record owner `owner`, running `exe`:
    /// LOCAL SYSTEM, `exe` quoted with the task's one argument, a logon trigger for `owner`
    /// only when the task starts at logon, and `sddl`, the task's descriptor
    /// (`rules::task_sddl`) as Windows writes it back (`staging::as_read_back` on Windows).
    pub fn expected(task: SystemTask, owner: &Sid, exe: &Path, sddl: String) -> TaskIdentity {
        let triggers = if task.at_logon() {
            vec![TaskTrigger::Logon(owner.as_str().to_owned())]
        } else {
            Vec::new()
        };
        TaskIdentity {
            principal: SYSTEM.to_owned(),
            actions: vec![TaskAction::Exec {
                path: action_path(exe),
                arguments: task.argument().to_owned(),
            }],
            triggers,
            sddl,
        }
    }
}

/// The first part of a task that isn't what the set-up registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Differs {
    /// The principal isn't LOCAL SYSTEM.
    Principal,
    /// Not one action, or it isn't the admin copy's program.
    Action,
    /// The action's arguments aren't the task's one argument.
    Arguments,
    /// Its triggers aren't the task's.
    Triggers,
    /// Its descriptor isn't `task_sddl`'s for the owner.
    Descriptor,
}

impl Differs {
    /// Its name, for the log.
    pub fn name(self) -> &'static str {
        match self {
            Differs::Principal => "principal",
            Differs::Action => "action",
            Differs::Arguments => "arguments",
            Differs::Triggers => "triggers",
            Differs::Descriptor => "descriptor",
        }
    }
}

/// How a registered task compares with the one the set-up registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskCheck {
    /// The same task.
    Same,
    /// No task of the name.
    Missing,
    /// A task of the name, but not the same: the first part that differs.
    Differs(Differs),
}

/// `found` (the task read back, `None` when there is none) against `expected`
/// ([`TaskIdentity::expected`]). Paths compare as Windows compares them, quotes aside; the
/// arguments exactly; a logon trigger's user case folded; the descriptor through
/// `rules::sddl_matches`.
pub fn check(found: Option<&TaskIdentity>, expected: &TaskIdentity) -> TaskCheck {
    let Some(found) = found else {
        return TaskCheck::Missing;
    };
    match differs(found, expected) {
        Some(part) => TaskCheck::Differs(part),
        None => TaskCheck::Same,
    }
}

/// The first part of `found` that isn't `expected`'s.
fn differs(found: &TaskIdentity, expected: &TaskIdentity) -> Option<Differs> {
    if !same_principal(&found.principal, &expected.principal) {
        return Some(Differs::Principal);
    }
    let [found_action] = found.actions.as_slice() else {
        return Some(Differs::Action);
    };
    let (
        TaskAction::Exec { path, arguments },
        [
            TaskAction::Exec {
                path: want_path,
                arguments: want_arguments,
            },
        ],
    ) = (found_action, expected.actions.as_slice())
    else {
        return Some(Differs::Action);
    };
    if !same_path(Path::new(unquoted(path)), Path::new(unquoted(want_path))) {
        return Some(Differs::Action);
    }
    if arguments != want_arguments {
        return Some(Differs::Arguments);
    }
    let same_trigger = |(a, b): (&TaskTrigger, &TaskTrigger)| match (a, b) {
        (TaskTrigger::Logon(a), TaskTrigger::Logon(b)) => a.eq_ignore_ascii_case(b),
        _ => false,
    };
    if found.triggers.len() != expected.triggers.len()
        || !found
            .triggers
            .iter()
            .zip(&expected.triggers)
            .all(same_trigger)
    {
        return Some(Differs::Triggers);
    }
    if !sddl_matches(&found.sddl, &expected.sddl) {
        return Some(Differs::Descriptor);
    }
    None
}

/// Whether `found` names the principal `expected` does, LOCAL SYSTEM by any of its names.
fn same_principal(found: &str, expected: &str) -> bool {
    let system = |name: &str| SYSTEM_NAMES.iter().any(|s| s.eq_ignore_ascii_case(name));
    found.eq_ignore_ascii_case(expected) || (system(found) && system(expected))
}

/// `path` without one pair of surrounding double quotes.
fn unquoted(path: &str) -> &str {
    let path = path.trim();
    path.strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(path)
}

/// The admin copy's path, the one program a SYSTEM task may run, once it and every folder above
/// it up to the volume root pass `admin_copy::check_chain`: else the block (`AdminCopyUnsafe`,
/// or `AdminCopyMissing` without a Program Files folder), and nothing may be registered.
pub fn action_exe(fs: &dyn AdminCopyFs) -> Result<PathBuf, Blocked> {
    let program_files = fs.program_files().map_err(|error| {
        tracing::warn!(%error, "Windows gave no Program Files folder");
        Blocked::AdminCopyMissing
    })?;
    let exe = admin_exe(&program_files);
    check_chain(fs, &program_files, &exe)?;
    Ok(exe)
}

/// What the readiness check reads, as the lawyer: `task_read.rs`'s `OsTasks` on Windows. Errors are
/// causes for the log.
pub trait TaskReader {
    /// The record's `AdminCopyOwner`, `None` when there is none.
    fn copy_owner(&mut self) -> Result<Option<CopyOwner>, String>;
    /// The user running the check.
    fn user(&mut self) -> Result<Sid, String>;
    /// The admin copy's path under `%ProgramFiles%`.
    fn exe(&mut self) -> Result<PathBuf, String>;
    /// `task` as registered, `None` when it isn't.
    fn found(&mut self, task: SystemTask) -> Result<Option<TaskIdentity>, String>;
    /// `task_sddl(owner)` as Windows writes it back.
    fn sddl(&mut self, owner: &Sid) -> Result<String, String> {
        Ok(task_sddl(owner))
    }
}

/// Whether `task` needs the set-up again: on a per-user install whose admin copy this user
/// placed, when the task is missing or isn't the one the set-up registers. A per-machine
/// install, or none, never does here. `Err` when it can't be told.
pub fn needs_repair(reader: &mut dyn TaskReader, task: SystemTask) -> Result<bool, String> {
    let Some(CopyOwner::User(placed)) = reader.copy_owner()? else {
        return Ok(false);
    };
    let user = reader.user()?;
    if placed != user {
        return Ok(false);
    }
    let exe = reader.exe()?;
    let expected = TaskIdentity::expected(task, &user, &exe, reader.sddl(&user)?);
    let check = check(reader.found(task)?.as_ref(), &expected);
    if check != TaskCheck::Same {
        tracing::warn!(
            task = task.name(),
            ?check,
            "a sandbox task isn't the set-up's"
        );
    }
    Ok(check != TaskCheck::Same)
}

/// The readiness check's last step: `state` as it is unless it is [`SetupState::Ready`] and a
/// stop or sweep task needs a repair ([`needs_repair`]), which is
/// [`SetupState::Failed`] with [`TASK_MISSING`]: not ready, and the app's "Set up again" runs
/// the repair. A task that can't be read is logged and leaves the state as it is.
pub fn ready_with_tasks(state: SetupState, reader: &mut dyn TaskReader) -> SetupState {
    if state != SetupState::Ready {
        return state;
    }
    for task in POOL_TASKS {
        match needs_repair(reader, task) {
            Ok(false) => {}
            Ok(true) => return SetupState::Failed(TASK_MISSING.to_owned()),
            Err(error) => {
                tracing::warn!(task = task.name(), %error, "a sandbox task can't be checked");
            }
        }
    }
    state
}
