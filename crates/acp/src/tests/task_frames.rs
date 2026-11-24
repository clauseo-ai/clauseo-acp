//! Claude Code's task frames (`_claude/sdkMessage`, `map/task.rs`), shaped as live runs of
//! claude-agent-acp 0.110.1 sent them, their text put short: a settle becomes
//! `AcpEvent::HelperSettled` naming the launching call; anything else gives nothing.

use clauseo_protocol::{CompactString, HelperOutcome};

use super::*;

/// The launching call (`tool_use_id`) and the task id of the helper in these tests.
const LAUNCH: &str = "toolu_01Launch";
const TASK: &str = "a1b2c3d4e5f60718";

/// Plays the agent sending each of `messages` as a `_claude/sdkMessage` of session `s-1` after
/// the handshake; returns every event the host got.
fn events_for(messages: &[Value]) -> Vec<AcpEvent> {
    let (client, events, driver, mut agent) = pipes();
    let calls = async move { handshake(&client).await };
    let script = async {
        agent.start().await;
        for message in messages {
            agent.send(&json!({"jsonrpc": "2.0", "method": "_claude/sdkMessage",
                "params": {"sessionId": "s-1", "message": message}}));
        }
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    drain(&events)
}

/// A `task_started` of `task` for the call `launch`, of `kind` (`local_agent`, `local_bash`).
fn started(task: &str, launch: &str, kind: &str) -> Value {
    json!({"type": "system", "subtype": "task_started", "task_id": task, "run_id": "r-1",
        "tool_use_id": launch, "description": "Read lease.txt", "subagent_type": "paper-reader",
        "is_backgrounded": true, "spawn_depth": 1, "task_type": kind,
        "prompt": "Read lease.txt in /matters/lease and return a digest.", "uuid": "u-1",
        "session_id": "s-1"})
}

/// A `task_updated` of `task` with `status`.
fn updated(task: &str, status: &str) -> Value {
    json!({"type": "system", "subtype": "task_updated", "task_id": task, "run_id": "r-1",
        "patch": {"status": status, "end_time": 1}, "uuid": "u-2", "session_id": "s-1"})
}

/// A `task_notification` of `task`, launched by `launch`, with `status`, and the hand-back's
/// report, as a helper that hands back has it.
fn notified(task: &str, launch: &str, status: &str) -> Value {
    json!({"type": "system", "subtype": "task_notification", "task_id": task, "run_id": "r-1",
        "tool_use_id": launch, "status": status,
        "output_file": "/tmp/claude/tasks/a1b2c3d4e5f60718.output",
        "summary": "This agent's report was delivered to you as a message.",
        "usage": {"total_tokens": 6812, "tool_uses": 1, "duration_ms": 8963},
        "handback": "send", "handback_report": {"text": "Digest of lease.txt."},
        "uuid": "u-3", "session_id": "s-1"})
}

/// Claude Code's `init` frame, which the same filter lets through.
fn init() -> Value {
    json!({"type": "system", "subtype": "init", "cwd": "/matters/lease", "session_id": "s-1",
        "tools": ["Read"], "model": "claude-sonnet", "uuid": "u-0"})
}

/// The helper and outcome of each event, which must all be `HelperSettled` of session `s-1`
/// with no turn running.
fn settled(events: Vec<AcpEvent>) -> Vec<(CompactString, HelperOutcome)> {
    events
        .into_iter()
        .map(|event| {
            let AcpEvent::HelperSettled {
                session,
                origin,
                helper,
                outcome,
            } = event
            else {
                panic!("expected a settle, got {event:?}");
            };
            assert_eq!((session, origin), (session_s1(), Origin::Idle));
            (helper, outcome)
        })
        .collect()
}

fn session_s1() -> AgentSessionId {
    AgentSessionId::new("s-1").unwrap()
}

fn pair(helper: &str, outcome: HelperOutcome) -> (CompactString, HelperOutcome) {
    (helper.into(), outcome)
}

/// A helper that hands back (live run L6d): its start, then the terminal `task_updated` and the
/// `task_notification` each give a settle naming the launch, `Done`; `init` gives nothing, and
/// nothing of the frames' text comes along.
#[test]
fn a_helper_s_settle_frames_each_name_its_launch() {
    let events = events_for(&[
        init(),
        started(TASK, LAUNCH, "local_agent"),
        updated(TASK, "completed"),
        notified(TASK, LAUNCH, "completed"),
        init(),
    ]);
    assert_eq!(
        settled(events),
        [
            pair(LAUNCH, HelperOutcome::Done),
            pair(LAUNCH, HelperOutcome::Done),
        ]
    );
}

/// A terminal `task_updated` with no `task_notification` after it (the adapter may skip one)
/// names the launch through the task's start: `killed` is `Stopped`, `failed` `Failed`. The
/// task is forgotten at its settle, so a later `task_updated` of it gives nothing.
#[test]
fn a_terminal_task_updated_alone_names_the_launch() {
    let events = events_for(&[
        started(TASK, LAUNCH, "local_agent"),
        started("t-2", "toolu_02Launch", "local_agent"),
        updated(TASK, "killed"),
        updated("t-2", "failed"),
        updated(TASK, "completed"),
    ]);
    assert_eq!(
        settled(events),
        [
            pair(LAUNCH, HelperOutcome::Stopped),
            pair("toolu_02Launch", HelperOutcome::Failed),
        ]
    );
}

/// A `task_updated` that says the task waits or runs (again), or whose status it does not
/// know, is no settle; one of a task it never saw start names no launch. Both give nothing.
#[test]
fn a_task_updated_that_is_no_settle_or_of_no_known_task_gives_nothing() {
    let events = events_for(&[
        started(TASK, LAUNCH, "local_agent"),
        updated(TASK, "pending"),
        updated(TASK, "running"),
        updated(TASK, "paused"),
        updated(TASK, "waiting_for_input"),
        updated("never-started", "completed"),
    ]);
    assert_eq!(settled(events), []);
}

/// Every `task_notification` is a settle: `stopped` is `Stopped`, `failed` `Failed`, and a
/// status it does not know `Failed`. It names the launch itself, so it needs no start.
#[test]
fn a_task_notification_settles_with_its_status() {
    let events = events_for(&[
        notified("t-1", "toolu_01", "stopped"),
        notified("t-2", "toolu_02", "failed"),
        notified("t-3", "toolu_03", "lost_in_space"),
    ]);
    assert_eq!(
        settled(events),
        [
            pair("toolu_01", HelperOutcome::Stopped),
            pair("toolu_02", HelperOutcome::Failed),
            pair("toolu_03", HelperOutcome::Failed),
        ]
    );
}

/// A shell command a helper runs is a task too (live run L7d: `local_bash`, its call the
/// `Bash` step): its settle names that step, and the host, which knows no helper by it, drops
/// it.
#[test]
fn a_shell_command_s_task_settles_naming_its_step() {
    let events = events_for(&[
        started("bkkl7ghaa", "toolu_03Bash", "local_bash"),
        notified("bkkl7ghaa", "toolu_03Bash", "stopped"),
    ]);
    assert_eq!(
        settled(events),
        [pair("toolu_03Bash", HelperOutcome::Stopped)]
    );
}

/// A frame missing what it needs, a frame of another type or subtype, a message that is no
/// object, or a session id that is empty or missing gives nothing, not even a warning: the
/// frames' text never reaches the host.
#[test]
fn a_frame_it_cannot_read_gives_nothing() {
    let (client, events, driver, mut agent) = pipes();
    let calls = async move { handshake(&client).await };
    let script = async {
        agent.start().await;
        let send = |params: Value| {
            agent.send(&json!({"jsonrpc": "2.0", "method": "_claude/sdkMessage",
                "params": params}));
        };
        let no_launch = json!({"type": "system", "subtype": "task_notification",
            "task_id": TASK, "status": "completed", "summary": "The lease runs to 2031."});
        let no_status = json!({"type": "system", "subtype": "task_notification",
            "task_id": TASK, "tool_use_id": LAUNCH});
        let assistant = json!({"type": "assistant", "subtype": "task_notification",
            "tool_use_id": LAUNCH, "status": "completed"});
        let other = json!({"type": "system", "subtype": "compact_boundary"});
        for message in [
            no_launch,
            no_status,
            assistant,
            other,
            json!("text"),
            json!(null),
        ] {
            send(json!({"sessionId": "s-1", "message": message}));
        }
        send(json!({"sessionId": "", "message": notified(TASK, LAUNCH, "completed")}));
        send(json!({"message": notified(TASK, LAUNCH, "completed")}));
        send(json!({"sessionId": "s-1"}));
        send(json!(["not", "an", "object"]));
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    assert_eq!(drain(&events), []);
}

/// A settle while a prompt runs comes with that turn's origin, as the session's updates do.
#[test]
fn a_settle_during_a_prompt_has_the_turn_s_origin() {
    let (client, events, driver, mut agent) = pipes();
    let calls = async move {
        handshake(&client).await;
        let _answer = client.prompt(&session(), turn(1), Vec::new()).await;
    };
    let script = async {
        agent.start().await;
        let prompt = agent.read().await;
        assert_eq!(prompt["method"], "session/prompt");
        agent.send(&json!({"jsonrpc": "2.0", "method": "_claude/sdkMessage",
            "params": {"sessionId": "s-1", "message": notified(TASK, LAUNCH, "completed")}}));
        agent.respond(&prompt, &json!({"stopReason": "end_turn"}));
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    let events = drain(&events);
    assert_eq!(
        events,
        [AcpEvent::HelperSettled {
            session: session_s1(),
            origin: Origin::Turn(turn(1)),
            helper: LAUNCH.into(),
            outcome: HelperOutcome::Done,
        }]
    );
}
