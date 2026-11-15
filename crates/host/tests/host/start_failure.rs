//! A first launch that fails before its session is ready ends the waiting turn with the code
//! whose words are true: an error the agent answered a start request with (Rohan's -32603 to
//! `session/new`, -32602, -32601, an error to `initialize`) and an agent that exits before its
//! session is ready are `agent.launch_failed` ("didn't start"); only an answer that does not
//! decode is `agent.protocol_error` ("sent something Clauseo can't read").

use clauseo_protocol::{DomainEvent, ErrorCode, StopReason, StreamId};

use crate::support::{Dir, FAKE, Scenario, matter_with_thread};

/// The code `scenario`'s first launch fails the first turn with, and its detail.
fn first_turn_fails(test: &str, scenario: &'static str) -> (ErrorCode, String) {
    let dir = Dir::new(test, &[(FAKE, Scenario::Local(scenario))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the brief.");
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::Failed, "{}", events.dump());
    let failed = events.events().find_map(|event| match event {
        DomainEvent::TurnFailed(failed) if failed.turn == turn => {
            Some((failed.code, failed.detail.clone().unwrap_or_default()))
        }
        _ => None,
    });
    host.shutdown();
    failed.unwrap_or_else(|| panic!("no TurnFailed: {}", events.dump()))
}

/// Rohan's case: `session/new` answered with -32603 is a failed start, never unreadable.
#[test]
fn an_internal_error_to_session_new_is_a_failed_start() {
    let (code, _) = first_turn_fails("start_new_session_internal", "new-session-error");
    assert_eq!(code, ErrorCode::AgentLaunchFailed);
}

#[test]
fn invalid_params_to_session_new_is_a_failed_start() {
    let (code, _) = first_turn_fails("start_new_session_params", "new-session-invalid-params");
    assert_eq!(code, ErrorCode::AgentLaunchFailed);
}

#[test]
fn method_not_found_to_session_new_is_a_failed_start() {
    let (code, _) = first_turn_fails("start_new_session_method", "new-session-no-method");
    assert_eq!(code, ErrorCode::AgentLaunchFailed);
}

#[test]
fn an_error_to_initialize_is_a_failed_start() {
    let (code, _) = first_turn_fails("start_initialize_error", "initialize-error");
    assert_eq!(code, ErrorCode::AgentLaunchFailed);
}

/// Exits on `session/new` without answering: "didn't start", not "the answer stopped".
#[test]
fn an_exit_before_the_session_is_ready_is_a_failed_start() {
    let (code, detail) = first_turn_fails("start_new_session_exit", "new-session-exit");
    assert_eq!(code, ErrorCode::AgentLaunchFailed, "{detail}");
}

/// Answers `initialize` and exits at once: whichever the host sees first, a failed start.
#[test]
fn an_exit_right_after_initialize_is_a_failed_start() {
    let (code, detail) = first_turn_fails("start_initialize_exit", "initialize-then-exit");
    assert_eq!(code, ErrorCode::AgentLaunchFailed, "{detail}");
}

/// The one unreadable case: a `session/new` answer that does not decode.
#[test]
fn an_answer_that_does_not_decode_is_unreadable() {
    let (code, _) = first_turn_fails("start_new_session_malformed", "new-session-malformed");
    assert_eq!(code, ErrorCode::AgentProtocolError);
}
