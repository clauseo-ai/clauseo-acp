//! The rail's triage (0045): `ThreadSnoozed` and `ThreadDone` on the header, and the wake rule
//! that clears both on the lawyer's message, a permission request, an elicitation, a limit stop,
//! a failed or an ended turn.

use clauseo_protocol::{
    CheckpointId, DomainEvent, DoneBy, ElicitationRequest, ElicitationRequested, ErrorCode,
    HeaderEntry, LimitStopReason, SnoozeBy, TextKind, ThreadDone, ThreadSnoozed, Timestamp,
    TurnFailed, TurnId, TurnStoppedAtLimit, ViewEntry,
};
use clauseo_reducer::{
    REDUCER_SCHEMA_VERSION, Snapshot, StreamState, ViewChange, apply, wakes_thread,
};

use crate::fixtures::{
    Log, permission_requested, start_turn, text, thread_created, turn_ended, user_message,
};
use crate::helpers::fold;

#[cfg(test)]
const UNTIL: i64 = 1_760_086_400_000;

#[cfg(test)]
fn snooze(until: Option<i64>, by: SnoozeBy) -> DomainEvent {
    ThreadSnoozed {
        until: until.map(|ms| Timestamp::new(ms).unwrap()),
        by,
    }
    .into()
}

#[cfg(test)]
fn done(done: bool, by: DoneBy) -> DomainEvent {
    ThreadDone { done, by }.into()
}

#[cfg(test)]
fn header(state: &StreamState) -> &HeaderEntry {
    state.as_thread().unwrap().header().unwrap()
}

/// The header upserts among `changes`.
#[cfg(test)]
fn header_upserts(changes: &[ViewChange]) -> Vec<&HeaderEntry> {
    changes
        .iter()
        .filter_map(|change| match change {
            ViewChange::Upsert(entry) => match &**entry {
                ViewEntry::Header(header) => Some(&**header),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// A thread with a running turn, snoozed and marked done, and that turn.
#[cfg(test)]
fn snoozed_and_done(n: u64) -> (Log, StreamState, TurnId) {
    let mut log = Log::thread(n).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Read the lease.").unwrap();
    log.push(snooze(Some(UNTIL), SnoozeBy::Lawyer)).unwrap();
    log.push(done(true, DoneBy::MorningSweep)).unwrap();
    let state = fold(log.events());
    assert_eq!(
        header(&state).snoozed_until,
        Some(Timestamp::new(UNTIL).unwrap())
    );
    assert!(header(&state).done);
    (log, state, turn)
}

/// Each waking event, given the running turn and a fresh log to mint ids from.
#[cfg(test)]
fn triggers(log: &mut Log, turn: TurnId) -> Vec<(&'static str, DomainEvent)> {
    let checkpoint: CheckpointId = log.fresh().unwrap();
    vec![
        (
            "user_message",
            user_message(log.fresh().unwrap(), log.fresh().unwrap(), "And the deed?"),
        ),
        (
            "permission_requested",
            permission_requested(log.fresh().unwrap(), None),
        ),
        (
            "elicitation_requested",
            ElicitationRequested {
                elicitation: log.fresh().unwrap(),
                request: ElicitationRequest::Url {
                    message: "Sign in.".to_owned(),
                    url: "https://example.org/login".to_owned(),
                },
            }
            .into(),
        ),
        (
            "turn_stopped_at_limit",
            TurnStoppedAtLimit {
                turn,
                window: None,
                resets_at: None,
                reason: LimitStopReason::PlanWindow,
            }
            .into(),
        ),
        (
            "turn_failed",
            TurnFailed {
                turn,
                code: ErrorCode::ALL[0],
                detail: None,
            }
            .into(),
        ),
        ("turn_ended", turn_ended(turn, checkpoint).unwrap()),
    ]
}

#[test]
fn snooze_and_done_set_the_header_and_until_none_clears() {
    let mut log = Log::thread(3).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let mut state = fold(log.events());
    assert_eq!(header(&state).snoozed_until, None);
    assert!(!header(&state).done);
    let out = apply(
        &mut state,
        &log.push(snooze(Some(UNTIL), SnoozeBy::Lawyer)).unwrap(),
    )
    .unwrap();
    assert_eq!(header_upserts(&out.changes).len(), 1);
    // Done leaves the snooze alone, and a repeat changes nothing.
    apply(&mut state, &log.push(done(true, DoneBy::Lawyer)).unwrap()).unwrap();
    let again = apply(&mut state, &log.push(done(true, DoneBy::Lawyer)).unwrap()).unwrap();
    assert!(again.changes.is_empty(), "{:?}", again.changes);
    assert_eq!(
        header(&state).snoozed_until,
        Some(Timestamp::new(UNTIL).unwrap())
    );
    assert!(header(&state).done);
    // Cleared by hand, then by the host at its time: both are `until: None`.
    apply(
        &mut state,
        &log.push(snooze(None, SnoozeBy::Lawyer)).unwrap(),
    )
    .unwrap();
    assert_eq!(header(&state).snoozed_until, None);
    apply(
        &mut state,
        &log.push(snooze(Some(UNTIL), SnoozeBy::Lawyer)).unwrap(),
    )
    .unwrap();
    let woke = apply(
        &mut state,
        &log.push(snooze(None, SnoozeBy::Expired)).unwrap(),
    )
    .unwrap();
    assert_eq!(header_upserts(&woke.changes)[0].snoozed_until, None);
    apply(&mut state, &log.push(done(false, DoneBy::Lawyer)).unwrap()).unwrap();
    assert!(!header(&state).done);
    assert_eq!(&fold(log.events()), &state);
}

#[test]
fn each_waking_event_clears_snooze_and_done_in_one_header_upsert() {
    let (mut scratch, _, _) = snoozed_and_done(9);
    let names: Vec<&str> = {
        let turn = scratch.fresh().unwrap();
        triggers(&mut scratch, turn)
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    };
    for (i, name) in names.into_iter().enumerate() {
        let (mut log, mut state, turn) = snoozed_and_done(9);
        let (_, event) = triggers(&mut log, turn).swap_remove(i);
        assert!(wakes_thread(&event), "{name}");
        let out = apply(&mut state, &log.push(event).unwrap()).unwrap();
        let upserts = header_upserts(&out.changes);
        assert_eq!(upserts.len(), 1, "{name}: {:?}", out.changes);
        assert_eq!(upserts[0].snoozed_until, None, "{name}");
        assert!(!upserts[0].done, "{name}");
        assert_eq!(header(&state).snoozed_until, None, "{name}");
        assert!(!header(&state).done, "{name}");
        // The whole log folds to the same.
        assert_eq!(&fold(log.events()), &state, "{name}");
    }
}

#[test]
fn a_snooze_or_done_after_the_waking_event_stands() {
    let (mut log, _, turn) = snoozed_and_done(4);
    let checkpoint = log.fresh().unwrap();
    log.push(turn_ended(turn, checkpoint).unwrap()).unwrap();
    log.push(snooze(Some(UNTIL + 1), SnoozeBy::Lawyer)).unwrap();
    log.push(done(true, DoneBy::Lawyer)).unwrap();
    let state = fold(log.events());
    assert_eq!(
        header(&state).snoozed_until,
        Some(Timestamp::new(UNTIL + 1).unwrap())
    );
    assert!(header(&state).done);
}

#[test]
fn other_events_leave_snooze_and_done() {
    let (mut log, mut state, turn) = snoozed_and_done(5);
    let message = log.fresh().unwrap();
    let chunk = text(turn, message, TextKind::Answer, "The lease runs".to_owned());
    assert!(!wakes_thread(&chunk));
    let out = apply(&mut state, &log.push(chunk).unwrap()).unwrap();
    assert!(header_upserts(&out.changes).is_empty(), "{:?}", out.changes);
    assert!(header(&state).done);
    assert!(header(&state).snoozed_until.is_some());
}

#[test]
fn snooze_and_done_survive_a_snapshot_at_this_schema() {
    assert_eq!(REDUCER_SCHEMA_VERSION, 10);
    let (_, state, _) = snoozed_and_done(7);
    let bytes = Snapshot::of(&state).to_bytes().unwrap();
    let read = Snapshot::from_bytes(&bytes).unwrap().into_state().unwrap();
    assert_eq!(read, state);
    assert_eq!(
        header(&read).snoozed_until,
        Some(Timestamp::new(UNTIL).unwrap())
    );
    assert!(header(&read).done);
}
