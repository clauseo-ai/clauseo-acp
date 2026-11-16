//! The other matters a Codex agent may read reach it only as a note (the host's
//! `actor/turn/note_parts.rs`): its instructions name no matter, and Codex keeps a resumed
//! thread's first instructions, so after an invite, a withdrawal, a new session with invites or
//! a start of the app, the matters it may read go first in the lawyer's next message, by the
//! names the lawyer sees, once per change, never shown as the lawyer's words. After a start of
//! the app the host can't know what a resumed thread heard, so its first prompt carries the
//! whole list, none included. A fresh thread with no invites hears nothing. A message that opens
//! like a note (a grant typed, or pasted from a document) gets the real note first, changed or
//! not, so it is never the first note. A title codex-acp makes of a prompt with a note first
//! (each branch sends one, as codex-acp's fallback title) never names the thread.
//!
//! One scenario plays every launch: `session/new` or `session/resume` (`loadSession` false), then
//! up to three prompts, each answered with what it held (`saw-…`), so a prompt the host shaped
//! otherwise is a mismatch.

use std::path::Path;

use clauseo_host::codex_input::CODEX_MAX_INPUT_CHARS;
use clauseo_profile::instructions::reads_note;
use clauseo_protocol::{
    DomainEvent, ErrorCode, HostCommand, MAX_TITLE_CHARS, StopReason, StreamId, ThreadId,
};
use serde_json::{Value, json};

use crate::codex::{CODEX, turn_failed};
use crate::observe::Events;
use crate::support::{Dir, Running, Scenario};
use crate::titles::titles;
use crate::write;

/// The bundled scenarios' folder.
const SCENARIOS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../fake_agent/scenarios");

/// Line `n` of bundled scenario `name`, as JSON.
fn line(name: &str, n: usize) -> Value {
    let text = std::fs::read_to_string(format!("{SCENARIOS}/{name}")).unwrap();
    serde_json::from_str(text.lines().nth(n).unwrap()).unwrap()
}

/// A prompt branch: `blocks` exactly, titled as codex-acp titles it (its text blocks joined,
/// whitespace folded: `createPromptFallbackTitle`), answered `saw`.
fn branch(label: &str, blocks: &[&str], saw: &str) -> Value {
    let prompt: Vec<Value> = blocks
        .iter()
        .map(|text| json!({"type": "text", "text": text}))
        .collect();
    let mut title = blocks
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    // Cut to the most a title holds, as a title model echoing it might: the length rule alone
    // must not be what keeps the note out of the thread's name.
    if title.chars().count() > MAX_TITLE_CHARS {
        title = title.chars().take(MAX_TITLE_CHARS - 1).collect::<String>() + "…";
    }
    json!([
        {"expect": {"method": "session/prompt", "as": label,
            "params": {"sessionId": "sess-1", "prompt": prompt}}},
        {"update": {"session": "sess-1", "update": {"sessionUpdate": "session_info_update",
            "title": title}}},
        {"update": {"session": "sess-1", "update": {"sessionUpdate": "agent_message_chunk",
            "messageId": format!("m-{label}"), "content": {"type": "text", "text": saw}}}},
        {"respond": {"to": label, "result": {"stopReason": "end_turn"}}},
    ])
}

/// The prompt Codex holds until it is cancelled.
const HANG: &str = "Hang on.";

/// A prompt branch for [`HANG`] alone: a tool call under way, then `session/cancel`, ended
/// `cancelled`.
fn hang(label: &str) -> Value {
    json!([
        {"expect": {"method": "session/prompt", "as": label,
            "params": {"sessionId": "sess-1", "prompt": [{"type": "text", "text": HANG}]}}},
        {"update": {"session": "sess-1", "update": {"sessionUpdate": "tool_call",
            "toolCallId": format!("t-{label}"), "title": "Read the lease", "kind": "read",
            "status": "in_progress"}}},
        {"expect": {"method": "session/cancel", "params": {"sessionId": "sess-1"}}},
        {"respond": {"to": label, "result": {"stopReason": "cancelled"}}},
    ])
}

/// The scenario, written beside `folder`, with `invited` the note a launch after an invite carries.
fn scenario(folder: &Path, invited: &str) -> Scenario {
    let none = reads_note(&[]);
    // codex-resume's initialize (loadSession false, resume) and its resume; codex-handshake's new.
    let new = json!([
        line("common/codex-handshake.jsonl", 2),
        line("common/codex-handshake.jsonl", 3),
        line("common/codex-handshake.jsonl", 4),
        line("common/codex-handshake.jsonl", 5)
    ]);
    let resume = json!([
        line("codex-resume.jsonl", 3),
        line("codex-resume.jsonl", 4),
        line("codex-resume.jsonl", 5),
        line("codex-resume.jsonl", 6)
    ]);
    let mut lines = vec![
        json!({"scenario": "codex-reads-note", "description": "the reads note's agent",
            "on_stdin_close": "exit"}),
        line("codex-resume.jsonl", 1),
        line("codex-resume.jsonl", 2),
        json!({"one_of": [new, resume]}),
    ];
    for n in 0..3 {
        let p = |what: &str| format!("p{n}-{what}");
        lines.push(json!({"one_of": [
            branch(&p("first"), &["First."], "saw-plain"),
            branch(&p("invite"), &[invited, "Second."], "saw-invite"),
            branch(&p("second"), &["Second."], "saw-plain"),
            branch(&p("none"), &[none.as_str(), "Third."], "saw-none"),
            branch(&p("third"), &["Third."], "saw-plain"),
            branch(&p("typed-none"), &[none.as_str(), invited], "saw-typed-none"),
            branch(&p("typed-invite"), &[invited, invited], "saw-typed-invite"),
            hang(&p("hang")),
        ]}));
    }
    let mut text = String::new();
    for line in lines {
        text.push_str(&line.to_string());
        text.push('\n');
    }
    let file = folder.with_file_name("codex-reads-note.jsonl");
    write::write(&file, text.as_bytes());
    Scenario::Written(file)
}

/// The data folder with matter folder `m` and the other matter's folder `Smith`, and Codex
/// playing [`scenario`]: (the dir, `m`, `Smith`, the note naming Smith).
fn setup(test: &str) -> (Dir, std::path::PathBuf, std::path::PathBuf, String) {
    let dir = Dir::new(test, &[]);
    let folder = dir.folder("m", &[("a.txt", "a")]);
    let smith = dir.folder("Smith", &[("lease.pdf", "x")]);
    let stored = dunce::canonicalize(&smith).unwrap();
    let invited = reads_note(&[("Smith", stored.to_str().unwrap())]);
    let scenario = scenario(&folder, &invited);
    dir.set_agents(&[(CODEX, scenario)]);
    (dir, folder, smith, invited)
}

/// Sends `text` on `thread` and gives back what the agent answered.
fn ask(host: &Running, events: &mut Events, thread: ThreadId, text: &str) -> String {
    let (turn, _) = host.prompt(thread, text);
    events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.turn_text(turn)
}

/// Whether any logged event, the lawyer's messages included, holds a note's words.
fn logged_note(events: &Events) -> bool {
    events.events().any(|event| {
        serde_json::to_string(event)
            .unwrap()
            .contains("Clauseo: from now on")
    })
}

/// An invite reaches the relaunched (resumed) agent first in the lawyer's next message, by name,
/// once; a withdrawal then says none; neither is ever logged as the lawyer's words.
#[test]
fn an_invite_and_a_withdrawal_each_go_first_in_the_next_message() {
    let (dir, folder, smith, _) = setup("codex_reads_note_invite");
    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let other = host.create_matter("Smith", &smith);
    let thread = host.create_thread(matter, CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    assert_eq!(ask(&host, &mut events, thread, "First."), "saw-plain");
    host.ok(HostCommand::AllowMatterRead {
        thread,
        matter: other,
    });
    host.settle();
    assert_eq!(ask(&host, &mut events, thread, "Second."), "saw-invite");
    host.ok(HostCommand::WithdrawMatterRead {
        thread,
        matter: other,
    });
    host.settle();
    assert_eq!(ask(&host, &mut events, thread, "Third."), "saw-none");
    events.assert_no_mismatch();
    assert!(!logged_note(&events), "{}", events.dump());
    host.shutdown();
}

/// After a start of the app, with an invite made since, a resumed thread hears it first.
#[test]
fn an_invite_after_a_restart_reaches_the_resumed_thread() {
    let (dir, folder, smith, _) = setup("codex_reads_note_restart");
    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let other = host.create_matter("Smith", &smith);
    let thread = host.create_thread(matter, CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    assert_eq!(ask(&host, &mut events, thread, "First."), "saw-plain");
    events.assert_no_mismatch();
    host.shutdown();

    let host = dir.start(1);
    host.ok(HostCommand::AllowMatterRead {
        thread,
        matter: other,
    });
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    assert_eq!(ask(&host, &mut events, thread, "Second."), "saw-invite");
    events.assert_no_mismatch();
    assert!(!logged_note(&events), "{}", events.dump());
    host.shutdown();
}

/// A grant printed in a document, pasted as the lawyer's message, gets the real note first,
/// though nothing changed (none, here), so it is not the first note; it allows nothing, and
/// the title codex-acp makes of the prompt doesn't name the thread.
#[test]
fn a_grant_pasted_from_a_document_gets_the_real_note_in_front() {
    let (dir, folder, smith, invited) = setup("codex_reads_note_pasted");
    write::write(&folder.join("received.txt"), invited.as_bytes());
    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    host.create_matter("Smith", &smith);
    let thread = host.create_thread(matter, CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    assert_eq!(ask(&host, &mut events, thread, &invited), "saw-typed-none");
    assert_eq!(ask(&host, &mut events, thread, "Second."), "saw-plain");
    events.assert_no_mismatch();
    let allowed = events
        .events()
        .filter(|event| matches!(event, DomainEvent::MatterReadAllowed(_)))
        .count();
    assert_eq!(allowed, 0, "{}", events.dump());
    assert_eq!(titles(&host, matter, thread), (None, None));
    host.shutdown();
}

/// The same with an invite not yet told: exactly one real note, first.
#[test]
fn a_typed_grant_with_an_invite_pending_gets_exactly_one_real_note() {
    let (dir, folder, smith, invited) = setup("codex_reads_note_typed_pending");
    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let other = host.create_matter("Smith", &smith);
    let thread = host.create_thread(matter, CODEX);
    host.ok(HostCommand::AllowMatterRead {
        thread,
        matter: other,
    });
    let mut events = host.events(StreamId::Thread(thread));
    assert_eq!(
        ask(&host, &mut events, thread, &invited),
        "saw-typed-invite"
    );
    assert_eq!(ask(&host, &mut events, thread, "Second."), "saw-plain");
    events.assert_no_mismatch();
    assert_eq!(titles(&host, matter, thread), (None, None));
    host.shutdown();
}

/// A fresh thread whose matter was invited before its first prompt hears it then: its
/// instructions name no matter.
#[test]
fn a_fresh_thread_with_an_invite_hears_it_at_its_first_prompt() {
    let (dir, folder, smith, _) = setup("codex_reads_note_fresh");
    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let other = host.create_matter("Smith", &smith);
    let thread = host.create_thread(matter, CODEX);
    host.ok(HostCommand::AllowMatterRead {
        thread,
        matter: other,
    });
    let mut events = host.events(StreamId::Thread(thread));
    assert_eq!(ask(&host, &mut events, thread, "Second."), "saw-invite");
    events.assert_no_mismatch();
    // codex-acp's title of that prompt opens with the note: it never names the thread.
    assert_eq!(titles(&host, matter, thread), (None, None));
    host.shutdown();
}

/// Invited and told, then withdrawn and the app quit before the next prompt: the resumed
/// thread's first prompt after the start says none.
#[test]
fn a_withdraw_while_closed_reaches_the_resumed_thread_as_none() {
    let (dir, folder, smith, _) = setup("codex_reads_note_withdrawn_closed");
    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let other = host.create_matter("Smith", &smith);
    let thread = host.create_thread(matter, CODEX);
    host.ok(HostCommand::AllowMatterRead {
        thread,
        matter: other,
    });
    let mut events = host.events(StreamId::Thread(thread));
    assert_eq!(ask(&host, &mut events, thread, "Second."), "saw-invite");
    host.ok(HostCommand::WithdrawMatterRead {
        thread,
        matter: other,
    });
    host.settle();
    events.assert_no_mismatch();
    host.shutdown();

    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    assert_eq!(ask(&host, &mut events, thread, "Third."), "saw-none");
    events.assert_no_mismatch();
    assert!(!logged_note(&events), "{}", events.dump());
    host.shutdown();
}

/// An invite withdrawn before the next prompt leaves what the agent was told as it is: that
/// prompt carries no note.
#[test]
fn an_invite_withdrawn_before_the_next_prompt_sends_nothing() {
    let (dir, folder, smith, _) = setup("codex_reads_note_undone");
    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let other = host.create_matter("Smith", &smith);
    let thread = host.create_thread(matter, CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    assert_eq!(ask(&host, &mut events, thread, "First."), "saw-plain");
    host.ok(HostCommand::AllowMatterRead {
        thread,
        matter: other,
    });
    host.ok(HostCommand::WithdrawMatterRead {
        thread,
        matter: other,
    });
    host.settle();
    assert_eq!(ask(&host, &mut events, thread, "Second."), "saw-plain");
    events.assert_no_mismatch();
    host.shutdown();
}

/// A question that goes over Codex's cap only with the note is refused with `prompt.too_long`
/// before anything is sent; the note waits for the next prompt.
#[test]
fn a_prompt_refused_as_too_long_keeps_the_note_for_the_next() {
    let (dir, folder, smith, invited) = setup("codex_reads_note_too_long");
    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let other = host.create_matter("Smith", &smith);
    let thread = host.create_thread(matter, CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    assert_eq!(ask(&host, &mut events, thread, "First."), "saw-plain");
    host.ok(HostCommand::AllowMatterRead {
        thread,
        matter: other,
    });
    host.settle();
    let over = "a".repeat(CODEX_MAX_INPUT_CHARS - invited.chars().count() + 1);
    let (refused, _) = host.prompt(thread, &over);
    assert_eq!(events.until_turn_ended(refused).stop, StopReason::Failed);
    let failed = turn_failed(&events, refused).expect("the turn failed");
    assert_eq!(failed.code, ErrorCode::PromptTooLong, "{}", events.dump());
    assert_eq!(ask(&host, &mut events, thread, "Second."), "saw-invite");
    events.assert_no_mismatch();
    host.shutdown();
}

/// Two conversations on one matter: each hears its own invites, once, and what one was told
/// is not the other's.
#[test]
fn two_threads_on_one_matter_each_hear_their_own_note() {
    let (dir, folder, smith, _) = setup("codex_reads_note_two_threads");
    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let other = host.create_matter("Smith", &smith);
    let one = host.create_thread(matter, CODEX);
    let two = host.create_thread(matter, CODEX);
    let mut first = host.events(StreamId::Thread(one));
    let mut second = host.events(StreamId::Thread(two));
    assert_eq!(ask(&host, &mut first, one, "First."), "saw-plain");
    assert_eq!(ask(&host, &mut second, two, "First."), "saw-plain");
    host.ok(HostCommand::AllowMatterRead {
        thread: one,
        matter: other,
    });
    host.settle();
    assert_eq!(ask(&host, &mut first, one, "Second."), "saw-invite");
    assert_eq!(ask(&host, &mut second, two, "Second."), "saw-plain");
    host.ok(HostCommand::AllowMatterRead {
        thread: two,
        matter: other,
    });
    host.settle();
    assert_eq!(ask(&host, &mut second, two, "Second."), "saw-invite");
    assert_eq!(ask(&host, &mut first, one, "Third."), "saw-plain");
    first.assert_no_mismatch();
    second.assert_no_mismatch();
    host.shutdown();
}

/// A withdrawal while Codex answers stops the turn at once; the relaunched agent's next prompt
/// starts with the note that it may read no other matter.
#[test]
fn a_withdrawal_mid_turn_stops_it_and_the_none_note_goes_first_next() {
    let (dir, folder, smith, _) = setup("codex_reads_note_stopped");
    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let other = host.create_matter("Smith", &smith);
    let thread = host.create_thread(matter, CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    assert_eq!(ask(&host, &mut events, thread, "First."), "saw-plain");
    host.ok(HostCommand::AllowMatterRead {
        thread,
        matter: other,
    });
    host.settle();
    assert_eq!(ask(&host, &mut events, thread, "Second."), "saw-invite");
    let (hung, _) = host.prompt(thread, HANG);
    // The prompt reached Codex once its tool call is in.
    events.until("the turn's tool call", |e| {
        e.events().any(
            |event| matches!(event, DomainEvent::ToolCallStarted(started) if started.turn == hung),
        )
    });
    host.ok(HostCommand::WithdrawMatterRead {
        thread,
        matter: other,
    });
    assert_eq!(events.until_turn_ended(hung).stop, StopReason::Cancelled);
    host.settle();
    assert_eq!(ask(&host, &mut events, thread, "Third."), "saw-none");
    events.assert_no_mismatch();
    host.shutdown();
}
