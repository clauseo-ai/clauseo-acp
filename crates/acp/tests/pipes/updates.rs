//! How agent updates reach the host: chunk sizes, command hints, grouped config values and a
//! tool call's raw input and output.

use clauseo_acp::{AcpEvent, AgentUpdate, MessageRole};
use clauseo_protocol::{
    AvailableCommand, ConfigChoice, ConfigOptionKind, ContentBlock, TextOrBlob,
};
use serde_json::{Value, json};

use crate::harness::{drain, handshake, pipes};

/// Plays the agent sending `updates` after the handshake, and returns the first `count` updates
/// the host received.
fn received(updates: &[Value], count: usize) -> Vec<AgentUpdate> {
    let (client, events, driver, mut agent) = pipes();
    let calls = async move {
        handshake(&client).await;
        let mut received = Vec::new();
        while received.len() < count {
            match events.recv().await.unwrap() {
                AcpEvent::Update { update, .. } => received.push(update),
                other => panic!("expected an update, got {other:?}"),
            }
        }
        assert_eq!(drain(&events), []);
        received
    };
    let script = async {
        agent.start().await;
        for update in updates {
            agent.update(update);
        }
        agent.read_end().await;
    };
    let (driver, received, ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    received
}

fn answer_chunk(text: &str) -> Value {
    json!({"sessionUpdate": "agent_message_chunk", "messageId": "m-1",
        "content": {"type": "text", "text": text}})
}

/// The texts of answer chunks, in order; any other update fails the test.
fn chunk_texts(updates: Vec<AgentUpdate>) -> Vec<String> {
    updates
        .into_iter()
        .map(|update| match update {
            AgentUpdate::MessageChunk {
                role: MessageRole::Answer,
                content: ContentBlock::Text { text },
                ..
            } => text,
            other => panic!("expected an answer chunk, got {other:?}"),
        })
        .collect()
}

/// A text chunk of exactly 1 MiB reaches the host as one chunk; one byte more is cut in two.
#[test]
fn text_chunk_of_1_mib_stays_whole_and_one_byte_more_is_cut() {
    const MIB: usize = 1024 * 1024;
    let whole = "a".repeat(MIB);
    let over = format!("{}b", "c".repeat(MIB));
    let texts = chunk_texts(received(&[answer_chunk(&whole), answer_chunk(&over)], 3));
    assert_eq!(texts.len(), 3);
    assert_eq!(texts[0], whole);
    assert_eq!(texts[1], "c".repeat(MIB));
    assert_eq!(texts[2], "b");
}

/// An empty text chunk still reaches the host, as one empty chunk.
#[test]
fn empty_text_chunk_reaches_the_host() {
    let texts = chunk_texts(received(&[answer_chunk(""), answer_chunk("after")], 2));
    assert_eq!(texts, ["", "after"]);
}

/// A command's unstructured input keeps its hint for the host to show.
#[test]
fn command_input_hint_is_kept() {
    let update = json!({"sessionUpdate": "available_commands_update", "availableCommands": [
        {"name": "redline", "description": "Redline a clause", "input": {"hint": "clause number"}},
        {"name": "summarise", "description": "Summarise the matter"},
    ]});
    let [AgentUpdate::Commands(commands)] = <[_; 1]>::try_from(received(&[update], 1)).unwrap()
    else {
        panic!("expected commands");
    };
    assert_eq!(
        commands,
        [
            AvailableCommand {
                name: "redline".into(),
                description: "Redline a clause".to_owned(),
                input_hint: Some("clause number".to_owned()),
            },
            AvailableCommand {
                name: "summarise".into(),
                description: "Summarise the matter".to_owned(),
                input_hint: None,
            },
        ]
    );
}

/// A select option whose values come in groups keeps every value, each with its group's name.
#[test]
fn grouped_config_values_keep_their_group() {
    let update = json!({"sessionUpdate": "config_option_update", "configOptions": [{
        "id": "model", "name": "Model", "category": "model", "type": "select",
        "currentValue": "fast",
        "options": [
            {"group": "g-quick", "name": "Quick", "options": [{"value": "fast", "name": "Fast"}]},
            {"group": "g-deep", "name": "Deep", "options": [
                {"value": "careful", "name": "Careful", "description": "Slower"},
            ]},
        ],
    }]});
    let [AgentUpdate::ConfigOptions(options)] = <[_; 1]>::try_from(received(&[update], 1)).unwrap()
    else {
        panic!("expected config options");
    };
    let [option] = <[_; 1]>::try_from(options).unwrap();
    assert_eq!(option.id, "model");
    let ConfigOptionKind::Select {
        current_value,
        options,
    } = option.kind
    else {
        panic!("expected a select option, got {:?}", option.kind);
    };
    assert_eq!(current_value, "fast");
    assert_eq!(
        options,
        [
            ConfigChoice {
                value: "fast".into(),
                name: "Fast".into(),
                description: None,
                group: Some("Quick".to_owned()),
            },
            ConfigChoice {
                value: "careful".into(),
                name: "Careful".into(),
                description: Some("Slower".to_owned()),
                group: Some("Deep".to_owned()),
            },
        ]
    );
}

/// A tool call's raw input and output reach the host as JSON text, in the call and in
/// its updates.
#[test]
fn tool_call_raw_input_and_output_kept_as_json() {
    let call = json!({"sessionUpdate": "tool_call", "toolCallId": "tc-1", "title": "Read",
        "kind": "read", "rawInput": {"path": "lease.md", "lines": [1, 2]}});
    let patch = json!({"sessionUpdate": "tool_call_update", "toolCallId": "tc-1",
        "status": "completed", "rawOutput": {"bytes": 42}});
    let [
        AgentUpdate::ToolCall(call),
        AgentUpdate::ToolCallUpdate { patch, .. },
    ] = <[_; 2]>::try_from(received(&[call, patch], 2)).unwrap()
    else {
        panic!("expected a tool call and its update");
    };
    let inline = |raw: Option<TextOrBlob>| match raw {
        Some(TextOrBlob::Inline(text)) => serde_json::from_str::<Value>(&text).unwrap(),
        other => panic!("expected inline JSON, got {other:?}"),
    };
    assert_eq!(
        inline(call.raw_input),
        json!({"path": "lease.md", "lines": [1, 2]})
    );
    assert_eq!(call.raw_output, None);
    assert_eq!(inline(patch.raw_output), json!({"bytes": 42}));
}
