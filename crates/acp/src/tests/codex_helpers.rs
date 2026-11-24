//! codex-acp 2.10.0's helper sessions (`ConnectOptions::helper_sessions`), played from recorded
//! frames. Each fixture `fixtures/codex-acp-2.10.0-helpers-*.jsonl` holds what the agent sent in
//! one recorded run, cut to the frames these tests need, the matter folder renamed
//! `/Users/lawyer/Matters/Lease`, ids kept: x1 one helper, x2 two at once, x3 a cancel, x4 a
//! helper's helper, x8 a helper's permission request, f1d the replay of `session/load` after a
//! restart.

use clauseo_protocol::{StopReason, ToolLocation};

use super::*;

mod edges;
mod sessions;
mod stopped;

const X1: &str = include_str!("fixtures/codex-acp-2.10.0-helpers-x1.jsonl");
const X2: &str = include_str!("fixtures/codex-acp-2.10.0-helpers-x2.jsonl");
const X3: &str = include_str!("fixtures/codex-acp-2.10.0-helpers-x3.jsonl");
const X4: &str = include_str!("fixtures/codex-acp-2.10.0-helpers-x4.jsonl");
const X8: &str = include_str!("fixtures/codex-acp-2.10.0-helpers-x8.jsonl");
const F1D: &str = include_str!("fixtures/codex-acp-2.10.0-helpers-f1d.jsonl");

/// `path`, written `/a/b`, as an absolute path on this OS: `C:\a\b` on Windows, where a path
/// without a drive is not absolute.
fn abs(path: &str) -> String {
    if cfg!(windows) {
        format!("C:{}", path.replace('/', "\\"))
    } else {
        path.to_owned()
    }
}

/// The session's `cwd`: the fixtures' matter folder on this OS, so the helper's reads fall
/// inside it.
fn folder() -> PathBuf {
    PathBuf::from(abs("/Users/lawyer/Matters/Lease"))
}

/// The frames of a fixture, its matter folder written as [`folder`].
fn frames(text: &str) -> Vec<Value> {
    // unix-paths-ok: the fixtures' folder, replaced by this OS's form of it
    let recorded = "/Users/lawyer/Matters/Lease";
    let here = serde_json::to_string(&folder().display().to_string()).unwrap();
    let here = here.trim_matches('"');
    text.replace(recorded, here)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn options(on: bool) -> ConnectOptions {
    ConnectOptions {
        helper_sessions: on,
        ..ConnectOptions::default()
    }
}

fn id(text: &str) -> AgentSessionId {
    AgentSessionId::new(text).unwrap()
}

/// `initialize`, then `session/new` in [`folder`].
async fn start(client: &AcpClient) {
    client.initialize().await.unwrap();
    let start = client.new_session(NewSession {
        cwd: folder(),
        additional_directories: Vec::new(),
        mcp_servers: Vec::new(),
        meta: None,
    });
    start.await.unwrap();
}

/// The agent's side of [`start`]: `session/new` answered with `root`.
async fn answer_start(agent: &mut Agent, root: &str) {
    agent.initialize().await;
    let new = agent.read().await;
    assert_eq!(new["method"], "session/new");
    agent.respond(&new, &json!({"sessionId": root}));
}

/// Plays `frames` while the prompt of turn 1 on `root` runs, with helper sessions `on` or off,
/// then answers it `end_turn`; returns every event.
fn play(on: bool, root: &str, frames: &[Value]) -> Vec<AcpEvent> {
    let (client, events, driver, mut agent) = pipes_with(options(on));
    let session = id(root);
    let calls = async move {
        start(&client).await;
        let end = client.prompt(&session, turn(1), Vec::new()).await.unwrap();
        assert_eq!(end.stop, StopReason::EndTurn);
    };
    let script = async {
        answer_start(&mut agent, root).await;
        let prompt = agent.read().await;
        for frame in frames {
            agent.send(frame);
        }
        agent.respond(&prompt, &json!({"stopReason": "end_turn"}));
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    drain(&events)
}

/// The second group of a session id, enough to tell the recorded sessions apart.
fn short(id: &str) -> &str {
    id.get(9..13).unwrap_or(id)
}

fn kind(update: &AgentUpdate) -> String {
    match update {
        AgentUpdate::MessageChunk { role, content, .. } => format!("{role:?} {}", said(content)),
        AgentUpdate::ToolCall(call) => format!("tool call {}", call.acp_id),
        AgentUpdate::ToolCallUpdate { acp_id, .. } => format!("tool update {acp_id}"),
        AgentUpdate::Plan(entries) => format!("plan of {}", entries.len()),
        AgentUpdate::Commands(_) => "commands".to_owned(),
        AgentUpdate::CurrentMode { mode_id } => format!("mode {mode_id}"),
        AgentUpdate::ConfigOptions(options) => format!("options of {}", options.len()),
        AgentUpdate::SessionTitle { .. } => "title".to_owned(),
        AgentUpdate::Usage { used, .. } => format!("usage {}", used.get()),
        AgentUpdate::Notice { .. } => "notice".to_owned(),
        AgentUpdate::CompactionStarted { .. }
        | AgentUpdate::CompactionEnded { .. }
        | AgentUpdate::CompactionSummaryChunk { .. } => "compaction".to_owned(),
    }
}

fn said(content: &ContentBlock) -> String {
    match content {
        ContentBlock::Text { text } => format!("{text:?}"),
        other @ (ContentBlock::Image { .. }
        | ContentBlock::Audio { .. }
        | ContentBlock::ResourceLink { .. }
        | ContentBlock::Resource { .. }
        | ContentBlock::Unsupported { .. }) => format!("{other:?}"),
    }
}

/// What an event is, in a line: its session (short), what it says, and the helper (short).
fn shape(event: &AcpEvent) -> String {
    match event {
        AcpEvent::Update {
            session, update, ..
        } => format!("{} {}", short(session.as_str()), kind(update)),
        AcpEvent::HelperUpdate {
            session,
            helper,
            update,
            ..
        } => format!(
            "{} helper {}: {}",
            short(session.as_str()),
            short(helper),
            kind(update)
        ),
        AcpEvent::HelperSpawned {
            session,
            helper,
            parent,
            name,
            task,
            ..
        } => format!(
            "{} spawned {} by {:?}: {name:?} {task:?}",
            short(session.as_str()),
            short(helper),
            parent.as_deref().map(short)
        ),
        AcpEvent::HelperAnswer {
            session,
            helper,
            content,
            ..
        } => format!(
            "{} answer {}: {}",
            short(session.as_str()),
            short(helper),
            said(content)
        ),
        AcpEvent::HelperEnded {
            session,
            helper,
            state,
            ..
        } => format!(
            "{} ended {}: {state:?}",
            short(session.as_str()),
            short(helper)
        ),
        AcpEvent::Warning { session, warning } => format!(
            "{:?} warning {warning:?}",
            session.as_ref().map(|session| short(session.as_str()))
        ),
        other @ (AcpEvent::HelperInBackground { .. }
        | AcpEvent::HelperSettled { .. }
        | AcpEvent::PermissionRequested { .. }
        | AcpEvent::ElicitationRequested { .. }
        | AcpEvent::ElicitationCompleted { .. }
        | AcpEvent::Limit { .. }
        | AcpEvent::TooLong { .. }
        | AcpEvent::ConnectionLost { .. }
        | AcpEvent::UpdateSkipped { .. }) => format!("{other:?}"),
    }
}

fn shapes(events: &[AcpEvent]) -> Vec<String> {
    events.iter().map(shape).collect()
}

/// The origin of an event that has one.
fn origin(event: &AcpEvent) -> Option<Origin> {
    match event {
        AcpEvent::Update { origin, .. }
        | AcpEvent::HelperUpdate { origin, .. }
        | AcpEvent::HelperInBackground { origin, .. }
        | AcpEvent::HelperSpawned { origin, .. }
        | AcpEvent::HelperAnswer { origin, .. }
        | AcpEvent::HelperEnded { origin, .. }
        | AcpEvent::HelperSettled { origin, .. }
        | AcpEvent::PermissionRequested { origin, .. }
        | AcpEvent::ElicitationRequested { origin, .. }
        | AcpEvent::Limit { origin, .. }
        | AcpEvent::TooLong { origin, .. }
        | AcpEvent::ConnectionLost { origin, .. }
        | AcpEvent::UpdateSkipped { origin, .. } => Some(*origin),
        AcpEvent::ElicitationCompleted { .. } | AcpEvent::Warning { .. } => None,
    }
}

const X1_ROOT: &str = "01a20c29-bdfa-7f90-811a-9ceea2c4da8e";

/// One helper (x1): announced on the root with its name and no task (the filler), its options,
/// usage, tool calls and commentary as `HelperUpdate`s and its `final_answer` chunks as
/// `HelperAnswer`s, all on the root session in the root's turn; its status-only
/// `session_info_update`s give nothing, as on the root; its end; the lead's own `final_answer`
/// chunk stays an `Update`.
#[test]
fn one_helper_comes_on_the_root_session() {
    let events = play(true, X1_ROOT, &frames(X1));
    let h = "bdfa helper d483:";
    assert_eq!(
        shapes(&events),
        [
            "bdfa Answer \".\"".to_owned(),
            "bdfa spawned d483 by None: \"Read documents\" None".to_owned(),
            format!("{h} options of 1"),
            "bdfa usage 12071".to_owned(),
            "bdfa tool call call_3a599374996943828a280bc01e0e8cb4".to_owned(),
            format!("{h} Answer \"I\""),
            format!("{h} Answer \"’ll\""),
            format!("{h} Answer \".\""),
            format!("{h} tool call exec-7fd37d45-aacb-4501-8bc2-96ebd303db13"),
            format!("{h} tool update exec-7fd37d45-aacb-4501-8bc2-96ebd303db13"),
            format!("{h} tool call exec-ceaf303f-ba35-46ac-9089-b0127f63760b"),
            format!("{h} tool update exec-ceaf303f-ba35-46ac-9089-b0127f63760b"),
            format!("{h} usage 12869"),
            "bdfa answer d483: \"Lease\"".to_owned(),
            "bdfa answer d483: \" start\"".to_owned(),
            "bdfa answer d483: \"5\"".to_owned(),
            format!("{h} usage 14406"),
            "bdfa ended d483: Completed".to_owned(),
            "bdfa tool update call_3a599374996943828a280bc01e0e8cb4".to_owned(),
            "bdfa Answer \"Lease\"".to_owned(),
        ]
    );
    for event in &events {
        assert_eq!(origin(event), Some(Origin::Turn(turn(1))), "{event:?}");
    }
    // The helper's read is in the root's folder.
    let Some(AcpEvent::HelperUpdate {
        update: AgentUpdate::ToolCall(call),
        ..
    }) = events.get(8)
    else {
        panic!("expected the helper's first read, got {events:?}");
    };
    assert!(
        matches!(
            call.locations.as_slice(),
            [ToolLocation::Inside { path, .. }] if path.as_str() == "lease.txt"
        ),
        "{call:?}"
    );
}

/// Two helpers at once (x2): each named by its own session id while their frames interleave,
/// each ended on its own.
#[test]
fn two_helpers_at_once_keep_their_own_names() {
    let events = play(true, "01a20c2b-4ddc-7e52-953c-20f893c114ad", &frames(X2));
    let (lease, order) = ("4ddc helper 62d3:", "4ddc helper 6e7c:");
    assert_eq!(
        shapes(&events),
        [
            "4ddc spawned 62d3 by None: \"Lease\" None".to_owned(),
            format!("{lease} options of 1"),
            "4ddc usage 12051".to_owned(),
            "4ddc spawned 6e7c by None: \"Order\" None".to_owned(),
            format!("{order} options of 1"),
            "4ddc usage 12110".to_owned(),
            "4ddc tool call call_ed817d36da284826ab37ca0f7495ce4c".to_owned(),
            format!("{lease} tool call exec-42efd73f-ab01-4701-9cc7-1bc4826c1b42"),
            format!("{lease} tool update exec-42efd73f-ab01-4701-9cc7-1bc4826c1b42"),
            format!("{lease} usage 12782"),
            format!("{order} tool call exec-999d7ab1-deda-4bd3-bf2c-d9ba551f7880"),
            format!("{order} tool update exec-999d7ab1-deda-4bd3-bf2c-d9ba551f7880"),
            format!("{order} usage 12835"),
            "4ddc tool update call_ed817d36da284826ab37ca0f7495ce4c".to_owned(),
            format!("{lease} usage 13060"),
            "4ddc usage 12151".to_owned(),
            "4ddc answer 6e7c: \"The\"".to_owned(),
            "4ddc answer 62d3: \"Lease\"".to_owned(),
            format!("{lease} usage 13086"),
            "4ddc ended 62d3: Completed".to_owned(),
            format!("{order} usage 13615"),
            "4ddc ended 6e7c: Completed".to_owned(),
        ]
    );
}

/// A helper's helper (x4): announced on its parent helper's session, it comes on the root with
/// `parent` set; its frames and its end, which also comes on the parent's session, are the
/// root's too.
#[test]
fn a_helpers_helper_names_its_parent() {
    let events = play(true, "01a20c2d-39af-7200-8bc8-23a213391a6b", &frames(X4));
    let (relay, reader) = ("39af helper 4f42:", "39af helper 621a:");
    assert_eq!(
        shapes(&events),
        [
            "39af spawned 4f42 by None: \"Relay\" None".to_owned(),
            format!("{relay} options of 1"),
            "39af usage 12076".to_owned(),
            "39af tool call call_a4657ac919e64a34bed52a6294677073".to_owned(),
            "39af spawned 621a by Some(\"4f42\"): \"Reader\" None".to_owned(),
            format!("{reader} options of 1"),
            format!("{relay} usage 12836"),
            format!("{relay} tool call call_a682f479ea4e4a379ecaa40696c03dc8"),
            format!("{reader} tool call exec-5372b597-987f-4b90-a4fc-a3fac98c93c0"),
            format!("{reader} tool update exec-5372b597-987f-4b90-a4fc-a3fac98c93c0"),
            format!("{reader} usage 13481"),
            "39af answer 621a: \"14\"".to_owned(),
            format!("{reader} usage 14177"),
            "39af ended 621a: Completed".to_owned(),
            format!("{relay} tool update call_a682f479ea4e4a379ecaa40696c03dc8"),
            format!("{relay} usage 12880"),
            "39af tool update call_a4657ac919e64a34bed52a6294677073".to_owned(),
            "39af usage 12118".to_owned(),
            "39af answer 4f42: \"14\"".to_owned(),
            format!("{relay} usage 12962"),
            "39af ended 4f42: Completed".to_owned(),
        ]
    );
}

/// A cancel (x3) while a helper starts: its frames before the cancel are the turn's, after it
/// late, as the root's are; its end says `Cancelled`.
#[test]
fn a_cancel_makes_the_helpers_frames_late_and_ends_it_cancelled() {
    let root = "01a20c2c-6eba-7720-8c11-b5dd1726e69d";
    let all = frames(X3);
    let (before, after) = all.split_at(2);
    let (client, events, driver, mut agent) = pipes_with(options(true));
    let session = id(root);
    let seen = &events;
    let calls = async move {
        start(&client).await;
        let prompt = client.prompt(&session, turn(1), Vec::new());
        let cancel = async {
            let mut first = Vec::new();
            while first.len() < 2 {
                first.push(seen.recv().await.unwrap());
            }
            assert_eq!(client.cancel(&session).await.unwrap(), []);
            first
        };
        let (end, first) = futures::join!(prompt, cancel);
        assert_eq!(end.unwrap().stop, StopReason::Cancelled);
        first
    };
    let script = async {
        answer_start(&mut agent, root).await;
        let prompt = agent.read().await;
        for frame in before {
            agent.send(frame);
        }
        assert_eq!(agent.read().await["method"], "session/cancel");
        for frame in after {
            agent.send(frame);
        }
        agent.respond(&prompt, &json!({"stopReason": "cancelled"}));
        agent.read_end().await;
    };
    let (driver, mut got, ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    got.extend(drain(seen));
    assert_eq!(
        shapes(&got),
        [
            "6eba spawned 870f by None: \"Summaries\" None",
            "6eba helper 870f: options of 1",
            "6eba usage 12102",
            "6eba ended 870f: Cancelled",
        ]
    );
    let origins: Vec<_> = got.iter().map(origin).collect();
    let (live, late) = (Some(Origin::Turn(turn(1))), Some(Origin::Late(turn(1))));
    assert_eq!(origins, [live, live, late, late]);
}

const F1D_ROOT: &str = "01a20c40-e27b-7d81-b373-6c80b2fd0488";

/// The replay of `session/load` after a restart (f1d): the helper is announced again, its
/// options come, and it ends `Disconnected`, all as `Replay`, as the root's history is.
#[test]
fn a_reload_replays_the_helper_and_ends_it_disconnected() {
    let (client, events, driver, mut agent) = pipes_with(options(true));
    let calls = async move {
        client.initialize().await.unwrap();
        let load = client.load_session(LoadSession {
            session: id(F1D_ROOT),
            cwd: folder(),
            additional_directories: Vec::new(),
            mcp_servers: Vec::new(),
            meta: None,
        });
        load.await.unwrap();
    };
    let script = async {
        agent.initialize().await;
        let load = agent.read().await;
        assert_eq!(load["method"], "session/load");
        for frame in frames(F1D) {
            agent.send(&frame);
        }
        agent.respond(&load, &json!({}));
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    let events = drain(&events);
    let shapes = shapes(&events);
    assert_eq!(
        shapes.get(4..),
        Some(
            &[
                "e27b spawned f763 by None: \"Read text\" None".to_owned(),
                "e27b helper f763: options of 1".to_owned(),
                "e27b ended f763: Disconnected".to_owned(),
            ][..]
        ),
        "{shapes:?}"
    );
    for event in &events {
        assert_eq!(origin(event), Some(Origin::Replay), "{event:?}");
    }
}

const X8_ROOT: &str = "01a20c2f-995c-7dc2-bf56-fa1b752e09e1";

/// Plays x8 up to the helper's permission request; `decide` answers it from the client, given
/// the request's event, and the agent then reads `expect` (the client's lines) in order.
fn x8_permission(decide: fn(&AcpEvent) -> Answer, expect: &[&str]) -> (AcpEvent, Vec<AcpEvent>) {
    let all = frames(X8);
    let (before, after) = all.split_at(8);
    let (client, events, driver, mut agent) = pipes_with(options(true));
    let seen = &events;
    let calls = async move {
        start(&client).await;
        let root = id(X8_ROOT);
        let prompt = client.prompt(&root, turn(1), Vec::new());
        let answer = async {
            loop {
                let event = seen.recv().await.unwrap();
                if matches!(event, AcpEvent::PermissionRequested { .. }) {
                    match decide(&event) {
                        Answer::Option(key, option) => {
                            let option = clauseo_protocol::AgentChoiceId::new(option).unwrap();
                            client.answer_permission(key, option).await.unwrap();
                        }
                        Answer::CancelRoot => {
                            let keys = client.cancel(&id(X8_ROOT)).await.unwrap();
                            assert_eq!(keys.len(), 1);
                        }
                    }
                    return event;
                }
            }
        };
        let (end, request) = futures::join!(prompt, answer);
        end.unwrap();
        request
    };
    let script = async {
        answer_start(&mut agent, X8_ROOT).await;
        let prompt = agent.read().await;
        for frame in before {
            agent.send(frame);
        }
        for line in expect {
            let read = agent.read().await;
            let outcome = &read["result"]["outcome"];
            let what = read["method"].as_str().map_or_else(
                || {
                    format!(
                        "answer {} {} {}",
                        read["id"], outcome["outcome"], outcome["optionId"]
                    )
                },
                str::to_owned,
            );
            assert_eq!(&what, line);
        }
        for frame in after {
            agent.send(frame);
        }
        agent.respond(&prompt, &json!({"stopReason": "end_turn"}));
        agent.read_end().await;
    };
    let (driver, request, ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    (request, drain(seen))
}

/// How [`x8_permission`]'s client answers.
enum Answer {
    /// `answer_permission` with this key and option.
    Option(PendingKey, &'static str),
    /// `cancel` of the root session.
    CancelRoot,
}

fn allow_once(event: &AcpEvent) -> Answer {
    let AcpEvent::PermissionRequested { key, .. } = event else {
        panic!("expected a permission request, got {event:?}");
    };
    Answer::Option(*key, "allow_once")
}

/// A helper's permission request (x8, under the helper's session id) comes as the root's, in
/// the root's turn, and its answer goes back to the agent by the request's id.
#[test]
fn a_helpers_permission_request_is_the_roots() {
    let expect = ["answer 0 \"selected\" \"allow_once\""];
    let (request, rest) = x8_permission(allow_once, &expect);
    let AcpEvent::PermissionRequested {
        session,
        origin,
        acp_tool_call_id,
        ..
    } = request
    else {
        panic!("expected a permission request, got {request:?}");
    };
    assert_eq!(session, id(X8_ROOT));
    assert_eq!(origin, Origin::Turn(turn(1)));
    assert_eq!(
        acp_tool_call_id,
        "exec-0960825a-462c-432d-80e5-c3c12ab16668"
    );
    assert_eq!(
        shapes(&rest).last().map(String::as_str),
        Some("995c ended ab8e: Completed")
    );
}

/// The root's cancel answers a helper's pending permission request `cancelled` too.
#[test]
fn the_roots_cancel_answers_a_helpers_permission_request() {
    let expect = ["session/cancel", "answer 0 \"cancelled\" null"];
    let (request, _) = x8_permission(|_| Answer::CancelRoot, &expect);
    assert!(matches!(request, AcpEvent::PermissionRequested { .. }));
}
