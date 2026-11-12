//! A turn whose answer carries a failure the agent gave up on (`gave_up.rs`): codex-acp 2.2.2's
//! `internal_error` on its `end_turn`. The turn fails `agent.protocol_error` with the answer so
//! far kept, and the agent's title reaches no event, the failure's detail included. A too-long
//! refusal before it wins.

use clauseo_protocol::{ErrorCode, StopReason};
use serde_json::{Value, json};

use super::failures::until_ended;
use super::offline::{answer, ended, prompting};
use super::rig::Rig;

const PROMPT: &str =
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}"#;

/// codex-acp's synthetic title for Codex's internal error.
const INTERNAL: &str = "Codex encountered an internal error.";

/// A script line sending `update`.
fn update(update: &Value) -> String {
    json!({"update": {"update": update}}).to_string()
}

/// `_meta` holding an AIR `sessionFailure` of severity `error`.
fn failure(category: &str, title: &str, actions: &Value) -> Value {
    json!({"jetbrains": {"air": {"version": 1, "sessionFailure": {
        "id": "turn-1:error", "revision": 1, "category": category, "severity": "error",
        "title": title, "actions": actions,
    }}}})
}

/// The script: the prompt, an answer chunk, `before`, then `end_turn` with Codex's internal error.
fn script(before: &[String]) -> String {
    let chunk = update(
        &json!({"sessionUpdate": "agent_message_chunk", "messageId": "m-1",
        "content": {"type": "text", "text": "Clause five says"}}),
    );
    let meta = failure("service", INTERNAL, &json!(["retry", "new_session"]));
    let end = json!({"respond": {"to": "p1", "result": {"stopReason": "end_turn", "_meta": meta}}});
    let mut lines = vec![PROMPT.to_owned(), chunk];
    lines.extend_from_slice(before);
    lines.push(end.to_string());
    let mut script = lines.join("\n");
    script.push('\n');
    script
}

#[test]
fn an_internal_error_the_agent_gave_up_on_fails_the_turn_and_shows_none_of_its_text() {
    let mut rig = Rig::start("gave-up-internal", &script(&[]));
    let (thread, turn) = prompting(&mut rig);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (
            Some(StopReason::Failed),
            Some(ErrorCode::AgentProtocolError)
        )
    );
    assert_eq!(answer(&log, turn), "Clause five says");
    let shown = format!("{log:?}");
    assert!(!shown.contains("internal error"), "{shown}");
    rig.finish();
}

#[test]
fn a_too_long_refusal_wins_over_a_failure_the_agent_gave_up_on() {
    let too_long = failure("service", "Prompt is too long", &json!(["retry"]));
    let before = [update(
        &json!({"sessionUpdate": "session_info_update", "_meta": too_long}),
    )];
    let mut rig = Rig::start("gave-up-too-long", &script(&before));
    let (thread, turn) = prompting(&mut rig);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(ended(&log, turn), (Some(StopReason::TooLong), None));
    rig.finish();
}
