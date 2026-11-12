//! A host killed while an agent-started turn checkpoints: its `TurnQueued` is durable, its
//! `TurnStarted` and `TurnEnded` are not. The next start ends it `Cancelled` before it started
//! (no step), so the thread is `Idle` (the app shows Ask, not Stop), and a new prompt runs.

use clauseo_protocol::{DomainEvent, StopReason, ThreadId, ThreadStatus, TurnId};

use super::agents::send_prompt;
use super::own_cancel::{At, OWN_THEN_LATE, own_turn_at, view_status};
use super::rig::{Rig, kind};

/// The next run's agent: one prompt, ended `end_turn`.
const ONE_PROMPT: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
"#;

fn ends(log: &[DomainEvent], turn: TurnId) -> Vec<StopReason> {
    log.iter()
        .filter_map(|event| {
            if let DomainEvent::TurnEnded(ended) = event
                && ended.turn == turn
            {
                Some(ended.stop)
            } else {
                None
            }
        })
        .collect()
}

/// The thread's status as the app reads it (its row, from the loaded view or the header kept as
/// the stream unloaded).
fn status(rig: &Rig, thread: ThreadId) -> ThreadStatus {
    crate::actor::rail::thread_facts(&rig.actor, &rig.actor.threads[&thread]).status
}

#[test]
fn a_restart_ends_an_agent_started_turn_killed_while_it_checkpoints() {
    let test = "own-restart-checkpointing";
    let mut rig = Rig::start(test, OWN_THEN_LATE);
    let thread = rig.thread();
    own_turn_at(&mut rig, thread, At::Checkpointing);
    rig.settle_commits();
    let own = rig.actor.threads[&thread]
        .turn
        .current
        .as_ref()
        .unwrap()
        .turn;
    assert_eq!(view_status(&rig, thread), ThreadStatus::Starting);
    // Killed: nothing after the open reaches the log.
    rig.fail(usize::MAX, |_| true);
    rig.release_held();
    let dir = rig.close();
    Rig::prepare(dir.path(), test, ONE_PROMPT, None);
    let mut rig = Rig::open(dir, 1, None);
    rig.settle_commits();

    let log = rig.log(thread);
    let kinds: Vec<_> = log.iter().map(kind).collect();
    assert_eq!(ends(&log, own), [StopReason::Cancelled], "{kinds:?}");
    let started = log
        .iter()
        .any(|e| matches!(e, DomainEvent::TurnStarted(s) if s.turn == own));
    assert!(!started, "{kinds:?}");
    assert_eq!(status(&rig, thread), ThreadStatus::Idle, "{kinds:?}");

    // A new prompt runs.
    let next = send_prompt(&mut rig, 20, thread);
    rig.run_until("the prompt's end", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && state.closing.is_none() && actor.work.commits == 0
    });
    assert_eq!(ends(&rig.log(thread), next), [StopReason::EndTurn]);
    assert_eq!(status(&rig, thread), ThreadStatus::Idle);
    let count = rig.log(thread).len();

    // A third start repairs nothing more.
    let dir = rig.close();
    let mut rig = Rig::open(dir, 2, None);
    rig.settle_commits();
    let log = rig.log(thread);
    let new: Vec<_> = log[count..].iter().map(kind).collect();
    assert_eq!(
        new,
        ["agent_session_detached"],
        "only the shutdown's detach"
    );
    rig.finish();
}
