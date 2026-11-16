//! L7: the longest prompt Clauseo sends, through store, search, the agent and a reopen. The
//! send check caps prompt text at `MAX_PROMPT_TEXT_BYTES` (6 MiB) counted as JSON-escaped bytes
//! (`clauseo_protocol::command::validate`), so that the agent's replay of it as one
//! `user_message_chunk` line on `session/load` fits under the ACP reader's 8 MiB line cap. Here
//! the prompt is exactly at the cap, with quotes, backslashes, newlines, a control character and
//! non-ASCII text, so raw and escaped sizes differ. The fake replays it in claude-agent-acp's
//! shape (`messageId`, `_meta`), the line's size is checked under 8 MiB, the load attaches with
//! no skipped line, and the next prompt ends `end_turn`. One escaped byte more is refused.

use clauseo_protocol::{
    Count, DomainEvent, ErrorCode, MAX_PROMPT_TEXT_BYTES, Principal, SearchAnswer, SearchKind,
    SearchRequest, SearchScope, StopReason, StreamId, escaped_len,
};
use serde_json::{Value, json};

use crate::load_line_cap::{
    MAX_LINE_BYTES, Warns, answered, attached, failures, first_session, skips, write_scenario,
};
use crate::support::{Dir, FAKE, Running, send_prompt, wait};

/// The word at the prompt's end that search must find.
const TAIL_WORD: &str = "zanzibartail";

/// A prompt of exactly [`MAX_PROMPT_TEXT_BYTES`] escaped bytes ending in [`TAIL_WORD`].
fn maximal_prompt() -> String {
    // `"` and `\` and a newline cost 2, the control character 6, the Devanagari and `é` their
    // UTF-8 bytes.
    let piece = "Clause \"4\" of the \\lease\\ deed\nपट्टा विलेख \u{1}é ";
    let tail = format!(" {TAIL_WORD}");
    let budget = MAX_PROMPT_TEXT_BYTES - escaped_len(&tail);
    let count = budget / escaped_len(piece);
    let mut text = piece.repeat(count);
    text.push_str(&"a".repeat(budget - count * escaped_len(piece)));
    text.push_str(&tail);
    assert_eq!(escaped_len(&text), MAX_PROMPT_TEXT_BYTES);
    assert_ne!(
        text.len(),
        MAX_PROMPT_TEXT_BYTES,
        "raw and escaped sizes differ"
    );
    text
}

/// The replay of `text` as claude-agent-acp writes it on `session/load`: one line.
fn replay_line(text: &str) -> String {
    json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "sess-1",
        "update": {"sessionUpdate": "user_message_chunk",
        "content": {"type": "text", "text": text},
        "messageId": "0b7c6f8e-1d2a-4f3b-9c8d-7e6f5a4b3c2d",
        "_meta": {"claudeCode": {"parentToolUseId": null}}}}})
    .to_string()
}

/// On `session/new` one turn; on `session/load` the replay of `line`, the load's answer, and
/// one more prompt.
fn steps(line: &str) -> Vec<Value> {
    let mut load = vec![
        json!({"expect": {"method": "session/load", "as": "load",
            "params": {"sessionId": "sess-1", "cwd": "{{any}}"}}}),
        json!({"send_line": line}),
        json!({"respond": {"to": "load", "result": {}}}),
    ];
    load.extend(answered("p2", "Second answer."));
    vec![json!({"one_of": [first_session(), load]})]
}

/// Searches for `word` until a prompt hit comes back; the watchdog fails the test if none does.
fn until_prompt_found(host: &Running, word: &str) {
    let request = SearchRequest {
        query: word.to_owned(),
        scope: SearchScope::All,
        include_archived: false,
        kinds: Some(vec![SearchKind::Prompt]),
        limit: None,
        cursor: None,
        generation: Count::new(0).unwrap(),
    };
    wait("search to find the prompt's last word", async {
        loop {
            let answer = host
                .handle
                .search(Principal::LocalDesktop, "l7", request.clone())
                .await
                .unwrap();
            let found = match answer {
                SearchAnswer::Page(page) => {
                    page.hits.iter().any(|hit| hit.kind == SearchKind::Prompt)
                }
                SearchAnswer::Cancelled => false,
            };
            if found {
                return;
            }
        }
    });
}

#[test]
fn a_maximal_prompt_is_stored_found_replayed_under_the_cap_and_reopens() {
    let warns = Warns::keep();
    let text = maximal_prompt();
    let line = replay_line(&text);
    assert!(
        line.len() < MAX_LINE_BYTES,
        "the replayed line is {} bytes",
        line.len()
    );
    let dir = Dir::new("max_prompt_reopen", &[]);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    write_scenario(&dir, &folder, "max-prompt-reopen", steps(&line));

    // Run 0: the prompt goes to the agent and is stored and indexed.
    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, &text);
    let ended = events.until_turn_ended(first);
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    until_prompt_found(&host, TAIL_WORD);
    // One escaped byte more is refused at send.
    let over = format!("{text}a");
    assert_eq!(escaped_len(&over), MAX_PROMPT_TEXT_BYTES + 1);
    host.refused(send_prompt(thread, &over), ErrorCode::PromptTooLong);
    host.shutdown();

    // Run 1: the reopening prompt loads `sess-1`, whose replay carries the whole prompt.
    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let (second, _) = host.prompt(thread, "Second question.");
    let ended = events.until_turn_ended(second);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert!(events.turn_text(second).contains("Second answer."));
    assert_eq!(attached(&events), ["sess-1", "sess-1"], "{}", events.dump());
    assert_eq!(failures(&events), 0, "{}", events.dump());
    // The replay adds nothing to the transcript: one message per prompt.
    let messages = events.count(|event| matches!(event, DomainEvent::UserMessage(_)));
    assert_eq!(messages, 2, "{}", events.dump());
    host.shutdown();
    let skipped = skips(&warns);
    assert!(skipped.is_empty(), "{skipped:#?}");
}
