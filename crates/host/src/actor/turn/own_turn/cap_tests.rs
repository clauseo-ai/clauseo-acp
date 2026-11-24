//! The held-output cap (`cap.rs`) on a pile-up built by hand: only words go, oldest first, and
//! one pile-up says so once.

use std::io::Write;
use std::sync::{Arc, Mutex};

use clauseo_acp::{AcpEvent, AgentUpdate, MessageRole, Origin, ToolCall};
use clauseo_protocol::{
    AgentSessionId, ContentBlock, Cost, Count, ThreadId, ToolCallPatch, ToolCallStatus, ToolKind,
};

use super::super::Held;
use super::{CAP_BYTES, new_pile, trim, weight, words};

fn session() -> AgentSessionId {
    AgentSessionId::new("sess-1").unwrap()
}

fn thread() -> ThreadId {
    ThreadId::new(uuid::Uuid::now_v7()).unwrap()
}

/// Holds `event` as `hold_event` does: a new pile when nothing is held, then its weight, then
/// the trim.
fn hold(held: &mut Held, thread: ThreadId, event: AcpEvent) {
    if held.updates.is_empty() {
        new_pile(held);
    }
    held.bytes += weight(&event);
    held.updates.push(event);
    trim(held, thread);
}

/// Words `n`, an eighth of the cap.
fn text(n: usize) -> AgentUpdate {
    let text = format!("{n}:{}", "x".repeat(CAP_BYTES / 8));
    AgentUpdate::MessageChunk {
        role: MessageRole::Answer,
        acp_message_id: None,
        content: ContentBlock::Text { text },
    }
}

/// Chunk `n` of a helper's words.
fn chunk(n: usize) -> AcpEvent {
    AcpEvent::HelperUpdate {
        session: session(),
        origin: Origin::Idle,
        helper: "toolu_A".into(),
        update: text(n),
    }
}

/// Chunk `n` of the lead's words.
fn lead_chunk(n: usize) -> AcpEvent {
    update(text(n))
}

fn update(update: AgentUpdate) -> AcpEvent {
    AcpEvent::Update {
        session: session(),
        origin: Origin::Idle,
        update,
    }
}

fn step(acp_id: &str, tool: &str) -> AgentUpdate {
    AgentUpdate::ToolCall(ToolCall {
        acp_id: acp_id.into(),
        title: tool.into(),
        kind: ToolKind::Other,
        tool_name: Some(tool.into()),
        status: ToolCallStatus::Pending,
        locations: Vec::new(),
        raw_input: None,
        raw_output: None,
        content: Vec::new(),
    })
}

/// A name for one held event: a chunk's number, a step's or step update's ACP id, `usage`, or
/// `other`.
fn name(event: &AcpEvent) -> String {
    if let Some(text) = words(event) {
        return text.split(':').next().unwrap_or("?").to_owned();
    }
    let (AcpEvent::Update { update, .. } | AcpEvent::HelperUpdate { update, .. }) = event else {
        return "other".to_owned();
    };
    if let AgentUpdate::ToolCall(call) = update {
        format!("step {}", call.acp_id)
    } else if let AgentUpdate::ToolCallUpdate { acp_id, .. } = update {
        format!("update {acp_id}")
    } else if let AgentUpdate::Usage { .. } = update {
        "usage".to_owned()
    } else {
        "other".to_owned()
    }
}

/// The name of each held event, in order.
fn names(held: &Held) -> Vec<String> {
    held.updates.iter().map(name).collect()
}

/// Words past the cap, with a launch, its input, a step under a card, the card, a handback
/// and its update, and the costed usage among them, all older than most words: only the oldest
/// words go, the lead's and the helper's alike; every other event stays, in order.
#[test]
fn past_the_cap_only_the_oldest_words_go() {
    let thread = thread();
    let mut held = Held::default();
    let input = ToolCallPatch {
        raw_input: Some(clauseo_protocol::TextOrBlob::Inline(
            r#"{"subagent_type":"paper-reader","prompt":"Read."}"#.to_owned(),
        )),
        ..ToolCallPatch::default()
    };
    let landed = ToolCallPatch {
        status: Some(ToolCallStatus::Completed),
        ..ToolCallPatch::default()
    };
    let cost = AgentUpdate::Usage {
        used: Count::from(1u32),
        size: Count::from(10u32),
        cost: Some(Cost {
            amount: clauseo_protocol::Finite::new(0.01).unwrap(),
            currency: "USD".into(),
        }),
    };
    let card = AcpEvent::ElicitationCompleted {
        elicitation_id: "el-1".into(),
    };
    let events = [
        lead_chunk(0),
        update(step("toolu_A", "Agent")),
        chunk(1),
        update(AgentUpdate::ToolCallUpdate {
            acp_id: "toolu_A".into(),
            patch: input,
        }),
        chunk(2),
        update(step("tc-R", "Read")),
        card,
        chunk(3),
        AcpEvent::HelperUpdate {
            session: session(),
            origin: Origin::Idle,
            helper: "toolu_A".into(),
            update: step("toolu_H", "SubagentHandback"),
        },
        update(AgentUpdate::ToolCallUpdate {
            acp_id: "toolu_H".into(),
            patch: landed,
        }),
        chunk(4),
        update(cost),
    ];
    for event in events {
        hold(&mut held, thread, event);
    }
    for n in 5..12 {
        hold(&mut held, thread, chunk(n));
    }
    assert_eq!(
        names(&held),
        [
            "step toolu_A",
            "update toolu_A",
            "step tc-R",
            "other",
            "step toolu_H",
            "update toolu_H",
            "usage",
            "6",
            "7",
            "8",
            "9",
            "10",
            "11",
        ]
    );
    assert!(held.bytes <= CAP_BYTES, "{}", held.bytes);
}

/// The log lines a test captures.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// One pile-up warns once: through two trims that drop words, and through holds over the cap
/// once only events that are not words are left above it. A new pile-up warns again.
#[test]
fn one_pile_up_warns_once() {
    let captured = Captured::default();
    let sink = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || sink.clone())
        .with_max_level(tracing::Level::WARN)
        .finish();
    let thread = thread();
    let mut held = Held::default();
    tracing::subscriber::with_default(subscriber, || {
        hold(&mut held, thread, update(step("tc-first", "Read")));
        for n in 0..20 {
            hold(&mut held, thread, chunk(n));
        }
        // Over the cap with only events that are not words left: nothing goes, nothing is said.
        held.updates.retain(|event| words(event).is_none());
        held.bytes = CAP_BYTES;
        for n in 0..8 {
            hold(&mut held, thread, update(step(&format!("tc-{n}"), "Read")));
        }
        assert_eq!(held.updates.len(), 9, "a step was dropped");
        // The pile goes to a turn; the next one starts over.
        held.updates.clear();
        for n in 0..9 {
            hold(&mut held, thread, chunk(n));
        }
    });
    let text = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    let warnings = text.matches("its oldest words were dropped").count();
    assert_eq!(warnings, 2, "{text}");
}
