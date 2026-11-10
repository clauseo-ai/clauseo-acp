//! The set-up's fake under a deadline, a kill and an intent (split from `fake.rs`): [`Life`]
//! counts the fake's changes, so a test can set the cancel flag after the Nth, end the run there
//! as a kill would (a panic the test catches, with no rollback), tear a record write part way,
//! or block a call until the watchdog's end; and the intent's and the markers' calls of
//! [`FakeMachine`] (free functions, as the fake has its one inherent impl in `fake_more.rs`),
//! whose state keeps the pending values and a partial record's raw values.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use super::fake::FakeMachine;
use crate::windows::acl_plan::Sid;
use crate::windows::pool::MachineRecord;
use crate::windows::setup::MachineError;
use crate::windows::setup::intent::{Intent, pool_names};
use crate::windows::setup::record_values::{
    DATA_DIR, GROUP_SID, OWNER_SID, POOL_SIDS, REMOVING, RawRecord, RegValue, decode, encode,
};

/// The call lines that change the machine: each is one change [`Life`] counts.
const CHANGES: [&str; 22] = [
    "write_record",
    "delete_record",
    "create_group",
    "delete_group",
    "deny_remote_logon",
    "remove_remote_logon_deny",
    "create_user",
    "set_password",
    "add_to_group",
    "hide",
    "delete_user",
    "log_on_once",
    "delete_profile",
    "give_to_admins",
    "write_marker",
    "reset_done",
    "stop_resets_done",
    "register_stop_task",
    "lock_admin_copy",
    "grow_journal",
    "network",
    "protect_for_owner",
];

/// A call that blocks until the watchdog's end, standing for a step that never returns.
pub(super) struct Block {
    /// The call line that blocks.
    pub(super) line: String,
    /// Told when the call is reached.
    pub(super) reached: Sender<()>,
    /// The call goes on (and the run ends, as the process would) when this gets a message.
    pub(super) release: Receiver<()>,
}

/// The run's life in the fake: its deadline's flag, its changes, a kill, a block.
#[derive(Default)]
pub(super) struct Life {
    /// The watchdog's cancel flag.
    pub(super) cancel: Arc<AtomicBool>,
    /// The flag also reads as set once this many changes were made.
    pub(super) cancel_after: Option<usize>,
    /// The change after this many panics before it is made: a kill, with no rollback.
    pub(super) kill_after: Option<usize>,
    /// The changes made so far (a record write counts five, one per value).
    pub(super) changes: usize,
    /// Each change, in order: its call line (`write_intent …` for an intent).
    pub(super) trail: Vec<String>,
    /// The call that blocks.
    pub(super) block: Option<Block>,
    /// Every intent written, in order, as lines (outside the call lines, so the call orders the
    /// older tests pin stay as they were).
    pub(super) intents: Vec<String>,
}

impl Life {
    /// Whether the run's deadline is near.
    pub(super) fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
            || self.cancel_after.is_some_and(|after| self.changes >= after)
    }

    /// One change about to be made: the kill, when it lands here, else counted.
    pub(super) fn change(&mut self, what: &str) {
        assert!(
            self.kill_after != Some(self.changes),
            "killed before change {} ({what})",
            self.changes + 1
        );
        self.changes += 1;
        self.trail.push(what.to_owned());
    }

    /// The call `line` was made: counted when it is a change, blocked when it is the block's.
    pub(super) fn on_call(&mut self, line: &str) {
        let verb = line.split(' ').next().unwrap_or_default();
        if CHANGES.contains(&verb) {
            self.change(line);
        }
        if self.block.as_ref().is_some_and(|block| block.line == line)
            && let Some(block) = self.block.take()
        {
            let _told = block.reached.send(());
            let ended = block.release.recv_timeout(Duration::from_secs(10));
            assert!(ended.is_ok(), "the watchdog never ended the run");
            panic!("the process was ended in {line}");
        }
    }
}

/// The record's raw values now: the whole record's, else a partial one's, else none.
pub(super) fn raw(machine: &FakeMachine) -> RawRecord {
    match &machine.state.record {
        Some(record) => raw_of(record, 5),
        None => machine.state.partial.clone().unwrap_or_default(),
    }
}

/// Before a record write: the first four of its five values, each a change, so a kill can
/// land between two of them and leave the record torn there.
pub(super) fn tear(machine: &mut FakeMachine, record: &MachineRecord) {
    for written in 0..4 {
        if machine.life.kill_after == Some(machine.life.changes) && written > 0 {
            let mut raw = raw(machine);
            overlay(&mut raw, &raw_of(record, written));
            machine.state.record = decode(raw.clone()).unwrap();
            machine.state.partial = machine.state.record.is_none().then_some(raw);
        }
        machine.life.change("write_record's value");
    }
}

pub(super) fn read_intent(machine: &FakeMachine) -> Intent {
    let raw = raw(machine);
    Intent {
        owner: raw.owner.as_deref().map(|owner| Sid::parse(owner).unwrap()),
        group: machine.state.pending_group,
        users: pool_names(machine.state.pending_users.clone()),
        partial: raw.partial(),
    }
}

pub(super) fn write_intent(
    machine: &mut FakeMachine,
    owner: &Sid,
    group: bool,
    users: &[String],
) -> Result<(), MachineError> {
    let line = format!("write_intent {group} {}", users.join(","));
    machine.life.change(&line);
    machine.life.intents.push(line.clone());
    if let Some(at) = machine.fails.iter().position(|(text, _)| *text == line) {
        return Err(machine.fails.remove(at).1);
    }
    if (group || !users.is_empty()) && raw(machine).owner.is_none() {
        let partial = machine.state.partial.get_or_insert_default();
        partial.owner = Some(owner.as_str().to_owned());
    }
    machine.state.pending_group = group;
    users.clone_into(&mut machine.state.pending_users);
    Ok(())
}

/// The intent's values and a partial record go with the record.
pub(super) fn forget_intent(machine: &mut FakeMachine) {
    machine.state.partial = None;
    machine.state.pending_group = false;
    machine.state.pending_users.clear();
}

pub(super) fn marked_user(machine: &FakeMachine, name: &str) -> Option<Sid> {
    let group = machine.state.group.as_ref();
    machine.state.accounts.get(name).and_then(|account| {
        let member = group.is_some_and(|g| account.groups.contains(g));
        (account.marked || member).then(|| account.sid.clone())
    })
}

pub(super) fn marked_group(machine: &FakeMachine) -> Option<Sid> {
    machine
        .state
        .group
        .clone()
        .filter(|_| machine.state.group_marked)
}

/// The first `written` of `record`'s values, in the order they are written, as read back.
fn raw_of(record: &MachineRecord, written: usize) -> RawRecord {
    let mut raw = RawRecord::default();
    for (name, value) in encode(record).unwrap().into_iter().take(written) {
        let text = |units: &[u16]| {
            let end = units
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(units.len());
            String::from_utf16(&units[..end]).unwrap()
        };
        match (name, value) {
            (OWNER_SID, RegValue::Sz(units)) => raw.owner = Some(text(&units)),
            (GROUP_SID, RegValue::Sz(units)) => raw.group = Some(text(&units)),
            (DATA_DIR, RegValue::Sz(units)) => raw.data_dir = Some(text(&units)),
            (POOL_SIDS, RegValue::MultiSz(units)) => raw.pool = Some(units),
            (REMOVING, RegValue::Dword(number)) => raw.removing = Some(number),
            other => panic!("{other:?}"),
        }
    }
    raw
}

/// `raw` with each value `new` has.
fn overlay(raw: &mut RawRecord, new: &RawRecord) {
    raw.owner = new.owner.clone().or(raw.owner.take());
    raw.group = new.group.clone().or(raw.group.take());
    raw.pool = new.pool.clone().or(raw.pool.take());
    raw.data_dir = new.data_dir.clone().or(raw.data_dir.take());
    raw.removing = new.removing.or(raw.removing.take());
}
