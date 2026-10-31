//! The thread's status around questions to the lawyer and prompts that end before they start:
//! an expired question settles like an answered one, a turn starts as `NeedsYou` while one
//! waits, a restored state still counts the questions that wait, and the end of an earlier
//! prompt's turn leaves the `Starting` of a later one.

use clauseo_protocol::{
    ElicitationExpired, ElicitationOutcome, ElicitationRequest, ElicitationRequested,
    ElicitationResolved, ElicitationState, ExpiryReason, MessageId, PermissionExpired,
    PermissionRequestId, PermissionState, Principal, ThreadStatus, TurnId, TurnStarted,
    UserMessageStatus, ViewEntry,
};
use clauseo_reducer::{StreamState, apply};

use crate::fixtures::{
    Log, permission_requested, permission_resolved, start_turn, thread_created, user_message,
};
use crate::helpers::{cancel, first_upsert, fold, prompt_status, round_trip};

/// The thread's status.
#[cfg(test)]
fn status(state: &StreamState) -> ThreadStatus {
    state.as_thread().unwrap().status()
}

#[test]
fn an_expired_permission_is_settled_and_the_turn_runs_on() {
    let mut log = Log::thread(23).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    start_turn(&mut log, "Edit clause 1.").unwrap();
    let request: PermissionRequestId = log.fresh().unwrap();
    log.push(permission_requested(request, None)).unwrap();
    let mut state = fold(log.events());
    assert_eq!(status(&state), ThreadStatus::NeedsYou);

    let reason = ExpiryReason::TurnEnded;
    let expired = PermissionExpired { request, reason };
    let out = apply(&mut state, &log.push(expired).unwrap()).unwrap();
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    let ViewEntry::Permission(entry) = first_upsert(&out.changes) else {
        panic!("{:?}", out.changes);
    };
    assert_eq!(entry.state, PermissionState::Expired { reason });
    assert_eq!(status(&state), ThreadStatus::Running);
}

#[test]
fn an_expired_elicitation_is_settled_and_the_turn_runs_on() {
    let mut log = Log::thread(23).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    start_turn(&mut log, "File the notice.").unwrap();
    let elicitation = log.fresh().unwrap();
    let request = ElicitationRequest::Url {
        message: "Sign in to the court portal.".to_owned(),
        url: "https://example.org/login".to_owned(),
    };
    log.push(ElicitationRequested {
        elicitation,
        request,
    })
    .unwrap();
    let mut state = fold(log.events());
    assert_eq!(status(&state), ThreadStatus::NeedsYou);

    let reason = ExpiryReason::AgentExited;
    let expired = ElicitationExpired {
        elicitation,
        reason,
    };
    let out = apply(&mut state, &log.push(expired).unwrap()).unwrap();
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    let ViewEntry::Elicitation(entry) = first_upsert(&out.changes) else {
        panic!("{:?}", out.changes);
    };
    assert_eq!(entry.state, ElicitationState::Expired { reason });
    assert_eq!(status(&state), ThreadStatus::Running);
}

#[test]
fn a_turn_that_starts_while_a_question_waits_needs_the_lawyer() {
    let mut log = Log::thread(23).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let (turn, message): (TurnId, MessageId) = (log.fresh().unwrap(), log.fresh().unwrap());
    log.push(user_message(turn, message, "Edit clause 1."))
        .unwrap();
    let request: PermissionRequestId = log.fresh().unwrap();
    log.push(permission_requested(request, None)).unwrap();
    let mut state = fold(log.events());
    let checkpoint_before = log.fresh().unwrap();
    let started = TurnStarted {
        turn,
        checkpoint_before,
    };
    apply(&mut state, &log.push(started).unwrap()).unwrap();
    assert_eq!(status(&state), ThreadStatus::NeedsYou);
    // Once answered, the turn runs.
    apply(&mut state, &log.push(permission_resolved(request)).unwrap()).unwrap();
    assert_eq!(status(&state), ThreadStatus::Running);
}

#[test]
fn a_state_read_from_a_snapshot_counts_the_questions_that_wait() {
    // A permission and an elicitation wait when the snapshot is taken.
    let mut log = Log::thread(23).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    start_turn(&mut log, "Edit clause 1 and file it.").unwrap();
    let request: PermissionRequestId = log.fresh().unwrap();
    log.push(permission_requested(request, None)).unwrap();
    let elicitation = log.fresh().unwrap();
    let asked = ElicitationRequest::Url {
        message: "Sign in to the court portal.".to_owned(),
        url: "https://example.org/login".to_owned(),
    };
    log.push(ElicitationRequested {
        elicitation,
        request: asked,
    })
    .unwrap();
    let folded = fold(log.events());
    let mut state = round_trip(&folded);
    assert_eq!(state, folded);
    assert_eq!(status(&state), ThreadStatus::NeedsYou);

    // One of two answered: the other still waits.
    apply(&mut state, &log.push(permission_resolved(request)).unwrap()).unwrap();
    assert_eq!(status(&state), ThreadStatus::NeedsYou);
    let resolved = ElicitationResolved {
        elicitation,
        outcome: ElicitationOutcome::Accepted,
        by: Principal::LocalDesktop,
    };
    apply(&mut state, &log.push(resolved).unwrap()).unwrap();
    assert_eq!(status(&state), ThreadStatus::Running);
}

#[test]
fn ending_an_earlier_prompts_turn_leaves_the_later_prompt_starting() {
    // Two prompts sent while no turn runs; the first one's turn is cancelled before it starts.
    let mut log = Log::thread(23).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let (first, first_message): (TurnId, MessageId) = (log.fresh().unwrap(), log.fresh().unwrap());
    log.push(user_message(first, first_message, "Draft the notice."))
        .unwrap();
    let (later, later_message): (TurnId, MessageId) = (log.fresh().unwrap(), log.fresh().unwrap());
    log.push(user_message(later, later_message, "Then the cover letter."))
        .unwrap();
    let mut state = fold(log.events());
    assert_eq!(status(&state), ThreadStatus::Starting);

    let out = cancel(&mut log, &mut state, first);
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    assert_eq!(
        prompt_status(&state, first_message),
        Some(UserMessageStatus::Removed)
    );
    assert_eq!(
        prompt_status(&state, later_message),
        Some(UserMessageStatus::Sent)
    );
    assert_eq!(status(&state), ThreadStatus::Starting);

    // The later prompt's own turn ending settles it.
    cancel(&mut log, &mut state, later);
    assert_eq!(status(&state), ThreadStatus::Idle);
}
