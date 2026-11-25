//! A helper's asks (`helpers::of_ask`): a Codex helper's permission request, which codex-acp
//! sends under the helper's session id (fixture X8, `fake_agent`'s `codex-helpers` scenario), names
//! the helper in the log and in the view; a Claude helper's, which names only its step, names
//! the helper that stamped the step; the lead's own names none.

use clauseo_acp::{AcpEvent, Origin, PendingKey};
use clauseo_protocol::{
    AgentChoiceId, AgentKind, DomainEvent, HelperId, HostCommand, StreamId, TextOrBlob, ThreadId,
    ToolCallStatus, ToolKind, ViewEntry,
};

use super::agents::send_prompt;
use super::codex_helpers::recorded_steps;
use super::failures::until_ended;
use super::helpers::{feed, live_turn, one_turn, session};
use super::rig::Rig;

/// The `helper` of each permission request in `thread`'s log, in order.
fn asked_by(rig: &mut Rig, thread: ThreadId) -> Vec<Option<HelperId>> {
    rig.log(thread)
        .iter()
        .filter_map(|event| {
            let DomainEvent::PermissionRequested(asked) = event else {
                return None;
            };
            Some(asked.helper)
        })
        .collect()
}

/// The helpers started in `thread`'s log, in order.
fn started(rig: &mut Rig, thread: ThreadId) -> Vec<HelperId> {
    rig.log(thread)
        .iter()
        .filter_map(|event| {
            let DomainEvent::HelperStarted(started) = event else {
                return None;
            };
            Some(started.helper)
        })
        .collect()
}

/// The recorded helper's permission request (X8's, put under X1's helper) names that helper,
/// in the logged event and in the view's entry, and the lawyer's answer reaches the agent.
#[test]
fn a_codex_helpers_permission_request_names_the_helper() {
    let mut rig = Rig::start("codex-helper-ask", &recorded_steps(true));
    rig.actor.turn_path.kind_for_tests = Some(AgentKind::Codex);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    rig.run_until("the helper's request", |actor| {
        actor.threads[&thread]
            .turn
            .agent
            .as_deref()
            .is_some_and(|agent| !agent.permissions.is_empty())
    });
    let agent = rig.actor.threads[&thread].turn.agent.as_deref().unwrap();
    let request = *agent.permissions.keys().next().unwrap();
    let answer = HostCommand::AnswerPermission {
        thread,
        request,
        option_id: AgentChoiceId::new("allow_once").unwrap(),
    };
    rig.dispatch(11, answer).unwrap();
    until_ended(&mut rig, thread);
    let helpers = started(&mut rig, thread);
    assert_eq!(helpers.len(), 1);
    assert_eq!(asked_by(&mut rig, thread), [Some(helpers[0])]);
    let loaded = rig.actor.loaded(StreamId::Thread(thread)).unwrap();
    let state = loaded.state.as_thread().unwrap();
    let entry = state.entries().values().find_map(|entry| {
        let ViewEntry::Permission(entry) = entry else {
            return None;
        };
        Some(entry.helper)
    });
    assert_eq!(entry, Some(Some(helpers[0])));
    rig.finish();
}

/// `acp_id`'s permission request, as the ACP reader hands it over, from `helper` when given.
fn ask(acp_id: &str, key: u64, helper: Option<&str>, origin: Origin) -> AcpEvent {
    AcpEvent::PermissionRequested {
        session: session(),
        origin,
        key: PendingKey(key),
        acp_tool_call_id: acp_id.into(),
        options: Vec::new(),
        paths: Vec::new(),
        raw_input: None,
        helper: helper.map(Into::into),
    }
}

/// A Claude helper's request names only its step: it names the helper that stamped that step.
/// The lead's request on its own step names none, nor does one from a helper never started.
#[test]
fn a_claude_helpers_permission_request_names_the_helper_of_its_step() {
    let mut rig = Rig::start("claude-helper-ask", &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = live_turn(&mut rig, thread);
    let at = Origin::Turn(turn);
    let launch = clauseo_acp::ToolCall {
        acp_id: "toolu_A".into(),
        title: "Agent".into(),
        kind: ToolKind::Other,
        tool_name: Some("Agent".into()),
        status: ToolCallStatus::Pending,
        locations: Vec::new(),
        raw_input: Some(TextOrBlob::Inline(
            r#"{"subagent_type":"paper-reader","prompt":"Read."}"#.to_owned(),
        )),
        raw_output: None,
        content: Vec::new(),
    };
    let step = clauseo_acp::ToolCall {
        acp_id: "toolu_R1".into(),
        title: "Read".into(),
        tool_name: Some("Read".into()),
        raw_input: None,
        ..launch.clone()
    };
    let update = |update| AcpEvent::Update {
        session: session(),
        origin: at,
        update,
    };
    feed(
        &mut rig,
        thread,
        update(clauseo_acp::AgentUpdate::ToolCall(launch)),
    );
    let stamped = AcpEvent::HelperUpdate {
        session: session(),
        origin: at,
        helper: "toolu_A".into(),
        update: clauseo_acp::AgentUpdate::ToolCall(step),
    };
    feed(&mut rig, thread, stamped);
    feed(&mut rig, thread, ask("toolu_R1", 5, None, at));
    feed(&mut rig, thread, ask("tc-0", 6, None, at));
    feed(&mut rig, thread, ask("toolu_X1", 7, Some("toolu_X"), at));
    rig.release_held();
    until_ended(&mut rig, thread);
    let helpers = started(&mut rig, thread);
    assert_eq!(helpers.len(), 1);
    assert_eq!(asked_by(&mut rig, thread), [Some(helpers[0]), None, None]);
    rig.finish();
}
