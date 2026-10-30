//! `TurnQueued`: a turn waiting behind another thread's writer shows `Queued { behind }`; one
//! waiting behind its own thread keeps a running turn's status and, with no turn running, settles
//! the status (`Idle` unless the lawyer is asked). A queued turn cancelled before
//! it starts settles the status.

use clauseo_protocol::{
    DomainEvent, MessageId, ThreadId, ThreadStatus, TurnId, TurnQueued, TurnStarted,
    UserMessageStatus, ViewEntry,
};
use clauseo_reducer::{Applied, StreamState, ViewChange, apply};

use crate::fixtures::{
    Log, permission_requested, permission_resolved, start_turn, thread_created, thread_id,
    turn_ended, user_message,
};
use crate::helpers::{cancel, fold, prompt_status, round_trip};

/// A log with its thread created, the state folded so far, and the thread.
#[cfg(test)]
fn created() -> (Log, StreamState, ThreadId) {
    let mut log = Log::thread(21).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let state = fold(log.events());
    let thread = state.as_thread().unwrap().thread();
    (log, state, thread)
}

/// Pushes `event` and applies it.
#[cfg(test)]
fn push(log: &mut Log, state: &mut StreamState, event: impl Into<DomainEvent>) -> Applied {
    apply(state, &log.push(event).unwrap()).unwrap()
}

/// Pushes a prompt opening a new turn, and returns the turn and its message.
#[cfg(test)]
fn prompt(log: &mut Log, state: &mut StreamState, text: &str) -> (TurnId, MessageId) {
    let (turn, message) = (log.fresh().unwrap(), log.fresh().unwrap());
    push(log, state, user_message(turn, message, text));
    (turn, message)
}

/// `turn` of `thread` queued behind `behind`, or behind `behind_turn` of `thread`.
#[cfg(test)]
fn queued(
    thread: ThreadId,
    turn: TurnId,
    behind: ThreadId,
    behind_turn: Option<TurnId>,
) -> TurnQueued {
    TurnQueued::new(thread, turn, behind, behind_turn).unwrap()
}

/// `turn`'s `TurnStarted`, pushed and applied.
#[cfg(test)]
fn start(log: &mut Log, state: &mut StreamState, turn: TurnId) -> Applied {
    let checkpoint_before = log.fresh().unwrap();
    push(
        log,
        state,
        TurnStarted {
            turn,
            checkpoint_before,
        },
    )
}

/// `turn` ending normally, pushed and applied.
#[cfg(test)]
fn end(log: &mut Log, state: &mut StreamState, turn: TurnId) -> Applied {
    let checkpoint = log.fresh().unwrap();
    push(log, state, turn_ended(turn, checkpoint).unwrap())
}

#[cfg(test)]
fn status(state: &StreamState) -> ThreadStatus {
    state.as_thread().unwrap().status()
}

/// The status of the header `Upsert`s in `out`, which holds no other change.
#[cfg(test)]
fn header_statuses(out: &Applied) -> Vec<ThreadStatus> {
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    out.changes
        .iter()
        .map(|change| match change {
            ViewChange::Upsert(entry) => match &**entry {
                ViewEntry::Header(header) => header.status,
                other => panic!("not a header: {other:?}"),
            },
            other => panic!("not an upsert: {other:?}"),
        })
        .collect()
}

#[cfg(test)]
fn has_step(state: &StreamState, turn: TurnId) -> bool {
    state
        .entries()
        .values()
        .any(|entry| matches!(entry, ViewEntry::Step(step) if step.turn == turn))
}

#[test]
fn writer_wait_gives_queued() {
    let (mut log, mut state, thread) = created();
    let writer = thread_id(77).unwrap();
    // A first turn waits for another thread's writer, then starts.
    let (first, _) = prompt(&mut log, &mut state, "Read the lease.");
    assert_eq!(status(&state), ThreadStatus::Starting);
    let out = push(&mut log, &mut state, queued(thread, first, writer, None));
    let waiting = ThreadStatus::Queued { behind: writer };
    assert_eq!(header_statuses(&out), [waiting]);
    let out = push(&mut log, &mut state, queued(thread, first, writer, None));
    assert_eq!(
        header_statuses(&out),
        [],
        "the same wait again changes nothing"
    );
    let out = start(&mut log, &mut state, first);
    assert_eq!(header_statuses(&out), [ThreadStatus::Running]);

    // A second turn, queued behind the first, waits for the writer once the first ends: its
    // second `TurnQueued` names the other thread.
    let (second, _) = prompt(&mut log, &mut state, "Then clause 3.");
    push(
        &mut log,
        &mut state,
        queued(thread, second, thread, Some(first)),
    );
    assert_eq!(status(&state), ThreadStatus::Running);
    end(&mut log, &mut state, first);
    assert_eq!(status(&state), ThreadStatus::Idle);
    let out = push(&mut log, &mut state, queued(thread, second, writer, None));
    assert_eq!(header_statuses(&out), [waiting]);
    let out = start(&mut log, &mut state, second);
    assert_eq!(header_statuses(&out), [ThreadStatus::Running]);
    assert_eq!(state, fold(log.events()));
}

#[test]
fn queued_prompt_keeps_running() {
    let (mut log, _, thread) = created();
    let first = start_turn(&mut log, "Read the lease.").unwrap();
    let mut state = fold(log.events());
    let (second, _) = prompt(&mut log, &mut state, "Then clause 3.");
    let out = push(
        &mut log,
        &mut state,
        queued(thread, second, thread, Some(first)),
    );
    assert_eq!(header_statuses(&out), []);
    assert_eq!(status(&state), ThreadStatus::Running);

    // While the lawyer is asked, the queued prompt keeps `NeedsYou`.
    let request = log.fresh().unwrap();
    push(&mut log, &mut state, permission_requested(request, None));
    let (third, _) = prompt(&mut log, &mut state, "And clause 4.");
    let out = push(
        &mut log,
        &mut state,
        queued(thread, third, thread, Some(second)),
    );
    assert_eq!(header_statuses(&out), []);
    assert_eq!(status(&state), ThreadStatus::NeedsYou);
    push(&mut log, &mut state, permission_resolved(request));
    assert_eq!(status(&state), ThreadStatus::Running);
    assert_eq!(state, fold(log.events()));
}

#[test]
fn held_queue_gives_idle() {
    let (mut log, _, thread) = created();
    let first = start_turn(&mut log, "Read the lease.").unwrap();
    let mut state = fold(log.events());
    cancel(&mut log, &mut state, first);
    assert_eq!(status(&state), ThreadStatus::Idle);
    // A prompt sent to the held queue: `Starting` from its message, then `Idle` from its wait.
    let (second, message) = prompt(&mut log, &mut state, "Then clause 3.");
    assert_eq!(status(&state), ThreadStatus::Starting);
    let out = push(
        &mut log,
        &mut state,
        queued(thread, second, thread, Some(first)),
    );
    assert_eq!(header_statuses(&out), [ThreadStatus::Idle]);
    assert!(!has_step(&state, second));
    assert_eq!(
        prompt_status(&state, message),
        Some(UserMessageStatus::Sent)
    );
    assert_eq!(state, fold(log.events()));
}

#[test]
fn own_thread_queue_while_asked_gives_needs_you() {
    // No turn runs and a request still waits: the wait behind the thread's own queue settles to
    // `NeedsYou`, not `Idle`, and to `Idle` once the request is answered.
    let (mut log, mut state, thread) = created();
    let (turn, _) = prompt(&mut log, &mut state, "Read the lease.");
    let request = log.fresh().unwrap();
    push(&mut log, &mut state, permission_requested(request, None));
    assert_eq!(status(&state), ThreadStatus::NeedsYou);
    let out = push(&mut log, &mut state, queued(thread, turn, thread, None));
    assert_eq!(header_statuses(&out), []);
    assert_eq!(status(&state), ThreadStatus::NeedsYou);
    push(&mut log, &mut state, permission_resolved(request));
    assert_eq!(status(&state), ThreadStatus::Idle);
    assert_eq!(state, fold(log.events()));
}

#[test]
fn restart_repair_of_writer_wait_gives_idle() {
    // R11's `restart_holds_writer_waiting_turn`: the first turn waits for another thread's
    // writer when the host stops; at the next start the host queues it behind its own thread,
    // with no turn ahead, and the thread is `Idle`, not `Queued`.
    let (mut log, mut state, thread) = created();
    let writer = thread_id(77).unwrap();
    let (turn, message) = prompt(&mut log, &mut state, "Read the lease.");
    push(&mut log, &mut state, queued(thread, turn, writer, None));
    assert_eq!(status(&state), ThreadStatus::Queued { behind: writer });
    let out = push(&mut log, &mut state, queued(thread, turn, thread, None));
    assert_eq!(header_statuses(&out), [ThreadStatus::Idle]);
    assert!(!has_step(&state, turn));
    assert_eq!(
        prompt_status(&state, message),
        Some(UserMessageStatus::Sent)
    );
    // The same state from the log and through a snapshot.
    assert_eq!(state, fold(log.events()));
    assert_eq!(round_trip(&state), state);
}

#[test]
fn cancel_queued_turn_ends_once() {
    // A first turn cancelled while it waits for another thread's writer: `Idle`, its prompt
    // `Removed`, no step.
    let (mut log, mut state, thread) = created();
    let writer = thread_id(77).unwrap();
    let (first, message) = prompt(&mut log, &mut state, "Read the lease.");
    push(&mut log, &mut state, queued(thread, first, writer, None));
    let out = cancel(&mut log, &mut state, first);
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    assert_eq!(status(&state), ThreadStatus::Idle);
    assert_eq!(
        prompt_status(&state, message),
        Some(UserMessageStatus::Removed)
    );
    assert!(!has_step(&state, first));
    let header = out.changes.iter().any(|change| {
        matches!(change, ViewChange::Upsert(entry)
            if matches!(&**entry, ViewEntry::Header(h) if h.status == ThreadStatus::Idle))
    });
    assert!(header, "{:?}", out.changes);

    // A second turn that waits for the writer after the first one ended, then is cancelled.
    let second = start_turn(&mut log, "Check clause 2.").unwrap();
    let (third, message) = (log.fresh().unwrap(), log.fresh().unwrap());
    log.push(user_message(third, message, "Then clause 3."))
        .unwrap();
    log.push(queued(thread, third, thread, Some(second)))
        .unwrap();
    let checkpoint = log.fresh().unwrap();
    log.push(turn_ended(second, checkpoint).unwrap()).unwrap();
    log.push(queued(thread, third, writer, None)).unwrap();
    let mut state = fold(log.events());
    assert_eq!(status(&state), ThreadStatus::Queued { behind: writer });
    // The wait made `third` the thread's last turn; a snapshot keeps it, so a cancel on the
    // restored state settles too.
    assert_eq!(round_trip(&state), state);
    let mut restored = round_trip(&state);
    let ended = log.events().len();
    let out = cancel(&mut log, &mut state, third);
    apply(&mut restored, &log.events()[ended]).unwrap();
    assert_eq!(restored, state);
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    assert_eq!(status(&state), ThreadStatus::Idle);
    assert_eq!(
        prompt_status(&state, message),
        Some(UserMessageStatus::Removed)
    );
    assert!(!has_step(&state, third));
    assert!(has_step(&state, second));
    assert_eq!(state, fold(log.events()));
}
