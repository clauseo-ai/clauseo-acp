//! Whether a SYSTEM task in `\Clauseo` is the one `register_task` makes, compared by meaning,
//! never by name or text alone: the principal's SID (LOCAL SYSTEM, whatever the language of the
//! name Windows reads back) and run level, the one action's program (its path canonicalised) and
//! argument, the triggers, the settings that decide whether the task runs at all (enabled, start
//! on demand, start and keep running on battery, what a second start does), and the descriptor
//! by its owner, group and entries (every one the set-up gives, exactly, and nothing more
//! but what Windows adds for SYSTEM or Administrators), on the task and in its definition. A
//! task of the right name that differs in any of them is changed, and a per-user set-up
//! registers it again (`setup::user_tasks`).
//!
//! The same comparison is the readiness check's (`UserSetup::state`, [`ready_with_tasks`]) and
//! the launch's ([`tasks_repair`]): on a per-user install, any of [`USER_TASKS`] (the refresh,
//! stop and sweep tasks) that is missing, changed or can't be read makes the set-up not ready,
//! in [`TASK_MISSING`]'s, [`TASK_CHANGED`]'s or [`TASK_UNCHECKED`]'s words (every task checked,
//! the first of those that applies said), so the app's repair runs the set-up, which puts it
//! back. A per-machine install is left as it was: its set-up replaces both tasks on every
//! run (`late_steps::stop_task`).
//!
//! Before a per-user set-up registers anything, the action's program is held to the whole of
//! `admin_copy::check` ([`action_exe`]): the copy and every folder above it up to the volume root
//! owned by SYSTEM, Administrators or `TrustedInstaller` and changeable by no one else, nothing
//! but the copy, its signature and staging in its folder, and its SHA-256 the recorded one. A
//! SYSTEM task that runs a file the lawyer could change would run the lawyer's code as SYSTEM, so
//! a copy that fails registers nothing, and the path checked is the path registered. No Windows
//! call here; tested on every OS.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::rules::action_path;
use super::{SystemTask, USER_TASKS};
use crate::windows::acl_bytes::{self, Acl, Descriptor};
use crate::windows::admin_copy::{self, AdminCopyFs};
use crate::windows::setup::record_values::CopyOwner;
use crate::windows::sids::Sid;
use crate::{Blocked, SetupState};

/// What the readiness check and a launch refused for `AwaitingSweep` say while a per-user
/// install's stop or sweep task is missing.
pub const TASK_MISSING: &str = "Clauseo's clean-up task is missing. Repair Clauseo to put it back.";
/// What they say while it is there but isn't the one the set-up registers.
pub const TASK_CHANGED: &str =
    "Clauseo's clean-up task has been changed. Repair Clauseo to put it back.";
/// What they say while it can't be read: the Task Scheduler is stopped or didn't answer.
pub const TASK_UNCHECKED: &str = "Clauseo couldn't check its clean-up task because Windows Task \
     Scheduler didn't respond. Restart the computer; if this message stays, repair Clauseo.";

/// LOCAL SYSTEM, as `register_task` names the principal.
const SYSTEM: &str = "S-1-5-18";
/// BUILTIN\Administrators.
const ADMINISTRATORS: &str = "S-1-5-32-544";
/// `TASK_RUNLEVEL_HIGHEST`.
pub const RUN_LEVEL_HIGHEST: i32 = 1;
/// `TASK_INSTANCES_QUEUE`: a start while one runs waits for it.
pub const INSTANCES_QUEUE: i32 = 1;
/// `TASK_INSTANCES_IGNORE_NEW`: a start while one runs is dropped.
pub const INSTANCES_IGNORE_NEW: i32 = 2;
/// `E_ACCESSDENIED`, as the Task Scheduler gives it for a task this caller may not read.
const ACCESS_DENIED: u32 = 0x8007_0005;
/// `ERROR_FILE_NOT_FOUND` and `ERROR_PATH_NOT_FOUND` as `HRESULT`s: no such task, or no
/// `\Clauseo` folder at all.
const NOT_FOUND: [u32; 2] = [0x8007_0002, 0x8007_0003];

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
    /// `TASK_TRIGGER_LOGON`, for the user it names (empty: anyone's logon), by SID when the name
    /// resolves to one ([`principal_sid`]): Windows may write it back as `DOMAIN\user`.
    Logon(String),
    /// Any other trigger, by its `TASK_TRIGGER_TYPE2`.
    Other(i32),
}

/// The settings that decide whether, and how often, the task runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskSettings {
    /// `IRegisteredTask::Enabled`.
    pub enabled: bool,
    /// `ITaskSettings::AllowDemandStart`: a run can be asked for.
    pub demand_start: bool,
    /// What it does on battery.
    pub on_battery: OnBattery,
    /// `ITaskSettings::MultipleInstances`, a `TASK_INSTANCES_POLICY`.
    pub instances: i32,
}

/// What a task does on battery power.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnBattery {
    /// It starts and keeps running, as the set-up registers it.
    Runs,
    /// It doesn't start (`ITaskSettings::DisallowStartIfOnBatteries`).
    NoStart,
    /// It starts, but stops when the computer goes on battery
    /// (`ITaskSettings::StopIfGoingOnBatteries`).
    Stopped,
}

impl OnBattery {
    /// From `DisallowStartIfOnBatteries` and `StopIfGoingOnBatteries`.
    pub fn of(disallow_start: bool, stop: bool) -> OnBattery {
        match (disallow_start, stop) {
            (true, _) => OnBattery::NoStart,
            (false, true) => OnBattery::Stopped,
            (false, false) => OnBattery::Runs,
        }
    }
}

/// What makes a task the one `register_task` registers, as read back from the Task Scheduler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskIdentity {
    /// The principal's SID ([`principal_sid`]), or its name as read when it names no account.
    pub principal: String,
    /// `IPrincipal::RunLevel`, a `TASK_RUNLEVEL_TYPE`.
    pub run_level: i32,
    /// Every action, in order.
    pub actions: Vec<TaskAction>,
    /// Every trigger, in order.
    pub triggers: Vec<TaskTrigger>,
    /// Whether and how it runs.
    pub settings: TaskSettings,
    /// The task's owner and DACL, as a self-relative descriptor.
    pub descriptor: Vec<u8>,
    /// The descriptor its definition carries (`IRegistrationInfo::SecurityDescriptor`, the
    /// XML's `<SecurityDescriptor>`) as a self-relative descriptor: `None` when it carries
    /// none, or one that can't be read.
    pub definition_descriptor: Option<Vec<u8>>,
}

impl TaskIdentity {
    /// What `register_task` registers as `task` for the record owner `owner`, running `exe`:
    /// LOCAL SYSTEM at the highest run level, `exe` quoted with the task's one argument, a
    /// logon trigger for `owner` only when the task starts at logon, enabled, started on demand
    /// and on battery, never stopped for it, a second start queued or dropped as the task says,
    /// and `descriptor`, the task's descriptor (`rules::task_sddl` made into bytes).
    pub fn expected(task: SystemTask, owner: &Sid, exe: &Path, descriptor: Vec<u8>) -> Self {
        let triggers = if task.at_logon() {
            vec![TaskTrigger::Logon(owner.as_str().to_owned())]
        } else {
            Vec::new()
        };
        TaskIdentity {
            principal: SYSTEM.to_owned(),
            run_level: RUN_LEVEL_HIGHEST,
            actions: vec![TaskAction::Exec {
                path: action_path(exe),
                arguments: task.argument().to_owned(),
            }],
            triggers,
            settings: TaskSettings {
                enabled: true,
                demand_start: true,
                on_battery: OnBattery::Runs,
                instances: if task.queues() {
                    INSTANCES_QUEUE
                } else {
                    INSTANCES_IGNORE_NEW
                },
            },
            definition_descriptor: Some(descriptor.clone()),
            descriptor,
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
    /// Its descriptor, or the one its definition carries (when it carries one), isn't
    /// `task_sddl`'s for the owner, or this caller may not read it.
    Descriptor,
    /// It is disabled.
    Disabled,
    /// A run can't be asked for.
    DemandStart,
    /// It won't start, or stops, on battery.
    Batteries,
    /// A second start isn't queued or dropped as the set-up has it.
    Instances,
    /// It doesn't run at the highest run level.
    RunLevel,
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

/// Why a task couldn't be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// The Task Scheduler refused this caller (`E_ACCESSDENIED`): the task's descriptor no
    /// longer grants what the set-up's does, so it counts as changed.
    Refused,
    /// Anything else (the Task Scheduler stopped, COM failing): the cause, for the log.
    Failed(String),
}

impl ReadError {
    /// The error an `HRESULT` from a Task Scheduler call is, `text` its words for the log.
    pub fn of(hresult: u32, text: String) -> ReadError {
        if hresult == ACCESS_DENIED {
            ReadError::Refused
        } else {
            ReadError::Failed(text)
        }
    }
}

/// Whether an `HRESULT` from `ITaskFolder::GetTask` or `ITaskService::GetFolder` says there is
/// no such task or folder: a missing `\Clauseo` (`ERROR_PATH_NOT_FOUND`) means a missing task.
pub fn not_found(hresult: u32) -> bool {
    NOT_FOUND.contains(&hresult)
}

/// `found` (the task read back) against `expected` ([`TaskIdentity::expected`]): `Err` when it
/// couldn't be read for any reason but a refusal, which is a changed descriptor.
pub fn check(
    found: Result<Option<&TaskIdentity>, &ReadError>,
    expected: &TaskIdentity,
) -> Result<TaskCheck, String> {
    match found {
        Ok(None) => Ok(TaskCheck::Missing),
        Ok(Some(found)) => Ok(differs(found, expected).map_or(TaskCheck::Same, TaskCheck::Differs)),
        Err(ReadError::Refused) => Ok(TaskCheck::Differs(Differs::Descriptor)),
        Err(ReadError::Failed(error)) => Err(error.clone()),
    }
}

/// The first part of `found` that isn't `expected`'s.
fn differs(found: &TaskIdentity, expected: &TaskIdentity) -> Option<Differs> {
    let sid = |text: &str| Sid::parse(text).ok();
    if sid(&found.principal).is_none() || sid(&found.principal) != sid(&expected.principal) {
        return Some(Differs::Principal);
    }
    let program = |action: &TaskAction| match action {
        TaskAction::Exec { path, arguments } => Some((canonical_path(path), arguments.clone())),
        TaskAction::Other => None,
    };
    let ([found_action], [want]) = (found.actions.as_slice(), expected.actions.as_slice()) else {
        return Some(Differs::Action);
    };
    let (Some((path, arguments)), Some((want_path, want_arguments))) =
        (program(found_action), program(want))
    else {
        return Some(Differs::Action);
    };
    if path != want_path {
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
    let (got, want) = (found.settings, expected.settings);
    let parts = [
        (got.enabled == want.enabled, Differs::Disabled),
        (got.demand_start == want.demand_start, Differs::DemandStart),
        (got.on_battery == want.on_battery, Differs::Batteries),
        (got.instances == want.instances, Differs::Instances),
        (found.run_level == expected.run_level, Differs::RunLevel),
        (
            same_descriptor(&found.descriptor, &expected.descriptor),
            Differs::Descriptor,
        ),
        // A definition that carries none (an earlier `--adopt`'s, from before it set one) rests on
        // the task's own descriptor, compared just above.
        (
            found
                .definition_descriptor
                .as_deref()
                .is_none_or(|carried| {
                    let want = expected.definition_descriptor.as_deref();
                    same_descriptor(carried, want.unwrap_or(&expected.descriptor))
                }),
            Differs::Descriptor,
        ),
    ];
    parts
        .into_iter()
        .find(|(same, _)| !same)
        .map(|(_, part)| part)
}

/// `path` as Windows resolves it, for comparing: one pair of surrounding quotes and spaces
/// dropped, any `\\?\` prefix dropped, either separator, `.` parts dropped and `..` parts
/// taken back, repeated separators as one, and case folded.
pub fn canonical_path(path: &str) -> String {
    let path = path.trim();
    let path = path
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(path)
        .trim();
    let path = path
        .strip_prefix(r"\\?\")
        .unwrap_or(path)
        .replace('/', "\\");
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('\\') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.len() > 1 {
                    parts.pop();
                }
            }
            part => parts.push(part),
        }
    }
    parts.join("\\").to_lowercase()
}

/// The principal's SID, or a logon trigger's user's: `name` itself when it is one, else what
/// `lookup` resolves it to (the Task Scheduler may write LOCAL SYSTEM by its name, in the
/// computer's language, and a user as `DOMAIN\user`), else `name`.
pub fn principal_sid(name: &str, lookup: impl FnOnce(&str) -> Option<Sid>) -> String {
    match Sid::parse(name.trim()) {
        Ok(sid) => sid.as_str().to_owned(),
        Err(_not_a_sid) => {
            lookup(name.trim()).map_or_else(|| name.to_owned(), |sid| sid.as_str().to_owned())
        }
    }
}

/// Whether the self-relative descriptor `found` is `expected`'s as Windows keeps it: the same
/// owner, the DACL protected from inheritance (`SE_DACL_PROTECTED`) as `expected`'s is, the same
/// group when `expected` names one (a group grants nothing on a task), every
/// DACL entry of `expected` present exactly (type, inheritance flags but inherited or not,
/// rights with generic ones mapped to the file rights the Task Scheduler checks, and SID), and
/// nothing else but allow entries for SYSTEM or Administrators. Windows adds those itself: a
/// registration names its principal, LOCAL SYSTEM, read access (`(A;;FR;;;SY)`, read back on
/// Server 2025) unless `TASK_DONT_ADD_PRINCIPAL_ACE` is passed. They grant nothing to anyone
/// who doesn't already hold full control. Any other entry (Users, Everyone, a pool user, a wider
/// or second entry for the owner, any deny entry) is a change.
fn same_descriptor(found: &[u8], expected: &[u8]) -> bool {
    let owner = acl_bytes::owner_sid(found);
    if owner.is_none() || owner != acl_bytes::owner_sid(expected) {
        return false;
    }
    let protected = |bytes: &[u8]| Descriptor::parse(bytes).ok().map(|d| d.dacl_protected());
    if protected(found).is_none() || protected(found) != protected(expected) {
        return false;
    }
    if let Some(group) = group_sid(expected)
        && group_sid(found) != Some(group)
    {
        return false;
    }
    let (Some(found), Some(expected)) = (entries(found), entries(expected)) else {
        return false;
    };
    expected.is_subset(&found)
        && found
            .difference(&expected)
            .all(|(kind, _, _, sid)| *kind == acl_bytes::ALLOWED && windows_adds_for(sid))
}

/// Whether `sid` (as bytes) is LOCAL SYSTEM or Administrators, who hold full control of the
/// task already and may be given more entries by Windows.
fn windows_adds_for(sid: &[u8]) -> bool {
    acl_bytes::sid_text(sid).is_some_and(|sid| [SYSTEM, ADMINISTRATORS].contains(&sid.as_str()))
}

/// The group of the self-relative descriptor `bytes`, when it holds one: its offset is the
/// third `u32` after the revision and control.
fn group_sid(bytes: &[u8]) -> Option<Sid> {
    let offset = u32::from_le_bytes(bytes.get(8..12)?.try_into().ok()?);
    let sid = bytes.get(usize::try_from(offset).ok().filter(|at| *at != 0)?..)?;
    acl_bytes::sid_text(sid)
}

/// One DACL entry, by meaning: type, flags, rights and SID.
type Entry = (u8, u8, u32, Vec<u8>);

/// The DACL entries of the self-relative descriptor `bytes`; `None` without a DACL (which
/// allows everyone everything) or when it can't be read.
fn entries(bytes: &[u8]) -> Option<BTreeSet<Entry>> {
    let dacl = Acl::parse(Descriptor::parse(bytes).ok()?.dacl?).ok()?;
    dacl.aces()
        .iter()
        .map(|ace| {
            let flags = acl_bytes::flags(ace) & !acl_bytes::INHERITED;
            let rights = file_rights(acl_bytes::mask(ace)?);
            Some((
                acl_bytes::kind(ace),
                flags,
                rights,
                acl_bytes::sid(ace)?.to_vec(),
            ))
        })
        .collect()
}

/// `mask` with its generic rights mapped as for a file, as the Task Scheduler maps them.
fn file_rights(mask: u32) -> u32 {
    const GENERIC: [(u32, u32); 4] = [
        (0x8000_0000, 0x0012_0089),
        (0x4000_0000, 0x0012_0116),
        (0x2000_0000, 0x0012_00A0),
        (0x1000_0000, 0x001F_01FF),
    ];
    GENERIC.iter().fold(mask, |mask, (generic, rights)| {
        if mask & generic == 0 {
            mask
        } else {
            (mask & !generic) | rights
        }
    })
}

/// The admin copy's path, the one program a SYSTEM task may run and the one it is registered
/// with, once it passes the whole of `admin_copy::check`: else the block, and nothing may be
/// registered.
pub fn action_exe(fs: &dyn AdminCopyFs) -> Result<PathBuf, Blocked> {
    admin_copy::check(fs)
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
    fn found(&mut self, task: SystemTask) -> Result<Option<TaskIdentity>, ReadError>;
    /// `task_sddl(owner)` made into a self-relative descriptor.
    fn descriptor(&mut self, owner: &Sid) -> Result<Vec<u8>, String>;
}

/// What the set-up's repair would mend in `task`, in the words for it: on a per-user install
/// whose admin copy this user placed, [`TASK_MISSING`], [`TASK_CHANGED`], or [`TASK_UNCHECKED`]
/// when it can't be told; `None` when the task is the set-up's. A per-machine install, or none,
/// never needs one here, and a record that can't be read can't say which kind it is: `None`,
/// logged (the admin copy's own check refuses an unreadable record).
pub fn repair_reason(reader: &mut dyn TaskReader, task: SystemTask) -> Option<&'static str> {
    let placed = match reader.copy_owner() {
        Ok(Some(CopyOwner::User(placed))) => placed,
        Ok(Some(CopyOwner::Machine) | None) => return None,
        Err(error) => {
            tracing::warn!(%error, "the record's AdminCopyOwner can't be read");
            return None;
        }
    };
    let checked = (|| {
        let user = reader.user()?;
        if placed != user {
            return Ok(TaskCheck::Same);
        }
        let exe = reader.exe()?;
        let expected = TaskIdentity::expected(task, &user, &exe, reader.descriptor(&user)?);
        let found = reader.found(task);
        check(found.as_ref().map(Option::as_ref), &expected)
    })();
    match checked {
        Ok(TaskCheck::Same) => None,
        Ok(TaskCheck::Missing) => Some(TASK_MISSING),
        Ok(TaskCheck::Differs(part)) => {
            tracing::warn!(
                task = task.name(),
                ?part,
                "a sandbox task isn't the set-up's"
            );
            Some(TASK_CHANGED)
        }
        Err(error) => {
            tracing::warn!(task = task.name(), %error, "a sandbox task can't be checked");
            Some(TASK_UNCHECKED)
        }
    }
}

/// What the set-up's repair would mend in any of [`USER_TASKS`], each checked
/// ([`repair_reason`]), in the words that say the most a repair can do: [`TASK_MISSING`] over
/// [`TASK_CHANGED`] over [`TASK_UNCHECKED`]. `None` when every one is the set-up's.
pub fn tasks_repair(reader: &mut dyn TaskReader) -> Option<&'static str> {
    let rank = |words: &&str| {
        [TASK_MISSING, TASK_CHANGED, TASK_UNCHECKED]
            .iter()
            .position(|known| known == words)
    };
    USER_TASKS
        .into_iter()
        .filter_map(|task| repair_reason(reader, task))
        .min_by_key(rank)
}

/// The readiness check's last step: `state` as it is unless it is [`SetupState::Ready`] and a
/// task needs a repair ([`tasks_repair`]), which is [`SetupState::Failed`] in its words: not
/// ready, and the app's "Set up again" runs the repair.
pub fn ready_with_tasks(state: SetupState, reader: &mut dyn TaskReader) -> SetupState {
    if state != SetupState::Ready {
        return state;
    }
    tasks_repair(reader).map_or(state, |words| SetupState::Failed(words.to_owned()))
}

/// `task_sddl(owner)`'s meaning as bytes, as Windows would make them, for the tests on every OS:
/// owned by Administrators, protected, SYSTEM and Administrators full control, `owner` read and
/// run (`FRFX`).
#[cfg(test)]
pub(crate) fn test_descriptor(owner: &Sid) -> Vec<u8> {
    use crate::windows::admin_copy::fake::descriptor;
    use crate::windows::admin_copy::{FULL_ACCESS, READ_EXECUTE};
    let entries = [
        (0, 0, FULL_ACCESS, SYSTEM),
        (0, 0, FULL_ACCESS, ADMINISTRATORS),
        (0, 0, READ_EXECUTE, owner.as_str()),
    ];
    descriptor(ADMINISTRATORS, Some(&entries), true)
}
