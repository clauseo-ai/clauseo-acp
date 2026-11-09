//! `task_identity.rs` on every OS: a task is the set-up's only when its principal's SID, run
//! level, program, argument, triggers, settings and descriptor all mean what `register_task`
//! registers, however Windows writes them back; the program a SYSTEM task may run passes the
//! whole admin copy check or nothing is registered; and the readiness check says not ready, in
//! words for missing, changed or unreadable, only on a per-user install this user placed.

use std::path::{Path, PathBuf};

use super::task_identity::*;
use super::{SystemTask, TASKS, USER_TASKS};
use crate::windows::acl_bytes;
use crate::windows::admin_copy::fake::{Fake, descriptor, locked_dacl};
use crate::windows::admin_copy::{FULL_ACCESS, Found, READ_EXECUTE};
use crate::windows::setup::record_values::CopyOwner;
use crate::windows::sids::Sid;
use crate::{Blocked, SetupState};

pub(super) const LAWYER: &str = "S-1-5-21-1-2-3-1001";
const OTHER: &str = "S-1-5-21-1-2-3-1002";
const EXE: &str = r"C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe";
pub(super) const SYSTEM: &str = "S-1-5-18";
pub(super) const ADMINISTRATORS: &str = "S-1-5-32-544";
/// LOCAL SYSTEM's name as a German Windows writes it back.
const GERMAN_SYSTEM: &str = "NT-AUTORITÄT\\SYSTEM";

pub(super) fn lawyer() -> Sid {
    Sid::parse(LAWYER).unwrap()
}

/// What `register_task` registers as `task` for the lawyer.
pub(super) fn registered(task: SystemTask) -> TaskIdentity {
    TaskIdentity::expected(task, &lawyer(), Path::new(EXE), test_descriptor(&lawyer()))
}

/// `found` against `expected`, read without error.
pub(super) fn compare(found: &TaskIdentity, expected: &TaskIdentity) -> TaskCheck {
    check(Ok(Some(found)), expected, &mut local_names).unwrap()
}

/// A SID whose local lookup fails, as one would with the LSA unable to answer.
pub(super) const UNANSWERED: &str = "S-1-5-21-1-2-3-9999";

/// `LookupAccountSidW` as this test machine, `CLAUSEO-VM`, answers it locally.
pub(super) fn local_names(sid: &Sid) -> Result<Option<AccountName>, String> {
    if sid.as_str() == UNANSWERED {
        return Err("LookupAccountSidW: the LSA didn't answer".to_owned());
    }
    let named = match sid.as_str() {
        SYSTEM => Some(("SYSTEM", "NT AUTHORITY")),
        LAWYER => Some(("lawyer", "CLAUSEO-VM")),
        OTHER => Some(("other", "CLAUSEO-VM")),
        _ => None,
    };
    Ok(named.map(|(name, domain)| (name.to_owned(), domain.to_owned())))
}

/// A program action.
fn exec(path: &str, arguments: &str) -> TaskAction {
    TaskAction::Exec {
        path: path.to_owned(),
        arguments: arguments.to_owned(),
    }
}

/// The task descriptor with `entries` in place of its DACL, owned by `owner`.
pub(super) fn with_entries(owner: &str, entries: &[(u8, u8, u32, &str)]) -> Vec<u8> {
    descriptor(owner, Some(entries), true)
}

/// `bytes` with `group` as its group.
pub(super) fn with_group(mut bytes: Vec<u8>, group: &str) -> Vec<u8> {
    let at = u32::try_from(bytes.len()).unwrap();
    bytes.extend(acl_bytes::sid_bytes(&Sid::parse(group).unwrap()));
    bytes[8..12].copy_from_slice(&at.to_le_bytes());
    bytes
}

#[test]
fn the_expected_task_runs_the_admin_copy_as_system_whenever_asked() {
    let sweep = registered(SystemTask::Sweep);
    assert_eq!(sweep.principal, SYSTEM);
    assert_eq!(sweep.run_level, RUN_LEVEL_HIGHEST);
    assert_eq!(
        sweep.actions,
        [exec(&format!("\"{EXE}\""), "--sweep-tasks")]
    );
    assert_eq!(sweep.triggers, Vec::<TaskTrigger>::new());
    let settings = TaskSettings {
        enabled: true,
        demand_start: true,
        on_battery: OnBattery::Runs,
        instances: INSTANCES_QUEUE,
    };
    assert_eq!(sweep.settings, settings);
    let stop = registered(SystemTask::Stop);
    assert_eq!(stop.settings.instances, INSTANCES_IGNORE_NEW);
    let refresh = registered(SystemTask::Refresh);
    assert_eq!(refresh.triggers, [TaskTrigger::Logon(LAWYER.to_owned())]);
    for task in TASKS {
        assert_eq!(
            compare(&registered(task), &registered(task)),
            TaskCheck::Same
        );
        assert_eq!(
            check(Ok(None), &registered(task), &mut local_names),
            Ok(TaskCheck::Missing)
        );
    }
}

/// A change to a task as read back.
type Change = Box<dyn Fn(&mut TaskIdentity)>;

#[test]
fn each_part_that_differs_is_named() {
    let quoted = format!("\"{EXE}\"");
    let writable = r#""C:\Users\lawyer\AppData\Local\Clauseo\clauseo-sandbox-setup.exe""#;
    let lawyer_full = (0, 0, FULL_ACCESS, LAWYER);
    let changes: Vec<(Differs, Change)> = vec![
        (
            Differs::Principal,
            Box::new(|t| t.principal = LAWYER.to_owned()),
        ),
        (
            Differs::Principal,
            Box::new(|t| t.principal = GERMAN_SYSTEM.to_owned()),
        ),
        (
            Differs::Action,
            Box::new(move |t| t.actions = vec![exec(writable, "--sweep-tasks")]),
        ),
        (Differs::Action, Box::new(|t| t.actions.clear())),
        (
            Differs::Action,
            Box::new(|t| t.actions.push(TaskAction::Other)),
        ),
        (
            Differs::Action,
            Box::new(|t| t.actions = vec![TaskAction::Other]),
        ),
        (
            Differs::Arguments,
            Box::new(move |t| t.actions = vec![exec(&quoted, "--stop")]),
        ),
        (
            Differs::Triggers,
            Box::new(|t| t.triggers.push(TaskTrigger::Other(1))),
        ),
        (
            Differs::Triggers,
            Box::new(|t| t.triggers.push(TaskTrigger::Logon(String::new()))),
        ),
        (Differs::Disabled, Box::new(|t| t.settings.enabled = false)),
        (
            Differs::DemandStart,
            Box::new(|t| t.settings.demand_start = false),
        ),
        (
            Differs::Batteries,
            Box::new(|t| t.settings.on_battery = OnBattery::NoStart),
        ),
        (
            Differs::Batteries,
            Box::new(|t| t.settings.on_battery = OnBattery::Stopped),
        ),
        (
            Differs::Instances,
            Box::new(|t| t.settings.instances = INSTANCES_IGNORE_NEW),
        ),
        (Differs::RunLevel, Box::new(|t| t.run_level = 0)),
        (
            Differs::Descriptor,
            Box::new(|t| t.descriptor = test_descriptor(&Sid::parse(OTHER).unwrap())),
        ),
        (
            Differs::Descriptor,
            Box::new(move |t| {
                t.descriptor =
                    with_entries(ADMINISTRATORS, &[(0, 0, FULL_ACCESS, SYSTEM), lawyer_full]);
            }),
        ),
        (
            Differs::Descriptor,
            Box::new(|t| {
                let entries = [
                    (0, 0, FULL_ACCESS, SYSTEM),
                    (0, 0, FULL_ACCESS, ADMINISTRATORS),
                ];
                t.descriptor = with_entries(LAWYER, &entries);
            }),
        ),
        (
            Differs::Descriptor,
            Box::new(|t| t.descriptor = descriptor(ADMINISTRATORS, None, true)),
        ),
        (Differs::Descriptor, Box::new(|t| t.descriptor = Vec::new())),
    ];
    let expected = registered(SystemTask::Sweep);
    for (part, change) in changes {
        let mut found = expected.clone();
        change(&mut found);
        assert_eq!(
            compare(&found, &expected),
            TaskCheck::Differs(part),
            "{found:?}"
        );
    }
    // The refresh task's logon trigger for another user differs too.
    let mut found = registered(SystemTask::Refresh);
    found.triggers = vec![TaskTrigger::Logon(OTHER.to_owned())];
    let refresh = registered(SystemTask::Refresh);
    assert_eq!(
        compare(&found, &refresh),
        TaskCheck::Differs(Differs::Triggers)
    );
}

#[test]
fn a_disabled_sweep_task_is_changed() {
    let mut found = registered(SystemTask::Sweep);
    found.settings.enabled = false;
    let expected = registered(SystemTask::Sweep);
    assert_eq!(
        compare(&found, &expected),
        TaskCheck::Differs(Differs::Disabled)
    );
    let mut reader = Reader::adopted();
    reader.tasks[2].1 = Ok(found);
    assert_eq!(
        repair_reason(&mut reader, SystemTask::Sweep),
        Some(TASK_CHANGED)
    );
}

#[test]
fn a_german_read_back_and_other_texts_with_the_same_meaning_are_the_same() {
    let expected = registered(SystemTask::Stop);
    // The principal by SID, or by LOCAL SYSTEM's own name in the machine's language.
    let mut german = |sid: &Sid| {
        let system = (sid.as_str() == SYSTEM).then(|| ("SYSTEM".into(), "NT-AUTORITÄT".into()));
        Ok(system)
    };
    let mut found = expected.clone();
    found.principal = account_text(GERMAN_SYSTEM);
    assert_eq!(found.principal, GERMAN_SYSTEM);
    let checked = check(Ok(Some(&found)), &expected, &mut german);
    assert_eq!(checked, Ok(TaskCheck::Same));
    assert_eq!(account_text(" S-1-5-18 "), SYSTEM);
    assert_eq!(
        account_text(" AUTORITE NT\\Inconnu "),
        "AUTORITE NT\\Inconnu"
    );
    // The program by its canonical path.
    let paths = [
        EXE.to_owned(),
        EXE.to_uppercase(),
        format!(" \"{}\" ", EXE.replace('\\', "/")),
        format!(r"\\?\{EXE}"),
        r"C:\Program Files\\Clauseo\.\Sandbox\..\Sandbox\clauseo-sandbox-setup.exe".to_owned(),
    ];
    for path in paths {
        let mut found = expected.clone();
        found.actions = vec![exec(&path, "--stop")];
        assert_eq!(compare(&found, &expected), TaskCheck::Same, "{path}");
    }
    // The descriptor by owner, group and entry set: reordered, inherited, a generic right for
    // the file rights it maps to, and a group the set-up didn't name.
    let owner_run = (0, 0x10, 0x8000_0000 | 0x2000_0000, LAWYER);
    let entries = [
        owner_run,
        (0, 0, 0x1000_0000, ADMINISTRATORS),
        (0, 0, FULL_ACCESS, SYSTEM),
    ];
    let mut found = expected.clone();
    found.descriptor = with_group(with_entries(ADMINISTRATORS, &entries), SYSTEM);
    assert_eq!(compare(&found, &expected), TaskCheck::Same);
    // A group is compared once the expected descriptor names one.
    let mut grouped = expected.clone();
    grouped.descriptor = with_group(grouped.descriptor, ADMINISTRATORS);
    assert_eq!(
        compare(&found, &grouped),
        TaskCheck::Differs(Differs::Descriptor)
    );
    found.descriptor = with_group(expected.descriptor, ADMINISTRATORS);
    assert_eq!(compare(&found, &grouped), TaskCheck::Same);
    assert_eq!(READ_EXECUTE, 0x0012_0089 | 0x0012_00A0);
}

/// This machine's own names for LOCAL SYSTEM and for the user running the tests, in its
/// language, as the Task Scheduler may write them back, are theirs through the local lookup
/// `task_read.rs` uses (`LookupAccountSidW`).
#[cfg(windows)]
#[test]
fn this_machines_names_for_system_and_the_user_are_theirs() {
    use crate::windows::record::current_user_sid;
    use crate::windows::refresh::local_account_name;
    let user = current_user_sid().unwrap();
    for sid in [Sid::parse(SYSTEM).unwrap(), user] {
        let (name, domain) = local_account_name(&sid).unwrap().unwrap();
        for written in [format!("{domain}\\{name}"), name.to_uppercase(), name] {
            let same = same_account(&written, sid.as_str(), &mut local_account_name);
            assert_eq!(same, Ok(true), "{written}");
        }
    }
}

#[test]
fn a_refused_read_is_changed_and_any_other_error_cant_be_told() {
    let expected = registered(SystemTask::Sweep);
    let refused = ReadError::of(0x8007_0005, "E_ACCESSDENIED".to_owned());
    assert_eq!(refused, ReadError::Refused);
    assert_eq!(
        check(Err(&refused), &expected, &mut local_names),
        Ok(TaskCheck::Differs(Differs::Descriptor))
    );
    let down = ReadError::of(0x8004_1315, "the Task Scheduler isn't running".to_owned());
    assert_eq!(
        check(Err(&down), &expected, &mut local_names),
        Err("the Task Scheduler isn't running".to_owned())
    );
    // No task, or no `\Clauseo` folder at all, is a missing task.
    assert!(not_found(0x8007_0002));
    assert!(not_found(0x8007_0003));
    assert!(!not_found(0x8007_0005));
}

/// A descriptor owned by Administrators that lets `who` change it.
fn writable_by(who: &str, folder: bool) -> Vec<u8> {
    let inherit = if folder { 0x3 } else { 0 };
    let entries = [
        (0, inherit, FULL_ACCESS, SYSTEM),
        (0, inherit, FULL_ACCESS, ADMINISTRATORS),
        (0, inherit, READ_EXECUTE | 0x2, who),
    ];
    descriptor(ADMINISTRATORS, Some(&entries), true)
}

#[test]
fn the_action_program_must_pass_the_whole_admin_copy_check() {
    let found = action_exe(&Fake::ready()).unwrap();
    assert!(
        super::same_path(&found, Path::new(EXE)),
        "{}",
        found.display()
    );
    // Written out, as a path with `\` has no parts off Windows.
    let levels = [
        (EXE, Found::File),
        (r"C:\Program Files\Clauseo\Sandbox", Found::Folder),
        (r"C:\Program Files\Clauseo", Found::Folder),
        (r"C:\Program Files", Found::Folder),
        (r"C:\", Found::Folder),
    ];
    for (path, found) in levels {
        let path = Path::new(path);
        let security = if found == Found::File {
            writable_by(LAWYER, false)
        } else if path == Path::new(r"C:\") {
            // The root: deleting a child (what replaces Program Files) is the right that counts.
            descriptor(ADMINISTRATORS, Some(&[(0, 0, 0x40, LAWYER)]), false)
        } else {
            writable_by(LAWYER, true)
        };
        let mut fake = Fake::ready();
        fake.set(path, found, security);
        assert_eq!(
            action_exe(&fake),
            Err(Blocked::AdminCopyUnsafe),
            "{}",
            path.display()
        );
        // An owner that isn't an administrator is refused as well.
        let mut fake = Fake::ready();
        fake.set(path, found, descriptor(LAWYER, Some(&[]), true));
        assert_eq!(
            action_exe(&fake),
            Err(Blocked::AdminCopyUnsafe),
            "{}",
            path.display()
        );
    }
    let mut fake = Fake::ready();
    fake.set(
        Path::new(EXE),
        Found::Link,
        writable_by(ADMINISTRATORS, false),
    );
    // A link in place of the copy isn't the copy at all.
    assert_eq!(action_exe(&fake), Err(Blocked::AdminCopyMissing));
    // A DLL planted beside the copy, and a copy whose hash isn't the recorded one.
    let mut fake = Fake::ready();
    let planted = r"C:\Program Files\Clauseo\Sandbox\version.dll";
    fake.set(Path::new(planted), Found::File, locked_dacl(false));
    assert_eq!(action_exe(&fake), Err(Blocked::AdminCopyUnsafe));
    let mut fake = Fake::ready();
    fake.hash = "f".repeat(64);
    assert_eq!(action_exe(&fake), Err(Blocked::AdminCopyChanged));
}

/// The readiness check's reads, as the lawyer.
pub(super) struct Reader {
    pub(super) copy: Result<Option<CopyOwner>, String>,
    pub(super) tasks: Vec<(SystemTask, Result<TaskIdentity, ReadError>)>,
}

impl Reader {
    /// A per-user install the lawyer placed, with its three tasks registered (`USER_TASKS`:
    /// refresh, stop, sweep).
    pub(super) fn adopted() -> Reader {
        let tasks = USER_TASKS.map(|task| (task, Ok(registered(task))));
        Reader {
            copy: Ok(Some(CopyOwner::User(lawyer()))),
            tasks: tasks.to_vec(),
        }
    }

    pub(super) fn without(mut self, task: SystemTask) -> Reader {
        self.tasks.retain(|(held, _)| *held != task);
        self
    }
}

impl TaskReader for Reader {
    fn copy_owner(&mut self) -> Result<Option<CopyOwner>, String> {
        self.copy.clone()
    }
    fn user(&mut self) -> Result<Sid, String> {
        Ok(lawyer())
    }
    fn exe(&mut self) -> Result<PathBuf, String> {
        Ok(PathBuf::from(EXE))
    }
    fn found(&mut self, task: SystemTask) -> Result<Option<TaskIdentity>, ReadError> {
        let found = self.tasks.iter().find(|(held, _)| *held == task);
        found.map(|(_, read)| read.clone()).transpose()
    }
    fn descriptor(&mut self, owner: &Sid) -> Result<Vec<u8>, String> {
        Ok(test_descriptor(owner))
    }
    fn account_name(&mut self, sid: &Sid) -> Result<Option<AccountName>, String> {
        local_names(sid)
    }
}

#[test]
fn readiness_says_missing_changed_or_unchecked_on_a_per_user_install() {
    let failed = |words: &str| SetupState::Failed(words.to_owned());
    assert_eq!(
        TASK_MISSING,
        "Clauseo's clean-up task is missing. Repair Clauseo to put it back."
    );
    let ready = |mut reader: Reader| ready_with_tasks(SetupState::Ready, &mut reader);
    assert_eq!(ready(Reader::adopted()), SetupState::Ready);
    assert_eq!(
        ready(Reader::adopted().without(SystemTask::Sweep)),
        failed(TASK_MISSING)
    );
    assert_eq!(
        ready(Reader::adopted().without(SystemTask::Stop)),
        failed(TASK_MISSING)
    );
    let mut tampered = Reader::adopted();
    if let Ok(task) = &mut tampered.tasks[2].1 {
        task.actions = vec![exec(r"C:\Users\lawyer\evil.exe", "--sweep-tasks")];
    }
    assert_eq!(ready(tampered), failed(TASK_CHANGED));
    // A read the Task Scheduler refuses is a changed descriptor; one it can't answer is
    // unchecked, never Ready.
    let mut refused = Reader::adopted();
    refused.tasks[2].1 = Err(ReadError::Refused);
    assert_eq!(ready(refused), failed(TASK_CHANGED));
    let mut down = Reader::adopted();
    down.tasks[1].1 = Err(ReadError::Failed("CoCreateInstance failed".to_owned()));
    assert_eq!(ready(down), failed(TASK_UNCHECKED));
    // A state that isn't ready stays as it is.
    let mut gone = Reader::adopted().without(SystemTask::Sweep);
    assert_eq!(
        ready_with_tasks(SetupState::Needed, &mut gone),
        SetupState::Needed
    );
}

#[test]
fn machine_installs_other_owners_and_an_unread_record_keep_the_state() {
    let ready = |mut reader: Reader| ready_with_tasks(SetupState::Ready, &mut reader);
    for copy in [
        Ok(Some(CopyOwner::Machine)),
        Ok(None),
        Ok(Some(CopyOwner::User(Sid::parse(OTHER).unwrap()))),
        Err("RegGetValueW failed (5)".to_owned()),
    ] {
        let mut reader = Reader::adopted().without(SystemTask::Sweep);
        reader.copy = copy.clone();
        assert_eq!(
            repair_reason(&mut reader, SystemTask::Sweep),
            None,
            "{copy:?}"
        );
        assert_eq!(ready(reader), SetupState::Ready, "{copy:?}");
    }
}
