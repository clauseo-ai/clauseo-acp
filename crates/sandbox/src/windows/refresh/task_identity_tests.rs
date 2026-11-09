//! `task_identity.rs` on every OS: a task is the set-up's only when its principal, program,
//! argument, triggers and descriptor all are; the program a SYSTEM task may run passes the
//! admin copy's chain up to the volume root or nothing is registered; and the readiness check
//! says not ready, in [`TASK_MISSING`]'s words, only on a per-user install this user placed.

use std::path::{Path, PathBuf};

use super::task_identity::*;
use super::{POOL_TASKS, SystemTask, TASKS, same_path, task_sddl};
use crate::windows::admin_copy::fake::{Fake, descriptor};
use crate::windows::admin_copy::{FULL_ACCESS, Found, READ_EXECUTE};
use crate::windows::setup::record_values::CopyOwner;
use crate::windows::sids::Sid;
use crate::{Blocked, SetupState};

const LAWYER: &str = "S-1-5-21-1-2-3-1001";
const OTHER: &str = "S-1-5-21-1-2-3-1002";
const EXE: &str = r"C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe";
const SYSTEM: &str = "S-1-5-18";
const ADMINISTRATORS: &str = "S-1-5-32-544";

fn lawyer() -> Sid {
    Sid::parse(LAWYER).unwrap()
}

/// What `register_task` registers as `task` for the lawyer.
fn registered(task: SystemTask) -> TaskIdentity {
    TaskIdentity::expected(task, &lawyer(), Path::new(EXE), task_sddl(&lawyer()))
}

#[test]
fn the_expected_task_is_system_running_the_admin_copy_with_its_one_argument() {
    let stop = registered(SystemTask::Stop);
    assert_eq!(stop.principal, SYSTEM);
    let program = TaskAction::Exec {
        path: format!("\"{EXE}\""),
        arguments: "--stop".to_owned(),
    };
    assert_eq!(stop.actions, [program]);
    assert_eq!(stop.triggers, Vec::<TaskTrigger>::new());
    assert_eq!(stop.sddl, task_sddl(&lawyer()));
    let refresh = registered(SystemTask::Refresh);
    assert_eq!(refresh.triggers, [TaskTrigger::Logon(LAWYER.to_owned())]);
    for task in TASKS {
        assert_eq!(
            check(Some(&registered(task)), &registered(task)),
            TaskCheck::Same
        );
        assert_eq!(check(None, &registered(task)), TaskCheck::Missing);
    }
}

/// A change to a task as read back.
type Change = Box<dyn Fn(&mut TaskIdentity)>;

/// A program action.
fn exec(path: &str, arguments: &str) -> TaskAction {
    TaskAction::Exec {
        path: path.to_owned(),
        arguments: arguments.to_owned(),
    }
}

#[test]
fn each_part_that_differs_is_named() {
    let quoted = format!("\"{EXE}\"");
    let writable = r#""C:\Users\lawyer\AppData\Local\Clauseo\clauseo-sandbox-setup.exe""#;
    let changes: Vec<(Differs, Change)> = vec![
        (
            Differs::Principal,
            Box::new(|t| t.principal = LAWYER.to_owned()),
        ),
        (
            Differs::Principal,
            Box::new(|t| t.principal = "S-1-5-19".to_owned()),
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
            Differs::Arguments,
            Box::new(|t| {
                t.actions = vec![exec(&format!("\"{EXE}\""), "--sweep-tasks --refresh")];
            }),
        ),
        (
            Differs::Triggers,
            Box::new(|t| t.triggers.push(TaskTrigger::Other(1))),
        ),
        (
            Differs::Triggers,
            Box::new(|t| t.triggers.push(TaskTrigger::Logon(String::new()))),
        ),
        (
            Differs::Descriptor,
            Box::new(|t| t.sddl = task_sddl(&Sid::parse(OTHER).unwrap())),
        ),
        (
            Differs::Descriptor,
            Box::new(|t| t.sddl.push_str("(A;;FA;;;S-1-5-21-1-2-3-1001)")),
        ),
    ];
    let expected = registered(SystemTask::Sweep);
    for (part, change) in changes {
        let mut found = expected.clone();
        change(&mut found);
        assert_eq!(
            check(Some(&found), &expected),
            TaskCheck::Differs(part),
            "{found:?}"
        );
    }
    // The refresh task's logon trigger for another user differs too.
    let mut found = registered(SystemTask::Refresh);
    found.triggers = vec![TaskTrigger::Logon(OTHER.to_owned())];
    let refresh = registered(SystemTask::Refresh);
    assert_eq!(
        check(Some(&found), &refresh),
        TaskCheck::Differs(Differs::Triggers)
    );
}

#[test]
fn system_by_any_name_a_path_as_windows_compares_it_and_read_back_flags_are_the_same() {
    let expected = registered(SystemTask::Stop);
    for name in ["SYSTEM", "nt authority\\system", "LocalSystem", "s-1-5-18"] {
        let mut found = expected.clone();
        found.principal = name.to_owned();
        assert_eq!(check(Some(&found), &expected), TaskCheck::Same, "{name}");
    }
    for path in [
        EXE.to_owned(),
        EXE.to_uppercase(),
        format!("\"{}\"", EXE.replace('\\', "/")),
    ] {
        let mut found = expected.clone();
        found.actions = vec![TaskAction::Exec {
            path: path.clone(),
            arguments: "--stop".to_owned(),
        }];
        assert_eq!(check(Some(&found), &expected), TaskCheck::Same, "{path}");
    }
    let mut found = expected.clone();
    found.sddl = found.sddl.replace("D:P", "D:PAI");
    assert_eq!(check(Some(&found), &expected), TaskCheck::Same);
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
fn the_action_program_and_every_folder_up_to_the_root_must_be_admin_only() {
    let found = action_exe(&Fake::ready()).unwrap();
    assert!(same_path(&found, Path::new(EXE)), "{}", found.display());
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
    // A link in place of the copy is refused.
    let mut fake = Fake::ready();
    fake.set(
        Path::new(EXE),
        Found::Link,
        writable_by(ADMINISTRATORS, false),
    );
    assert_eq!(action_exe(&fake), Err(Blocked::AdminCopyUnsafe));
}

/// The readiness check's reads, as the lawyer.
struct Reader {
    copy: Result<Option<CopyOwner>, String>,
    tasks: Vec<(SystemTask, TaskIdentity)>,
    unreadable: bool,
}

impl Reader {
    /// A per-user install the lawyer placed, with both tasks registered.
    fn adopted() -> Reader {
        let tasks = POOL_TASKS.map(|task| (task, registered(task)));
        Reader {
            copy: Ok(Some(CopyOwner::User(lawyer()))),
            tasks: tasks.to_vec(),
            unreadable: false,
        }
    }

    fn without(mut self, task: SystemTask) -> Reader {
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
    fn found(&mut self, task: SystemTask) -> Result<Option<TaskIdentity>, String> {
        if self.unreadable {
            return Err("ITaskService::Connect failed (0x80070005)".to_owned());
        }
        let found = self.tasks.iter().find(|(held, _)| *held == task);
        Ok(found.map(|(_, identity)| identity.clone()))
    }
}

#[test]
fn readiness_is_not_ready_while_a_per_user_task_is_missing_or_differs() {
    let missing = SetupState::Failed(TASK_MISSING.to_owned());
    assert_eq!(
        TASK_MISSING,
        "Clauseo's clean-up task is missing. Repair Clauseo to put it back."
    );
    let ready = |mut reader: Reader| ready_with_tasks(SetupState::Ready, &mut reader);
    assert_eq!(ready(Reader::adopted()), SetupState::Ready);
    assert_eq!(ready(Reader::adopted().without(SystemTask::Sweep)), missing);
    assert_eq!(ready(Reader::adopted().without(SystemTask::Stop)), missing);
    // The refresh task isn't one of the two.
    assert_eq!(
        ready(Reader::adopted().without(SystemTask::Refresh)),
        SetupState::Ready
    );
    let mut tampered = Reader::adopted();
    tampered.tasks[1].1.actions = vec![TaskAction::Exec {
        path: r"C:\Users\lawyer\evil.exe".to_owned(),
        arguments: "--sweep-tasks".to_owned(),
    }];
    assert_eq!(ready(tampered), missing);
    let mut swept = Reader::adopted().without(SystemTask::Sweep);
    assert_eq!(needs_repair(&mut swept, SystemTask::Sweep), Ok(true));
    assert_eq!(needs_repair(&mut swept, SystemTask::Stop), Ok(false));
    // A state that isn't ready stays as it is.
    let mut gone = Reader::adopted().without(SystemTask::Sweep);
    assert_eq!(
        ready_with_tasks(SetupState::Needed, &mut gone),
        SetupState::Needed
    );
}

#[test]
fn machine_installs_other_owners_and_unreadable_tasks_keep_the_state() {
    let ready = |mut reader: Reader| ready_with_tasks(SetupState::Ready, &mut reader);
    for copy in [
        Ok(Some(CopyOwner::Machine)),
        Ok(None),
        Ok(Some(CopyOwner::User(Sid::parse(OTHER).unwrap()))),
    ] {
        let mut reader = Reader::adopted().without(SystemTask::Sweep);
        reader.copy = copy.clone();
        assert_eq!(
            needs_repair(&mut reader, SystemTask::Sweep),
            Ok(false),
            "{copy:?}"
        );
        assert_eq!(ready(reader), SetupState::Ready, "{copy:?}");
    }
    let mut unreadable = Reader::adopted();
    unreadable.unreadable = true;
    assert!(needs_repair(&mut unreadable, SystemTask::Sweep).is_err());
    assert_eq!(ready(unreadable), SetupState::Ready);
    let mut record = Reader::adopted();
    record.copy = Err("RegGetValueW failed (5)".to_owned());
    assert_eq!(ready(record), SetupState::Ready);
}
