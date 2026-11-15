//! A Tools change reaching a live agent (`idle/tools.rs`) with a real `fake_agent`: an idle agent
//! whose servers changed is stopped and its next prompt relaunches it with the new servers (the
//! agent's `session/new` names them); an agent in a turn, with a permission open and a prompt
//! queued, is stopped only after both turns; an agent whose servers did not change keeps
//! running; two quick changes make one stop.
//!
//! Each scenario's `session/new` is a `one_of`: the first launch gets no server, the relaunch
//! gets `notes` and answers "With notes.", so a relaunch that sent other servers fails its turn.

use clauseo_protocol::{AgentChoiceId, DomainEvent, HostCommand, McpCommand, StopReason, ThreadId};
use serde_json::{Value, json};

use super::agents::send_prompt;
use super::idle::{idle_stops, live, until_ended, until_idle};
use super::rig::{HANDSHAKE, Rig};
use crate::actor::Actor;
use crate::actor::input::Input;
use crate::actor::turn::idle::Relaunch;
use crate::agent::AgentInput;
use crate::config::IDLE_LIMIT;
use crate::pipeline::tests::support::TempDir;

/// The relaunch's branch: `session/new` with `notes` alone, then one prompt answered.
fn with_notes() -> Value {
    json!([
        {"expect":{"method":"session/new","as":"new2","params":{
            "mcpServers":[{"name":"notes","command":"{{any}}"}]}}},
        {"respond":{"to":"new2","result":{"sessionId":"sess-1"}}},
        {"expect":{"method":"session/prompt","as":"p-notes","params":{"sessionId":"sess-1"}}},
        {"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-n",
            "content":{"type":"text","text":"With notes."}}}},
        {"respond":{"to":"p-notes","result":{"stopReason":"end_turn"}}}
    ])
}

/// The first launch's branch: `session/new` with no server, then `steps`.
fn first(steps: &[Value]) -> Value {
    let mut branch = vec![
        json!({"expect":{"method":"session/new","as":"new","params":{"mcpServers":[]}}}),
        json!({"respond":{"to":"new","result":{"sessionId":"sess-1"}}}),
    ];
    branch.extend_from_slice(steps);
    Value::Array(branch)
}

/// One prompt answered "First.".
fn one_answer() -> Vec<Value> {
    vec![
        json!({"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}),
        json!({"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-1",
            "content":{"type":"text","text":"First."}}}}),
        json!({"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}),
    ]
}

/// A rig whose settings hold `notes` (a local server, the fake agent's own program) and `web` (a
/// remote one), both off, with the idle stop at its limit and no cap; the first launch plays
/// `steps`.
fn rig(test: &str, steps: &[Value]) -> Rig {
    let dir = TempDir::new(test);
    let root = dir.path();
    let one_of = json!({"one_of":[first(steps), with_notes()]});
    Rig::prepare(root, test, &format!("{one_of}\n"), None);
    // The handshake's own `session/new` gives way to the `one_of`.
    let scenario = root.join(format!("{test}.jsonl"));
    let text = std::fs::read_to_string(&scenario).unwrap();
    let new = HANDSHAKE.lines().skip(2).collect::<Vec<_>>().join("\n") + "\n";
    assert!(text.contains(&new), "the handshake's session/new not found");
    let text = text.replacen(&new, "", 1);
    crate::write::write_file(&scenario, text.as_bytes()).unwrap();
    let settings_path = root.join("data").join("settings.json");
    let mut settings: Value =
        serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
    let program = clauseo_fake_agent::bin_path().unwrap();
    settings["mcp"] = json!({"servers":{
        "notes":{"command":program.display().to_string(),"enabled":false},
        "web":{"url":"https://tools.example.com/mcp","enabled":false},
    }});
    crate::write::write_file(&settings_path, settings.to_string().as_bytes()).unwrap();
    Rig::open_with(dir, 0, None, |config| {
        // The program as typed is the path: no search of this computer's paths in a test.
        let resolve = |typed: &str| Some(std::path::PathBuf::from(typed));
        config
            .with_idle_limit(IDLE_LIMIT)
            .with_live_agents(usize::MAX)
            .with_mcp_resolver(std::sync::Arc::new(resolve))
    })
}

/// Turns `server` on as command `n`, holding back any agent's end.
fn turn_on(rig: &mut Rig, n: u128, server: &str) {
    let command = HostCommand::Mcp(McpCommand::SetServerEnabled {
        name: server.to_owned(),
        enabled: true,
    });
    rig.dispatch_holding(n, command, agent_end).unwrap();
}

/// Whether `input` is an agent's end.
fn agent_end(_: &Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Agent {
            input: AgentInput::Ended(_),
            ..
        }
    )
}

/// Whether `thread`'s agent is marked for a Tools change.
fn stale(actor: &Actor, thread: ThreadId) -> bool {
    let agent = actor.threads[&thread].turn.agent.as_deref().unwrap();
    agent.idle.relaunch == Some(Relaunch::ToolsChanged)
}

/// How many turns of `thread` ended `EndTurn`.
fn turns_ended(rig: &mut Rig, thread: ThreadId) -> usize {
    rig.log(thread)
        .iter()
        .filter(|event| {
            matches!(event, DomainEvent::TurnEnded(ended) if ended.stop == StopReason::EndTurn)
        })
        .count()
}

/// The next prompt relaunches the agent, whose `session/new` names `notes`: its answer is the
/// relaunch branch's.
fn relaunches_with_notes(rig: &mut Rig, n: u128, thread: ThreadId) {
    let before = turns_ended(rig, thread);
    send_prompt(rig, n, thread);
    rig.run_until("the relaunched turn's end", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && state.agent.is_some() && actor.work.commits == 0
    });
    assert_eq!(turns_ended(rig, thread), before + 1);
    let log = format!("{:?}", rig.log(thread));
    assert!(log.contains("With notes."), "not the relaunch's answer");
}

#[test]
fn an_idle_agent_is_stopped_and_relaunched_with_the_new_servers() {
    let mut rig = rig("tools-live-idle", &one_answer());
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    turn_on(&mut rig, 11, "notes");
    // The tick after the command stopped it; its end was held back.
    assert!(!live(&rig.actor, thread), "not stopped");
    rig.release_held();
    until_ended(&mut rig, thread);
    assert_eq!(idle_stops(&mut rig, thread), 1);
    relaunches_with_notes(&mut rig, 12, thread);
    assert!(live(&rig.actor, thread));
    assert!(
        !stale(&rig.actor, thread),
        "the relaunch has the new servers"
    );
    rig.finish();
}

#[test]
fn an_agent_in_a_turn_is_stopped_after_its_queue_and_its_ask() {
    let steps = vec![
        json!({"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}),
        json!({"request":{"id":"perm-1","method":"session/request_permission","params":{
            "sessionId":"sess-1","toolCall":{"toolCallId":"t-1","title":"Edit brief.txt",
            "kind":"edit","status":"pending"},"options":[{"optionId":"allow-once",
            "name":"Allow once","kind":"allow_once"}]}}}),
        json!({"expect_response":{"id":"perm-1","result":{"outcome":{"outcome":"selected",
            "optionId":"allow-once"}}}}),
        json!({"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}),
        json!({"expect":{"method":"session/prompt","as":"p2","params":{"sessionId":"sess-1"}}}),
        json!({"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-2",
            "content":{"type":"text","text":"Queued answer."}}}}),
        json!({"respond":{"to":"p2","result":{"stopReason":"end_turn"}}}),
    ];
    let mut rig = rig("tools-live-turn", &steps);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    rig.run_until("the permission request", |actor| {
        actor.threads[&thread]
            .turn
            .agent
            .as_deref()
            .is_some_and(|agent| !agent.permissions.is_empty())
    });
    // A prompt queued behind the turn, then the change: the agent is marked, not stopped.
    send_prompt(&mut rig, 11, thread);
    turn_on(&mut rig, 12, "notes");
    assert!(live(&rig.actor, thread), "stopped in its turn");
    assert!(stale(&rig.actor, thread));
    let agent = rig.actor.threads[&thread].turn.agent.as_deref().unwrap();
    let request = *agent.permissions.keys().next().unwrap();
    let answer = HostCommand::AnswerPermission {
        thread,
        request,
        option_id: AgentChoiceId::new("allow-once").unwrap(),
    };
    rig.dispatch(13, answer).unwrap();
    // Both turns run on the agent launched first, then it stops, once.
    until_ended(&mut rig, thread);
    assert_eq!(turns_ended(&mut rig, thread), 2);
    let log = format!("{:?}", rig.log(thread));
    assert!(log.contains("Queued answer."), "the queued prompt was lost");
    assert_eq!(idle_stops(&mut rig, thread), 1);
    relaunches_with_notes(&mut rig, 14, thread);
    rig.finish();
}

#[test]
fn an_agent_whose_servers_did_not_change_keeps_running() {
    let mut rig = rig("tools-live-same", &one_answer());
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    // A remote server: the fake agent takes none (no `mcpCapabilities.http`), so its servers
    // stay the same.
    turn_on(&mut rig, 11, "web");
    rig.settle_commits();
    assert!(rig.held.is_empty(), "the agent ended");
    assert!(live(&rig.actor, thread), "stopped with the same servers");
    assert!(!stale(&rig.actor, thread));
    let agent = rig.actor.threads[&thread].turn.agent.as_deref().unwrap();
    assert_eq!(
        agent.idle.tools_seen, rig.actor.mcp.tools_rev,
        "never compared"
    );
    assert_eq!(idle_stops(&mut rig, thread), 0);
    rig.finish();
}

#[test]
fn two_quick_changes_make_one_stop() {
    let mut rig = rig("tools-live-twice", &one_answer());
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    // The second change comes while the first one's stop is under way.
    turn_on(&mut rig, 11, "notes");
    turn_on(&mut rig, 12, "web");
    rig.release_held();
    until_ended(&mut rig, thread);
    assert_eq!(idle_stops(&mut rig, thread), 1);
    // Nothing relaunched it: the next prompt does, with `notes` alone (no remote server for
    // this agent).
    relaunches_with_notes(&mut rig, 13, thread);
    assert_eq!(idle_stops(&mut rig, thread), 1);
    rig.finish();
}
