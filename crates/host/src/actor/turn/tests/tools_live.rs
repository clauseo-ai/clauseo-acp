//! A Tools change reaching a live agent (`idle/tools.rs`) with a real `fake_agent`: an idle agent
//! whose servers changed is stopped once idle for the grace (`cap_min_idle`), and its next prompt
//! relaunches it with the new servers (the agent's `session/new` names them); an agent in a turn,
//! with a permission open, is stopped only after the turn and the grace; a
//! background command's agent-started turn within the grace keeps its agent; output held for a
//! turn keeps it; an agent whose servers did not change keeps running; two quick changes make one
//! stop.
//!
//! Each scenario's `session/new` is a `one_of`: the first launch gets no server, the relaunch
//! gets `notes` and answers "With notes.", so a relaunch that sent other servers fails its turn.

use clauseo_protocol::{AgentChoiceId, DomainEvent, HostCommand, McpCommand, StopReason, ThreadId};
use serde_json::{Value, json};

use super::agents::send_prompt;
use std::cell::Cell;

use clauseo_acp::{AcpEvent, AgentUpdate, Origin};
use clauseo_protocol::AgentSessionId;

use super::idle::{MS, advance, idle_since, idle_stops, live, pending_of, until_ended, until_idle};
use super::rig::{HANDSHAKE, Rig};
use crate::actor::Actor;
use crate::actor::input::Input;
use crate::actor::turn::idle::{Pending, Relaunch};
use crate::actor::turn::own_turn;
use crate::agent::AgentInput;
use crate::config::{CAP_MIN_IDLE, HostConfig, IDLE_LIMIT};
use crate::pipeline::tests::support::TempDir;

/// The relaunch's branch: `session/new` with `notes` alone, then one prompt answered.
pub(super) fn with_notes() -> Value {
    let mut branch = vec![
        json!({"expect":{"method":"session/new","as":"new2","params":{
            "mcpServers":[{"name":"notes","command":"{{any}}"}]}}}),
        json!({"respond":{"to":"new2","result":{"sessionId":"sess-1"}}}),
    ];
    branch.extend(answer_with_notes());
    Value::Array(branch)
}

/// The relaunch's prompt, answered "With notes.".
pub(super) fn answer_with_notes() -> Vec<Value> {
    vec![
        json!({"expect":{"method":"session/prompt","as":"p-notes","params":{"sessionId":"sess-1"}}}),
        json!({"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-n",
            "content":{"type":"text","text":"With notes."}}}}),
        json!({"respond":{"to":"p-notes","result":{"stopReason":"end_turn"}}}),
    ]
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
pub(super) fn one_answer() -> Vec<Value> {
    vec![
        json!({"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}),
        json!({"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-1",
            "content":{"type":"text","text":"First."}}}}),
        json!({"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}),
    ]
}

/// A rig whose settings hold `notes` (a local server, the fake agent's own program) and `web` (a
/// remote one), both off, with the idle stop at its limit and no cap; the first launch plays
/// `steps`, the relaunch [`with_notes`].
fn rig(test: &str, steps: &[Value]) -> Rig {
    rig_with(test, steps, &with_notes(), false)
}

/// [`rig`], with the relaunch's branch `relaunch`, and the agent taking `session/load` when
/// `load`.
pub(super) fn rig_with(test: &str, steps: &[Value], relaunch: &Value, load: bool) -> Rig {
    rig_adjusted(test, steps, relaunch, load, |config| config)
}

/// [`rig_with`], its host config then given to `adjust`.
pub(super) fn rig_adjusted(
    test: &str,
    steps: &[Value],
    relaunch: &Value,
    load: bool,
    adjust: impl FnOnce(HostConfig) -> HostConfig,
) -> Rig {
    let dir = TempDir::new(test);
    let root = dir.path();
    let one_of = json!({"one_of":[first(steps), relaunch]});
    Rig::prepare(root, test, &format!("{one_of}\n"), None);
    // The handshake's own `session/new` gives way to the `one_of`.
    let scenario = root.join(format!("{test}.jsonl"));
    let text = std::fs::read_to_string(&scenario).unwrap();
    let new = HANDSHAKE.lines().skip(2).collect::<Vec<_>>().join("\n") + "\n";
    assert!(text.contains(&new), "the handshake's session/new not found");
    let mut text = text.replacen(&new, "", 1);
    if load {
        text = text.replacen(r#""loadSession":false"#, r#""loadSession":true"#, 1);
    }
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
        adjust(
            config
                .with_idle_limit(IDLE_LIMIT)
                .with_live_agents(usize::MAX)
                .with_mcp_resolver(std::sync::Arc::new(resolve)),
        )
    })
}

/// Turns `server` on as command `n`, holding back any agent's end.
pub(super) fn turn_on(rig: &mut Rig, n: u128, server: &str) {
    let command = HostCommand::Mcp(McpCommand::SetServerEnabled {
        name: server.to_owned(),
        enabled: true,
    });
    rig.dispatch_holding(n, command, agent_end).unwrap();
}

/// Whether `input` is an agent's end.
pub(super) fn agent_end(_: &Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Agent {
            input: AgentInput::Ended(_),
            ..
        }
    )
}

/// Whether `thread`'s agent is marked for a Tools change.
pub(super) fn stale(actor: &Actor, thread: ThreadId) -> bool {
    let agent = actor.threads[&thread].turn.agent.as_deref().unwrap();
    agent.idle.relaunch == Some(Relaunch::ToolsChanged)
}

/// `thread`'s marked agent, idle since `since`, is live a millisecond before the grace ends and
/// stopped at it, once.
pub(super) fn stopped_after_the_grace(rig: &mut Rig, thread: ThreadId, since: std::time::Duration) {
    let at = since + CAP_MIN_IDLE;
    assert_eq!(rig.actor.turn_path.idle.armed_at(), Some(at));
    let now = rig.actor.now().mono;
    advance(rig, at.checked_sub(now + MS).unwrap());
    assert!(live(&rig.actor, thread), "stopped before the grace");
    // The clock alone moves; the agent's end is held, not drained (#1050).
    rig.clock.advance(MS);
    rig.hold_next("the agent's end", agent_end);
    rig.release_held();
    until_ended(rig, thread);
    assert_eq!(idle_stops(rig, thread), 1);
}

/// Whether `input` is an update the agent sent outside a turn.
fn idle_update(input: &Input) -> bool {
    let Input::Agent {
        input: AgentInput::Event(event),
        ..
    } = input
    else {
        return false;
    };
    matches!(
        **event,
        AcpEvent::Update {
            origin: Origin::Idle,
            ..
        }
    )
}

/// How many turns of `thread` ended `EndTurn`.
pub(super) fn turns_ended(rig: &mut Rig, thread: ThreadId) -> usize {
    rig.log(thread)
        .iter()
        .filter(|event| {
            matches!(event, DomainEvent::TurnEnded(ended) if ended.stop == StopReason::EndTurn)
        })
        .count()
}

/// The next prompt relaunches the agent, whose `session/new` names `notes`: its answer is the
/// relaunch branch's.
pub(super) fn relaunches_with_notes(rig: &mut Rig, n: u128, thread: ThreadId) {
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
    let since = until_idle(&mut rig, thread);
    turn_on(&mut rig, 11, "notes");
    assert!(stale(&rig.actor, thread));
    stopped_after_the_grace(&mut rig, thread, since);
    relaunches_with_notes(&mut rig, 12, thread);
    assert!(live(&rig.actor, thread));
    assert!(
        !stale(&rig.actor, thread),
        "the relaunch has the new servers"
    );
    rig.finish();
}

#[test]
fn an_agent_in_a_turn_is_stopped_after_its_ask_and_the_grace() {
    let steps = vec![
        json!({"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}),
        json!({"request":{"id":"perm-1","method":"session/request_permission","params":{
            "sessionId":"sess-1","toolCall":{"toolCallId":"t-1","title":"Edit brief.txt",
            "kind":"edit","status":"pending"},"options":[{"optionId":"allow-once",
            "name":"Allow once","kind":"allow_once"}]}}}),
        json!({"expect_response":{"id":"perm-1","result":{"outcome":{"outcome":"selected",
            "optionId":"allow-once"}}}}),
        json!({"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}),
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
    // The change in the turn: the agent is marked, not stopped.
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
    // The turn ends on the agent launched first, which stops after the grace, once. (A prompt
    // queued behind it relaunches it at once: `tools_prompt.rs`.)
    let since = until_idle(&mut rig, thread);
    assert_eq!(turns_ended(&mut rig, thread), 1);
    stopped_after_the_grace(&mut rig, thread, since);
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
    let since = until_idle(&mut rig, thread);
    turn_on(&mut rig, 11, "notes");
    turn_on(&mut rig, 12, "web");
    stopped_after_the_grace(&mut rig, thread, since);
    // Nothing relaunched it: the next prompt does, with `notes` alone (no remote server for
    // this agent).
    relaunches_with_notes(&mut rig, 13, thread);
    assert_eq!(idle_stops(&mut rig, thread), 1);
    rig.finish();
}

#[test]
fn a_background_command_s_own_turn_within_the_grace_keeps_its_agent() {
    // Prompt 1 ends with a command in the background; its result comes as a turn of the
    // agent's own (`own_turn.rs`), which the test holds back until after the Tools edit.
    let mut steps = vec![
        json!({"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}),
        json!({"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}),
        json!({"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"tc-bg-1",
            "title":"Run the build","kind":"execute","status":"completed"}}}),
        json!({"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-bg",
            "content":{"type":"text","text":"The build finished."}}}}),
    ];
    steps.push(
        json!({"update":{"update":{"sessionUpdate":"usage_update","used":10,
        "size":200_000,"cost":{"amount":0.01,"currency":"USD"}}}}),
    );
    let mut rig = rig("tools-live-background", &steps);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    let held = Cell::new(0);
    rig.run_holding(
        "the prompt's end, with the background result held",
        |_, input| {
            let hold = idle_update(input);
            held.set(held.get() + usize::from(hold));
            hold
        },
        |actor| {
            held.get() == 3
                && actor.work.commits == 0
                && actor.threads[&thread].turn.current.is_none()
                && idle_since(actor, thread).is_some()
        },
    );
    turn_on(&mut rig, 11, "notes");
    assert!(stale(&rig.actor, thread));
    // The result comes a millisecond before the grace ends: the agent still runs.
    advance(&mut rig, CAP_MIN_IDLE.checked_sub(MS).unwrap());
    assert!(live(&rig.actor, thread), "stopped within the grace");
    rig.release_held();
    rig.run_until("the agent's own turn", |actor| {
        actor.threads[&thread].turn.current.is_some()
    });
    // Past the grace, its own turn keeps it; it ends (its cost, or its quiet).
    advance(&mut rig, own_turn::QUIET * 2);
    rig.run_until("the agent's own turn's end", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && state.closing.is_none() && actor.work.commits == 0
    });
    assert!(live(&rig.actor, thread), "stopped in its own turn");
    let log = format!("{:?}", rig.log(thread));
    assert!(
        log.contains("The build finished."),
        "the background result was lost"
    );
    assert_eq!(idle_stops(&mut rig, thread), 0);
    let since = until_idle(&mut rig, thread);
    stopped_after_the_grace(&mut rig, thread, since);
    relaunches_with_notes(&mut rig, 12, thread);
    rig.finish();
}

#[test]
fn output_held_for_a_turn_keeps_a_marked_agent() {
    let mut rig = rig("tools-live-held", &one_answer());
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    turn_on(&mut rig, 11, "notes");
    // Output (a plan) waits for an agent-started turn to open.
    let event = AcpEvent::Update {
        session: AgentSessionId::new("sess-1").unwrap(),
        origin: Origin::Idle,
        update: AgentUpdate::Plan(Vec::new()),
    };
    own_turn::hold_for_test(&mut rig.actor, thread, event);
    crate::actor::turn::idle_tick(&mut rig.actor);
    advance(&mut rig, CAP_MIN_IDLE * 2);
    assert!(live(&rig.actor, thread), "stopped with output held");
    assert_eq!(pending_of(&rig.actor, thread), Some(Pending::Turn));
    assert_eq!(idle_stops(&mut rig, thread), 0);
    rig.finish();
}
