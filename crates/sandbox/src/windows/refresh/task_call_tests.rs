//! The bounded Task Scheduler calls (`task_call.rs`): one thread per kind at a time, and the
//! readiness check bounded like a launch and only reading.

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{Slot, ready_within, within};
use crate::SetupState;
use crate::windows::refresh::SystemTask;
use crate::windows::refresh::task_identity::{
    AccountName, ReadError, TASK_MISSING, TASK_UNCHECKED, TaskIdentity, TaskReader, tasks_repair,
};
use crate::windows::refresh::task_identity_tests::Reader;
use crate::windows::setup::record_values::CopyOwner;
use crate::windows::sids::Sid;

/// The short limit the tests use in place of 10 s.
const LIMIT: Duration = Duration::from_millis(50);

#[test]
fn three_calls_to_a_task_scheduler_that_never_answers_start_one_thread() {
    static HUNG: Slot<bool> = Slot::new();
    let (keep, never) = mpsc::channel::<()>();
    let never = Arc::new(Mutex::new(never));
    for _ in 0..3 {
        let never = Arc::clone(&never);
        let call = move || never.lock().unwrap().recv().is_ok();
        assert!(within(&HUNG, LIMIT, call).is_err());
    }
    assert_eq!(HUNG.started(), 1);
    // Once it answers, the next call starts afresh and is answered.
    drop(keep);
    // A call that joins the hung one as it ends gets its answer (`false`); the one after it
    // starts the second thread.
    let joined = within(&HUNG, Duration::from_secs(5), || true);
    if joined == Ok(false) {
        assert_eq!(within(&HUNG, Duration::from_secs(5), || true), Ok(true));
    } else {
        assert_eq!(joined, Ok(true));
    }
    assert_eq!(HUNG.started(), 2);
}

#[test]
fn a_call_joining_a_pending_one_gets_its_answer() {
    static SLOW: Slot<u32> = Slot::new();
    let (go, wait) = mpsc::channel::<()>();
    let first = within(&SLOW, LIMIT, move || {
        let _answered_or_dropped = wait.recv().is_ok();
        7
    });
    assert!(first.is_err());
    drop(go);
    assert_eq!(within(&SLOW, Duration::from_secs(5), || 8), Ok(7));
    assert_eq!(SLOW.started(), 1);
}

/// A readiness reader whose name lookup waits on a domain controller that never answers.
struct HungLookup {
    reader: Reader,
    never: mpsc::Receiver<()>,
}

impl TaskReader for HungLookup {
    fn copy_owner(&mut self) -> Result<Option<CopyOwner>, String> {
        self.reader.copy_owner()
    }
    fn user(&mut self) -> Result<Sid, String> {
        self.reader.user()
    }
    fn exe(&mut self) -> Result<PathBuf, String> {
        self.reader.exe()
    }
    fn found(&mut self, task: SystemTask) -> Result<Option<TaskIdentity>, ReadError> {
        self.reader.found(task)
    }
    fn descriptor(&mut self, owner: &Sid) -> Result<Vec<u8>, String> {
        self.reader.descriptor(owner)
    }
    fn account_name(&mut self, _sid: &Sid) -> Result<Option<AccountName>, String> {
        let _hung_until_the_test_ends = self.never.recv();
        Err("no answer".to_owned())
    }
}

#[test]
fn readiness_whose_name_lookup_never_answers_is_couldnt_check_within_the_bound() {
    static READY: Slot<SetupState> = Slot::new();
    let (keep, never) = mpsc::channel::<()>();
    // The refresh task's trigger reads back as a name, so its user is looked up.
    let mut reader = Reader::adopted();
    for (task, read) in &mut reader.tasks {
        if let (SystemTask::Refresh, Ok(read)) = (task, read) {
            for trigger in &mut read.triggers {
                *trigger = crate::windows::refresh::task_identity::TaskTrigger::Logon(
                    r"CLAUSEO-VM\lawyer".into(),
                );
            }
        }
    }
    let hung = HungLookup { reader, never };
    let state = ready_within(&READY, LIMIT, SetupState::Ready, move || hung);
    assert_eq!(state, SetupState::Failed(TASK_UNCHECKED.to_owned()));
    // A state that isn't Ready starts no check at all.
    let needed = ready_within(&READY, LIMIT, SetupState::Needed, Reader::adopted);
    assert_eq!(needed, SetupState::Needed);
    assert_eq!(READY.started(), 1);
    drop(keep);
}

#[test]
fn readiness_with_the_sweep_task_missing_asks_for_no_refresh_run() {
    static READY: Slot<SetupState> = Slot::new();
    // The readiness check gets a reader and nothing that can start a task: the missing sweep
    // task is said, and no run is asked for. A launch's heal asks for one (`unclean::tests::
    // failures::a_missing_task_is_put_back_by_the_refresh_task_before_the_launch_refuses`).
    let missing = || Reader::adopted().without(SystemTask::Sweep);
    let state = ready_within(&READY, Duration::from_secs(5), SetupState::Ready, missing);
    assert_eq!(state, SetupState::Failed(TASK_MISSING.to_owned()));
    // On Windows, `ready_with_user_tasks` is that check alone.
    let source = include_str!("task_read.rs");
    let (_, body) = source.split_once("pub fn ready_with_user_tasks").unwrap();
    let (body, _) = body.split_once("\n}\n").unwrap();
    assert!(body.contains("ready_within(&READY_CALL"), "{body}");
    for request in ["refresh", "request_", "launch_", "Run"] {
        let code = body
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"));
        assert!(!code.clone().any(|line| line.contains(request)), "{body}");
    }
}

/// A reader that records every name lookup.
struct CountedLookups {
    reader: Reader,
    lookups: usize,
}

impl TaskReader for CountedLookups {
    fn copy_owner(&mut self) -> Result<Option<CopyOwner>, String> {
        self.reader.copy_owner()
    }
    fn user(&mut self) -> Result<Sid, String> {
        self.reader.user()
    }
    fn exe(&mut self) -> Result<PathBuf, String> {
        self.reader.exe()
    }
    fn found(&mut self, task: SystemTask) -> Result<Option<TaskIdentity>, ReadError> {
        self.reader.found(task)
    }
    fn descriptor(&mut self, owner: &Sid) -> Result<Vec<u8>, String> {
        self.reader.descriptor(owner)
    }
    fn account_name(&mut self, _sid: &Sid) -> Result<Option<AccountName>, String> {
        self.lookups += 1;
        Err("no lookup on a launch".to_owned())
    }
}

#[test]
fn the_launch_check_of_stop_and_sweep_read_back_as_system_looks_up_no_name() {
    // The VM read the principal as `SYSTEM` through COM; English forms need no lookup.
    for name in [
        "SYSTEM",
        r"NT AUTHORITY\SYSTEM",
        r".\SYSTEM",
        "LocalSystem",
        "system",
    ] {
        let mut reader = CountedLookups {
            reader: Reader::adopted(),
            lookups: 0,
        };
        for (task, read) in &mut reader.reader.tasks {
            if let (SystemTask::Stop | SystemTask::Sweep, Ok(read)) = (task, read) {
                read.principal = name.to_owned();
            }
        }
        assert_eq!(tasks_repair(&mut reader), None, "{name}");
        assert_eq!(reader.lookups, 0, "{name}");
    }
}
