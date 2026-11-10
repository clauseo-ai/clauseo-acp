//! Set-up and `--remove` under `--deadline` (`deadline.rs`), against the fake: the cancel flag
//! before each change stops the run, rolls it back and exits `DeadlineReached`; a step that
//! never returns is ended at the deadline with nothing undone behind it; no deadline is the run
//! as before, its intent written before the group and each account and cleared once recorded.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use super::fake::{FakeAcl, FakeMachine};
use super::fake_intent::Block;
use super::remove::{assert_all_gone, pool};
use super::*;
use crate::windows::setup::deadline::{self, Clock, ROLLBACK_BOUND, Waited};

/// The changes a rollback makes: none goes forward.
const UNDOING: [&str; 6] = [
    "delete_user ",
    "delete_profile ",
    "remove_remote_logon_deny ",
    "delete_group ",
    "delete_record",
    "write_intent false ",
];

/// A fresh set-up that succeeds, and the changes it made, in order.
fn whole_run(test: &str) -> (FakeMachine, Vec<String>) {
    let mut machine = FakeMachine::new(test);
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    let trail = machine.life.trail.clone();
    (machine, trail)
}

/// Nothing a fresh set-up makes is left: no pool account, group, record, intent or right.
fn assert_nothing_left(machine: &FakeMachine, case: &str) {
    let state = &machine.state;
    let names: Vec<&String> = state.accounts.keys().collect();
    assert_eq!(names, [OWNER_NAME], "{case}");
    assert_eq!(state.group, None, "{case}");
    assert_eq!(state.record, None, "{case}");
    assert_eq!(state.partial, None, "{case}");
    assert!(
        !state.pending_group && state.pending_users.is_empty(),
        "{case}"
    );
    assert!(state.remote_denied.is_empty(), "{case}");
}

#[test]
fn the_cancel_flag_before_each_change_rolls_back_then_exits_deadline_reached() {
    let (_, whole) = whole_run("deadline-whole");
    for after in 0..whole.len() {
        // A record write is one call, its five values five changes: the flag is checked
        // before the call, never between its values.
        if after > 0 && whole[after - 1] == "write_record's value" {
            continue;
        }
        let mut machine = FakeMachine::new("deadline-cancel");
        let mut acl = FakeAcl::default();
        machine.life.cancel_after = Some(after);
        let req = request(&mut machine, 0, Vec::new());
        let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
        assert_eq!(failure.exit, SetupExit::DeadlineReached, "{after}");
        assert!(failure.cause.contains("ran out of the time"), "{after}");
        assert!(failure.rolled_back, "{after}: {}", failure.cause);
        let trail = &machine.life.trail;
        assert_eq!(
            trail[..after],
            whole[..after],
            "{after}: the same changes up to it"
        );
        let forward: Vec<&String> = trail[after..]
            .iter()
            .filter(|change| !UNDOING.iter().any(|undo| change.starts_with(undo)))
            .collect();
        assert!(
            forward.is_empty(),
            "{after}: no change after the flag: {forward:?}"
        );
        assert_nothing_left(&machine, &format!("{after}"));
        assert_eq!(machine.calls.last().map(String::as_str), Some("unlock"));
    }
}

#[test]
fn a_deadline_within_r_stops_the_run_before_its_first_change() {
    let mut machine = FakeMachine::new("deadline-at-once");
    let flag = Arc::clone(&machine.life.cancel);
    let watchdog = deadline::start(
        ROLLBACK_BOUND,
        deadline::RealClock,
        move || flag.store(true, Ordering::SeqCst),
        || {},
    )
    .unwrap();
    let mut acl = FakeAcl::default();
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    drop(watchdog);
    assert_eq!(failure.exit, SetupExit::DeadlineReached);
    assert_eq!(failure.step, Step::Group);
    assert!(machine.life.trail.is_empty(), "{:?}", machine.life.trail);
    assert_nothing_left(&machine, "at once");
}

/// A clock whose first wait passes once `gate` hears the run is stuck, and the next at once.
struct StuckClock {
    gate: Option<Receiver<()>>,
}

impl Clock for StuckClock {
    fn wait(&mut self, _span: Duration, _stop: &Receiver<()>) -> Waited {
        if let Some(gate) = self.gate.take() {
            let _stuck = gate.recv_timeout(Duration::from_secs(10));
        }
        Waited::Elapsed
    }
}

#[test]
fn a_step_that_blocks_forever_is_ended_at_the_deadline_with_nothing_undone() {
    let mut machine = FakeMachine::new("deadline-blocked");
    let (reached, stuck) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let blocked = "log_on_once ClauseoSb01";
    machine.life.block = Some(Block {
        line: blocked.to_owned(),
        reached,
        release: released,
    });
    let flag = Arc::clone(&machine.life.cancel);
    let (ended, end) = mpsc::channel();
    let clock = StuckClock { gate: Some(stuck) };
    let watchdog = deadline::start(
        Duration::from_secs(600),
        clock,
        move || flag.store(true, Ordering::SeqCst),
        move || {
            // `TerminateProcess` in the real exe; here, the code, then the stuck call goes on
            // to end the run as the process would.
            let _code = ended.send(SetupExit::DeadlineReached.as_code());
            let _released = release.send(());
        },
    )
    .unwrap();
    let mut acl = FakeAcl::default();
    let req = request(&mut machine, 0, Vec::new());
    let run = catch_unwind(AssertUnwindSafe(|| set_up(&mut machine, &mut acl, &req)));
    assert!(run.is_err(), "the run never returned: the process ended");
    assert_eq!(end.recv_timeout(Duration::from_secs(10)), Ok(11));
    drop(watchdog);
    assert!(
        machine.life.cancelled(),
        "set at D - R, too late for the stuck step"
    );
    let at = machine.calls.iter().position(|c| c == blocked).unwrap();
    assert_eq!(machine.calls_since(at + 1), ["unlock"], "nothing undone");
    // What it left, its record names (the user was recorded before its logon), so a removal
    // takes it.
    let record = machine.state.record.clone().unwrap();
    assert_eq!(record.pool, [sid(1001)]);
    machine.life.cancel.store(false, Ordering::SeqCst);
    let remover = super::remove_fake::Remover::new(machine, Some(owner()));
    let mut acl = remover.acl(acl);
    let mut remover = remover;
    remove(&mut remover, &mut acl, false).unwrap();
    assert_all_gone(&remover, &acl);
}

#[test]
fn no_deadline_runs_as_before_with_its_intent_before_each_account() {
    let (machine, trail) = whole_run("deadline-none");
    assert!(!machine.life.cancelled());
    assert_eq!(machine.calls, fresh_calls());
    // The intent names the group before it is made and each account before it is made, and
    // is cleared once the record names them.
    let kept: Vec<&str> = trail
        .iter()
        .map(String::as_str)
        .filter(|c| {
            c.starts_with("write_intent")
                || c.starts_with("create_")
                || c.starts_with("write_record ")
        })
        .collect();
    let mut expected = vec!["write_intent true ", "create_group"];
    let names: Vec<String> = (1..=8).map(|n| format!("ClauseoSb0{n}")).collect();
    let intents: Vec<String> = names
        .iter()
        .enumerate()
        .map(|(at, name)| format!("write_intent {} {name}", at == 0))
        .collect();
    let users: Vec<String> = names.iter().map(|n| format!("create_user {n}")).collect();
    let records: Vec<String> = (1..=8u32)
        .map(|n| {
            let pool: Vec<String> = (1..=n).map(|r| (1000 + r).to_string()).collect();
            format!("write_record {}", pool.join(","))
        })
        .collect();
    for at in 0..8 {
        expected.extend([
            intents[at].as_str(),
            users[at].as_str(),
            records[at].as_str(),
            "write_intent false ",
        ]);
    }
    assert_eq!(kept, expected);
    assert!(!machine.state.pending_group && machine.state.pending_users.is_empty());
}

#[test]
fn a_removal_stopped_for_its_deadline_is_finished_by_the_next() {
    let (whole, _) = pool("deadline-remove-whole", Some(owner()));
    let from = whole.m.life.changes;
    drop(whole);
    for step in 0..30 {
        let (mut machine, mut acl) = pool("deadline-remove", Some(owner()));
        machine.m.life.cancel_after = Some(from + step);
        let Err(failure) = remove(&mut machine, &mut acl, false) else {
            // Past the removal's last change: it finished.
            assert_all_gone(&machine, &acl);
            return;
        };
        assert_eq!(failure.exit, SetupExit::DeadlineReached, "{step}");
        assert!(failure.cause.contains("ran out of the time"), "{step}");
        if step > 0 {
            assert!(machine.m.state.record.as_ref().unwrap().removing, "{step}");
        }
        machine.m.life.cancel_after = None;
        remove(&mut machine, &mut acl, false).unwrap();
        assert_all_gone(&machine, &acl);
    }
    panic!("the removal never finished within 30 changes");
}
