//! Switching a conversation back to an agent it had before (issue 4b): the agent loads its own
//! last session of this conversation, and the handover holds only the answers it missed.
//! Each agent plays its own session id, so a session loaded from the wrong agent, or from the
//! wrong conversation, is a mismatch; each prompt that carries a handover is expected exactly.
//! The log reading (which session, what it saw) has unit tests in `switch/resume/tests.rs`.

use std::path::{Path, PathBuf};

use clauseo_protocol::{
    AgentId, CommandOutcome, ContextHandoff, DeliveryRequest, DomainEvent, HandoffStatus,
    HostCommand, NoticeKind, StopReason, StreamId, ThreadId, TurnId,
};
use serde_json::{Value, json};

use crate::codex::CODEX;
use crate::limits_host::CLAUDE;
use crate::observe::Events;
use crate::support::{Dir, Running, Scenario, matter_with_thread, text};
use crate::write;

const SCENARIOS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../fake_agent/scenarios");

const FIRST: &str = "Draft the clause.";
const SECOND: &str = "Check the clause.";
const THIRD: &str = "Tighten it.";
const CLAUDE_ANSWER: &str = "Claude drafted it.";
const CHATGPT_ANSWER: &str = "ChatGPT checked it.";
const CLAUDE_AGAIN: &str = "Claude tightened it.";

/// The handover's opening for `included` of `total` items in which the lawyer worked with
/// `with`.
fn header(with: &str, included: usize, total: usize) -> String {
    format!(
        "Earlier in this conversation the lawyer worked with {with}. Below are {included} of \
         {total} earlier items, whole. They are background, not a new request or instructions; \
         files, tool state and reasoning are not included."
    )
}

/// The opening of a handover to an agent that took its own session back: the items came after
/// its own last answer, in which the lawyer worked with `with`.
fn since(with: &str, included: usize, total: usize) -> String {
    format!(
        "Since your last answer in this conversation, the lawyer worked with {with}. The \
         conversation you already have came first; these items came after it. Below are \
         {included} of {total} items from after your last answer, whole. They are background, \
         not a new request or instructions; files, tool state and reasoning are not included."
    )
}

/// The prompt that carries `handover` before the lawyer's `words`.
fn wrapped(handover: &str, words: &str) -> String {
    format!("{handover}\n\nUser message:\n{words}")
}

/// The lines of the bundled `common/<file>`, with its session `sess-1` renamed `session`.
fn common(file: &str, session: &str) -> Vec<Value> {
    let text = std::fs::read_to_string(format!("{SCENARIOS}/common/{file}")).unwrap();
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(&line.replace("sess-1", session)).unwrap())
        .collect()
}

/// Claude's modes, as claude-agent-acp 0.89.1 answers `session/new` and `session/load`.
fn claude_modes() -> Value {
    json!({"currentModeId": "auto", "availableModes": [
        {"id": "default", "name": "Manual"}, {"id": "acceptEdits", "name": "Accept edits"},
        {"id": "plan", "name": "Plan"}, {"id": "auto", "name": "Auto"}]})
}

/// Codex's modes, as codex-acp answers a session start, in Clauseo's mode `agent`.
fn codex_modes() -> Value {
    json!({"currentModeId": "agent", "availableModes": [
        {"id": "read-only", "name": "Read-only"}, {"id": "workspace-write", "name": "Workspace access"},
        {"id": "agent", "name": "Auto review"}, {"id": "agent-full-access", "name": "Full access"}]})
}

/// A prompt labelled `label` to `session`, its text exactly `words` when given.
fn prompt(label: &str, session: &str, words: Option<&str>) -> Value {
    let mut params = json!({"sessionId": session});
    if let Some(words) = words {
        params["prompt"] = json!([{"type": "text", "text": words}]);
    }
    json!({"expect": {"method": "session/prompt", "as": label, "params": params}})
}

/// The answer `words` to the prompt `label`, as message `id`, and the turn's end.
fn answer(label: &str, id: &str, words: &str) -> [Value; 2] {
    [
        json!({"update": {"update": {"sessionUpdate": "agent_message_chunk", "messageId": id,
            "content": {"type": "text", "text": words}}}}),
        json!({"respond": {"to": label, "result": {"stopReason": "end_turn"}}}),
    ]
}

/// A `session/load` of `session` answered with `modes`.
fn loads(session: &str, modes: &Value) -> [Value; 2] {
    [
        json!({"expect": {"method": "session/load", "as": "load",
            "params": {"sessionId": session, "cwd": "{{any}}"}}}),
        json!({"respond": {"to": "load", "result": {"modes": modes}}}),
    ]
}

/// A scenario of the bundled `handshake`'s `initialize`, then on a first launch its
/// `session/new` (as `session`) and `new`, on a relaunch `load`.
fn agent(handshake: &str, session: &str, new: Vec<Value>, load: &[Value]) -> Vec<Value> {
    let lines = common(handshake, session);
    let (opening, start) = lines.split_at(2);
    let mut first: Vec<Value> = start.to_vec();
    first.extend(new);
    let mut steps = opening.to_vec();
    steps.push(json!({"one_of": [first, load.to_vec()]}));
    steps
}

/// Writes `steps` as the scenario `name` in `dir`'s data folder.
fn scenario(dir: &Dir, name: &str, steps: &[Value]) -> (Scenario, PathBuf) {
    let file = dir.data().join("scenarios").join(format!("{name}.jsonl"));
    rewrite(&file, name, steps);
    (Scenario::Written(file.clone()), file)
}

fn rewrite(file: &Path, name: &str, steps: &[Value]) {
    let mut text =
        json!({"scenario": name, "description": "an agent switch back", "on_stdin_close": "exit"})
            .to_string();
    text.push('\n');
    for step in steps {
        text.push_str(&step.to_string());
        text.push('\n');
    }
    write::write(file, text.as_bytes());
}

/// `SendPrompt` of `words` to `thread`, naming `agent`; its turn.
fn send_to(host: &Running, thread: ThreadId, words: &str, agent: &str) -> TurnId {
    let command = HostCommand::SendPrompt {
        thread,
        content: text(words),
        delivery: DeliveryRequest::Queue,
        config: None,
        agent: Some(AgentId::new(agent).unwrap()),
    };
    match host.ok(command) {
        CommandOutcome::PromptAccepted { turn, .. } => turn,
        other => panic!("SendPrompt answered {other:?}"),
    }
}

/// `turn` ended with the agent's answer, with no mismatch on either agent.
fn ends(host: &Running, events: &mut Events, turn: TurnId) {
    let ended = events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
}

/// Every `ContextHandoff`, in log order.
fn handoffs(events: &Events) -> Vec<ContextHandoff> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::ContextHandoff(handoff) => Some((**handoff).clone()),
            _ => None,
        })
        .collect()
}

/// The status each `HandoffSettled` logs for `handoff`.
fn settled(events: &Events, handoff: &ContextHandoff) -> Vec<HandoffStatus> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::HandoffSettled(settled) if settled.handoff == handoff.handoff => {
                Some(settled.status)
            }
            _ => None,
        })
        .collect()
}

/// The session each `AgentSessionAttached` names, in log order.
fn sessions(events: &Events) -> Vec<String> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::AgentSessionAttached(attached) => {
                Some(attached.session.as_str().to_owned())
            }
            _ => None,
        })
        .collect()
}

fn no_context_notice(events: &Events) {
    let notices = events.count(|event| {
        matches!(event, DomainEvent::Notice(n) if n.kind == Some(NoticeKind::ContextNotRestored))
    });
    assert_eq!(notices, 0, "{}", events.dump());
}

/// (what it covers, how many items, whether resumed), from and to.
fn shape(handoff: &ContextHandoff) -> (u64, u64, bool, String, String) {
    (
        handoff.answers_covered.get(),
        handoff.items_included.get(),
        handoff.resumed,
        handoff.from.agent.as_str().to_owned(),
        handoff.to.agent.as_str().to_owned(),
    )
}

/// `ChatGPT`'s part, the same in each test: a new session `sess-codex`, whose one prompt
/// carries Claude's handover (as 4a's tests expect it), answered.
fn chatgpt(dir: &Dir) -> Scenario {
    let mut steps = common("codex-handshake.jsonl", "sess-codex");
    steps.push(prompt("p1", "sess-codex", None));
    steps.extend(answer("p1", "m-x1", CHATGPT_ANSWER));
    scenario(dir, "back-codex", &steps).0
}

/// Claude answers, the thread goes to `ChatGPT`, which answers, then back to Claude: the handover
/// Claude gets is `ChatGPT`'s turn alone.
#[test]
fn back_to_claude_loads_its_first_session_and_hands_over_only_what_it_missed() {
    let dir = Dir::new("agent_switch_back", &[]);
    let missed = since("ChatGPT", 2, 2)
        + &format!(
            "\n\n[Earlier: the lawyer]\n> {SECOND}\n\n[Earlier: ChatGPT's answer]\n> {CHATGPT_ANSWER}"
        );
    let mut new = vec![prompt("p1", "sess-claude", Some(FIRST))];
    new.extend(answer("p1", "m-c1", CLAUDE_ANSWER));
    let mut load = loads("sess-claude", &claude_modes()).to_vec();
    // Its own earlier answer comes back with the load: the log already has it.
    load.insert(
        1,
        json!({"update": {"update": {"sessionUpdate": "agent_message_chunk", "messageId": "m-c1",
            "content": {"type": "text", "text": CLAUDE_ANSWER}}}}),
    );
    load.push(prompt("p2", "sess-claude", Some(&wrapped(&missed, THIRD))));
    load.extend(answer("p2", "m-c2", CLAUDE_AGAIN));
    let claude = agent("claude-handshake.jsonl", "sess-claude", new, &load);
    let (claude, _) = scenario(&dir, "back-claude", &claude);
    dir.set_agents(&[(CLAUDE, claude), (CODEX, chatgpt(&dir))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, FIRST);
    ends(&host, &mut events, first);
    let second = send_to(&host, thread, SECOND, CODEX);
    ends(&host, &mut events, second);

    let third = send_to(&host, thread, THIRD, CLAUDE);
    ends(&host, &mut events, third);
    assert_eq!(events.turn_text(third), CLAUDE_AGAIN, "{}", events.dump());
    assert_eq!(
        events.turn_text(first),
        CLAUDE_ANSWER,
        "the replay is not logged again"
    );
    assert_eq!(
        sessions(&events),
        ["sess-claude", "sess-codex", "sess-claude"]
    );
    let all = handoffs(&events);
    assert_eq!(all.len(), 2, "{}", events.dump());
    let back = &all[1];
    let want = (1, 2, true, CODEX.to_owned(), CLAUDE.to_owned());
    assert_eq!(shape(back), want, "{}", events.dump());
    assert_eq!(settled(&events, back), [HandoffStatus::Delivered]);
    assert!(!all[0].resumed, "ChatGPT started a new session");
    no_context_notice(&events);
    host.shutdown();
}

/// As above, but Claude can no longer load its session: it starts a new one, which takes the
/// whole conversation, and the handover says it was not resumed.
#[test]
fn back_to_claude_that_cannot_load_its_session_hands_over_everything() {
    let dir = Dir::new("agent_switch_back_unloaded", &[]);
    let everything = header("Claude, then ChatGPT", 4, 4)
        + &format!(
            "\n\n[Earlier: the lawyer]\n> {FIRST}\n\n[Earlier: Claude's answer]\n> {CLAUDE_ANSWER}\
             \n\n[Earlier: the lawyer]\n> {SECOND}\n\n[Earlier: ChatGPT's answer]\n> {CHATGPT_ANSWER}"
        );
    let mut new = vec![prompt("p1", "sess-claude", Some(FIRST))];
    new.extend(answer("p1", "m-c1", CLAUDE_ANSWER));
    let load = vec![
        json!({"expect": {"method": "session/load", "as": "load",
            "params": {"sessionId": "sess-claude", "cwd": "{{any}}"}}}),
        json!({"respond": {"to": "load",
            "error": {"code": -32002, "message": "Session not found"}}}),
        json!({"expect": {"method": "session/new", "as": "new2", "params": {"cwd": "{{any}}"}}}),
        json!({"respond": {"to": "new2",
            "result": {"sessionId": "sess-claude-2", "modes": claude_modes()}}}),
        prompt("p2", "sess-claude-2", Some(&wrapped(&everything, THIRD))),
    ];
    let mut load = load;
    load.extend(answer("p2", "m-c2", CLAUDE_AGAIN));
    let claude = agent("claude-handshake.jsonl", "sess-claude", new, &load);
    let (claude, _) = scenario(&dir, "back-claude-unloaded", &claude);
    dir.set_agents(&[(CLAUDE, claude), (CODEX, chatgpt(&dir))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, FIRST);
    ends(&host, &mut events, first);
    let second = send_to(&host, thread, SECOND, CODEX);
    ends(&host, &mut events, second);

    let third = send_to(&host, thread, THIRD, CLAUDE);
    ends(&host, &mut events, third);
    assert_eq!(events.turn_text(third), CLAUDE_AGAIN, "{}", events.dump());
    assert_eq!(
        sessions(&events),
        ["sess-claude", "sess-codex", "sess-claude-2"]
    );
    let all = handoffs(&events);
    assert_eq!(all.len(), 2, "{}", events.dump());
    let back = &all[1];
    let want = (2, 4, false, CODEX.to_owned(), CLAUDE.to_owned());
    assert_eq!(shape(back), want, "{}", events.dump());
    assert_eq!(settled(&events, back), [HandoffStatus::Delivered]);
    // The handover carries the context: no notice that it was lost.
    no_context_notice(&events);
    host.shutdown();
}

/// Two conversations in one matter, each with its own Claude session. Going back to Claude in
/// the second loads the second's session. Claude's scenario is rewritten before each launch,
/// so a load of the first conversation's session is a mismatch.
#[test]
fn back_to_claude_in_one_conversation_never_loads_the_other_ones_session() {
    let dir = Dir::new("agent_switch_back_two", &[]);
    let claude_new = |session: &str, words: &str| {
        let mut new = vec![prompt("p1", session, Some(words))];
        new.extend(answer("p1", "m-c1", CLAUDE_ANSWER));
        // The load branch only ever expects the second conversation's session.
        let mut load = loads("sess-b", &claude_modes()).to_vec();
        load.push(prompt("p2", "sess-b", None));
        load.extend(answer("p2", "m-c2", CLAUDE_AGAIN));
        agent("claude-handshake.jsonl", session, new, &load)
    };
    let (claude, file) = scenario(&dir, "back-claude-two", &claude_new("sess-a", FIRST));
    dir.set_agents(&[(CLAUDE, claude), (CODEX, chatgpt(&dir))]);
    let host = dir.start(0);
    let (matter, one, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CLAUDE);
    let mut first_events = host.events(StreamId::Thread(one));
    let (first, _) = host.prompt(one, FIRST);
    ends(&host, &mut first_events, first);

    rewrite(&file, "back-claude-two", &claude_new("sess-b", SECOND));
    let two = host.create_thread(matter, CLAUDE);
    let mut events = host.events(StreamId::Thread(two));
    let (asked, _) = host.prompt(two, SECOND);
    ends(&host, &mut events, asked);
    let moved = send_to(&host, two, THIRD, CODEX);
    ends(&host, &mut events, moved);
    let back = send_to(&host, two, "And once more.", CLAUDE);
    ends(&host, &mut events, back);

    assert_eq!(sessions(&first_events), ["sess-a"]);
    assert_eq!(sessions(&events), ["sess-b", "sess-codex", "sess-b"]);
    let all = handoffs(&events);
    assert_eq!(all.len(), 2, "{}", events.dump());
    let want = (1, 2, true, CODEX.to_owned(), CLAUDE.to_owned());
    assert_eq!(shape(&all[1]), want, "{}", events.dump());
    assert_eq!(handoffs(&first_events).len(), 0);
    host.shutdown();
}

/// `ChatGPT` took a handover, then answered twice; back to Claude, then to `ChatGPT` again: it
/// loads its session, and the handover holds only Claude's turn since `ChatGPT`'s last answer.
#[test]
fn back_to_chatgpt_hands_over_only_what_came_after_its_last_answer() {
    const FOURTH: &str = "One more check.";
    const FIFTH: &str = "Last look.";
    let dir = Dir::new("agent_switch_back_chatgpt", &[]);
    let mut claude_new = vec![prompt("p1", "sess-claude", Some(FIRST))];
    claude_new.extend(answer("p1", "m-c1", CLAUDE_ANSWER));
    let mut claude_load = loads("sess-claude", &claude_modes()).to_vec();
    claude_load.push(prompt("p2", "sess-claude", None));
    claude_load.extend(answer("p2", "m-c2", CLAUDE_AGAIN));
    let claude = agent(
        "claude-handshake.jsonl",
        "sess-claude",
        claude_new,
        &claude_load,
    );
    let (claude, _) = scenario(&dir, "back-claude-d", &claude);

    let after = since("Claude", 2, 2)
        + &format!(
            "\n\n[Earlier: the lawyer]\n> {FOURTH}\n\n[Earlier: Claude's answer]\n> {CLAUDE_AGAIN}"
        );
    let mut codex_new = vec![prompt("p1", "sess-codex", None)];
    codex_new.extend(answer("p1", "m-x1", CHATGPT_ANSWER));
    codex_new.push(prompt("p2", "sess-codex", Some(THIRD)));
    codex_new.extend(answer("p2", "m-x2", "ChatGPT again."));
    let mut codex_load = loads("sess-codex", &codex_modes()).to_vec();
    codex_load.push(prompt("p3", "sess-codex", Some(&wrapped(&after, FIFTH))));
    codex_load.extend(answer("p3", "m-x3", "ChatGPT once more."));
    let codex = agent(
        "codex-handshake.jsonl",
        "sess-codex",
        codex_new,
        &codex_load,
    );
    let (codex, _) = scenario(&dir, "back-codex-d", &codex);
    dir.set_agents(&[(CLAUDE, claude), (CODEX, codex)]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, FIRST);
    ends(&host, &mut events, first);
    let second = send_to(&host, thread, SECOND, CODEX);
    ends(&host, &mut events, second);
    let (third, _) = host.prompt(thread, THIRD);
    ends(&host, &mut events, third);
    let fourth = send_to(&host, thread, FOURTH, CLAUDE);
    ends(&host, &mut events, fourth);

    let fifth = send_to(&host, thread, FIFTH, CODEX);
    ends(&host, &mut events, fifth);
    assert_eq!(events.turn_text(fifth), "ChatGPT once more.");
    assert_eq!(
        sessions(&events),
        ["sess-claude", "sess-codex", "sess-claude", "sess-codex"]
    );
    let all = handoffs(&events);
    assert_eq!(all.len(), 3, "{}", events.dump());
    // Back to Claude: ChatGPT's two turns.
    let want = (2, 4, true, CODEX.to_owned(), CLAUDE.to_owned());
    assert_eq!(shape(&all[1]), want, "{}", events.dump());
    // Back to ChatGPT: Claude's turn alone.
    let want = (1, 2, true, CLAUDE.to_owned(), CODEX.to_owned());
    assert_eq!(shape(&all[2]), want, "{}", events.dump());
    assert_eq!(settled(&events, &all[2]), [HandoffStatus::Delivered]);
    no_context_notice(&events);
    host.shutdown();
}

/// Back to Claude, whose launch fails before its session attaches: the switch stands and owes
/// the handover, which no `ContextHandoff` logged yet. The next message loads Claude's session
/// and carries only what it missed, the failed turn included.
#[test]
fn a_switch_back_whose_launch_fails_owes_only_what_was_missed() {
    let dir = Dir::new("agent_switch_back_owed", &[]);
    let mut new = vec![prompt("p1", "sess-claude", Some(FIRST))];
    new.extend(answer("p1", "m-c1", CLAUDE_ANSWER));
    let mut load = loads("sess-claude", &claude_modes()).to_vec();
    load.push(prompt("p2", "sess-claude", None));
    load.extend(answer("p2", "m-c2", CLAUDE_AGAIN));
    let steps = agent("claude-handshake.jsonl", "sess-claude", new, &load);
    let (claude, file) = scenario(&dir, "back-claude-owed", &steps);
    dir.set_agents(&[(CLAUDE, claude), (CODEX, chatgpt(&dir))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, FIRST);
    ends(&host, &mut events, first);
    let second = send_to(&host, thread, SECOND, CODEX);
    ends(&host, &mut events, second);

    // Claude's next launch refuses `initialize`.
    let refused = vec![
        json!({"expect": {"method": "initialize", "as": "init", "params": {"protocolVersion": 1}}}),
        json!({"respond": {"to": "init", "error": {"code": -32603, "message": "broken"}}}),
    ];
    rewrite(&file, "back-claude-owed", &refused);
    let third = send_to(&host, thread, THIRD, CLAUDE);
    let ended = events.until_turn_ended(third);
    assert_eq!(ended.stop, StopReason::Failed, "{}", events.dump());
    host.settle();
    events.drain_ready();
    assert_eq!(handoffs(&events).len(), 1, "{}", events.dump());

    rewrite(&file, "back-claude-owed", &steps);
    let (fourth, _) = host.prompt(thread, "Try again.");
    ends(&host, &mut events, fourth);
    assert_eq!(events.turn_text(fourth), CLAUDE_AGAIN, "{}", events.dump());
    assert_eq!(
        sessions(&events),
        ["sess-claude", "sess-codex", "sess-claude"]
    );
    let all = handoffs(&events);
    assert_eq!(all.len(), 2, "{}", events.dump());
    let owed = &all[1];
    assert!(owed.resumed, "{}", events.dump());
    // ChatGPT's answer, not Claude's own.
    assert_eq!(owed.answers_covered.get(), 1);
    assert_eq!(
        (owed.from.agent.as_str(), owed.to.agent.as_str()),
        (CODEX, CLAUDE)
    );
    assert_eq!(settled(&events, owed), [HandoffStatus::Delivered]);
    no_context_notice(&events);
    host.shutdown();
}

/// As above, with a restart between the failed launch and the next message: the handover held
/// for the attach lived in memory only, so start-up owes it again. The next message loads
/// Claude's session when it still loads and hands over only what it missed, else it starts a
/// new session and hands over everything.
fn owed_across_a_restart(name: &str, loads_again: bool) {
    let dir = Dir::new(name, &[]);
    let mut new = vec![prompt("p1", "sess-claude", Some(FIRST))];
    new.extend(answer("p1", "m-c1", CLAUDE_ANSWER));
    // The first launch takes the new branch; the load branch waits for the restart.
    let unused = loads("sess-claude", &claude_modes());
    let steps = agent(
        "claude-handshake.jsonl",
        "sess-claude",
        new.clone(),
        &unused,
    );
    let (claude, file) = scenario(&dir, name, &steps);
    dir.set_agents(&[(CLAUDE, claude), (CODEX, chatgpt(&dir))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, FIRST);
    ends(&host, &mut events, first);
    let second = send_to(&host, thread, SECOND, CODEX);
    ends(&host, &mut events, second);
    let refused = vec![
        json!({"expect": {"method": "initialize", "as": "init", "params": {"protocolVersion": 1}}}),
        json!({"respond": {"to": "init", "error": {"code": -32603, "message": "broken"}}}),
    ];
    rewrite(&file, name, &refused);
    let third = send_to(&host, thread, THIRD, CLAUDE);
    let ended = events.until_turn_ended(third);
    assert_eq!(ended.stop, StopReason::Failed, "{}", events.dump());
    host.settle();
    drop(events);
    host.shutdown();

    let mut load = if loads_again {
        loads("sess-claude", &claude_modes()).to_vec()
    } else {
        vec![
            json!({"expect": {"method": "session/load", "as": "load",
                "params": {"sessionId": "sess-claude", "cwd": "{{any}}"}}}),
            json!({"respond": {"to": "load",
                "error": {"code": -32002, "message": "Session not found"}}}),
            json!({"expect": {"method": "session/new", "as": "new2", "params": {"cwd": "{{any}}"}}}),
            json!({"respond": {"to": "new2",
                "result": {"sessionId": "sess-claude-2", "modes": claude_modes()}}}),
        ]
    };
    let session = if loads_again {
        "sess-claude"
    } else {
        "sess-claude-2"
    };
    load.push(prompt("p2", session, None));
    load.extend(answer("p2", "m-c2", CLAUDE_AGAIN));
    rewrite(
        &file,
        name,
        &agent("claude-handshake.jsonl", "sess-claude", new, &load),
    );
    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let (fourth, _) = host.prompt(thread, "Try again.");
    ends(&host, &mut events, fourth);
    assert_eq!(events.turn_text(fourth), CLAUDE_AGAIN, "{}", events.dump());
    assert_eq!(sessions(&events), ["sess-claude", "sess-codex", session]);
    let all = handoffs(&events);
    assert_eq!(
        all.len(),
        2,
        "the next message carries it: {}",
        events.dump()
    );
    let owed = &all[1];
    assert_eq!(
        (owed.from.agent.as_str(), owed.to.agent.as_str()),
        (CODEX, CLAUDE),
        "{}",
        events.dump()
    );
    // Resumed: `ChatGPT`'s answer only; a new session: both answers.
    let answers = if loads_again { 1 } else { 2 };
    assert_eq!(
        (owed.resumed, owed.answers_covered.get()),
        (loads_again, answers),
        "{}",
        events.dump()
    );
    assert_eq!(settled(&events, owed), [HandoffStatus::Delivered]);
    host.shutdown();
}

#[test]
fn a_handover_held_for_an_attach_is_owed_after_a_restart_and_resumes_the_session() {
    owed_across_a_restart("agent_switch_back_restart", true);
}

#[test]
fn a_handover_held_for_an_attach_is_owed_after_a_restart_in_full_when_the_session_is_gone() {
    owed_across_a_restart("agent_switch_back_restart_gone", false);
}
