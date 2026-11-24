//! `TurnQueued` with a hold on the whole matter (another thread's undo, a restore, a relocate):
//! the thread shows `Queued` whatever `behind` says, and its header carries the hold until the
//! turn starts or leaves the queue. A wait with no hold folds as before.

use clauseo_protocol::{
    DomainEvent, MatterHold, MessageId, ThreadId, ThreadStatus, TurnId, TurnQueued, TurnStarted,
    UserMessageStatus,
};
use clauseo_reducer::{
    Applied, Error, REDUCER_SCHEMA_VERSION, Snapshot, SnapshotRef, StreamState, apply,
};

use crate::fixtures::{Log, thread_created, user_message};
use crate::helpers::{cancel, fold, prompt_status};

#[cfg(test)]
fn created() -> (Log, StreamState, ThreadId) {
    let mut log = Log::thread(21).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let state = fold(log.events());
    let thread = state.as_thread().unwrap().thread();
    (log, state, thread)
}

#[cfg(test)]
fn push(log: &mut Log, state: &mut StreamState, event: impl Into<DomainEvent>) -> Applied {
    let out = apply(state, &log.push(event).unwrap()).unwrap();
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    out
}

/// A prompt, then its turn waiting behind its own thread for `hold`.
#[cfg(test)]
fn held(
    log: &mut Log,
    state: &mut StreamState,
    thread: ThreadId,
    hold: MatterHold,
) -> (TurnId, MessageId) {
    let (turn, message) = (log.fresh().unwrap(), log.fresh().unwrap());
    push(log, state, user_message(turn, message, "Read the lease."));
    let queued = TurnQueued::new(thread, turn, thread, None)
        .unwrap()
        .held_by(hold);
    push(log, state, queued);
    (turn, message)
}

/// The header's status and hold.
#[cfg(test)]
fn header(state: &StreamState) -> (ThreadStatus, Option<MatterHold>) {
    let header = state.as_thread().unwrap().header().unwrap();
    (header.status, header.hold)
}

#[test]
fn a_turn_waiting_for_a_restore_shows_queued_with_the_hold_until_it_starts() {
    let (mut log, mut state, thread) = created();
    let (turn, message) = held(&mut log, &mut state, thread, MatterHold::Restore);
    let queued = ThreadStatus::Queued { behind: thread };
    assert_eq!(header(&state), (queued, Some(MatterHold::Restore)));
    // Its prompt left the thread's queue, as for a wait behind another thread's writer.
    assert_eq!(
        prompt_status(&state, message),
        Some(UserMessageStatus::Sent)
    );
    // The folded log gives the same view.
    assert_eq!(
        header(&fold(log.events())),
        (queued, Some(MatterHold::Restore))
    );

    let checkpoint_before = log.fresh().unwrap();
    push(
        &mut log,
        &mut state,
        TurnStarted {
            model: None,
            effort: None,
            turn,
            checkpoint_before,
        },
    );
    assert_eq!(header(&state), (ThreadStatus::Running, None));
}

#[test]
fn a_turn_waiting_for_another_threads_undo_or_a_relocate_carries_that_hold() {
    for hold in [MatterHold::Undo, MatterHold::Relocate] {
        let (mut log, mut state, thread) = created();
        held(&mut log, &mut state, thread, hold);
        assert_eq!(
            header(&state),
            (ThreadStatus::Queued { behind: thread }, Some(hold))
        );
    }
}

#[test]
fn a_turn_cancelled_while_it_waits_for_a_hold_clears_it() {
    let (mut log, mut state, thread) = created();
    let (turn, _) = held(&mut log, &mut state, thread, MatterHold::Relocate);
    cancel(&mut log, &mut state, turn);
    assert_eq!(header(&state), (ThreadStatus::Idle, None));
}

/// Schema 14 (and every later one) folds a wait for a hold on the matter to `Queued` with the
/// hold, and its prompt to `Sent`: a snapshot schema 13 wrote (a build that folded a restore's or
/// a relocate's wait as one behind the thread's own turn) is refused, so the stream is refolded
/// from its log and shows it.
#[test]
fn a_schema_13_snapshot_is_refolded() {
    let (mut log, mut state, thread) = created();
    held(&mut log, &mut state, thread, MatterHold::Relocate);
    let old = SnapshotRef {
        reducer_schema: 13,
        state: &state,
    };
    let read = Snapshot::from_bytes(&old.to_bytes().unwrap());
    assert!(
        matches!(
            read,
            Err(Error::SchemaMismatch {
                found: 13,
                expected: REDUCER_SCHEMA_VERSION
            })
        ),
        "{read:?}"
    );
    let queued = ThreadStatus::Queued { behind: thread };
    assert_eq!(
        header(&fold(log.events())),
        (queued, Some(MatterHold::Relocate))
    );
}
