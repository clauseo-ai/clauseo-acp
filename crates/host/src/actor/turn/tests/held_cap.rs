//! The cap on agent output held for a turn (`own_turn/cap.rs`): while a turn of the thread is on
//! its way, so no agent-started turn opens, a background helper's words pile up; past the cap
//! the oldest go and a card the agent waits on stays.

use clauseo_acp::{AcpEvent, AgentUpdate, MessageRole, Origin};
use clauseo_protocol::{AgentSessionId, ContentBlock};

use super::rig::Rig;
use crate::actor::turn::{TurnPhase, own_turn};

/// A helper's words, as they wait with no turn live.
fn words(text: String) -> AcpEvent {
    AcpEvent::HelperUpdate {
        session: AgentSessionId::new("sess-1").unwrap(),
        origin: Origin::Idle,
        helper: "toolu_A".into(),
        update: AgentUpdate::MessageChunk {
            role: MessageRole::Answer,
            acp_message_id: None,
            content: ContentBlock::Text { text },
        },
    }
}

/// The text of held words, its number for a chunk this test made.
fn number(event: &AcpEvent) -> Option<usize> {
    let AcpEvent::HelperUpdate {
        update:
            AgentUpdate::MessageChunk {
                content: ContentBlock::Text { text },
                ..
            },
        ..
    } = event
    else {
        return None;
    };
    text.split(':').next()?.parse().ok()
}

/// Twelve chunks of half a MiB each (three halves of the cap) wait behind a card: the oldest
/// chunks go, the newest stay in order and within the cap, and the card stays first.
#[test]
fn held_output_past_its_cap_drops_its_oldest_updates_and_keeps_its_cards() {
    let mut rig = Rig::start("held-cap", "");
    let thread = rig.thread();
    rig.current_turn(thread, TurnPhase::Starting);
    let card = AcpEvent::ElicitationCompleted {
        elicitation_id: "el-1".into(),
    };
    own_turn::hold_helper(&mut rig.actor, thread, card.clone());
    let half = "x".repeat(own_turn::HELD_CAP_BYTES / 8);
    for n in 0..12 {
        own_turn::hold_helper(&mut rig.actor, thread, words(format!("{n}:{half}")));
    }
    let held = own_turn::held_events(&mut rig.actor, thread);
    assert_eq!(held.first(), Some(&card));
    let kept: Vec<usize> = held.iter().filter_map(number).collect();
    assert_eq!(kept.len(), held.len() - 1, "every other event is a chunk");
    assert!(kept.len() < 12, "nothing was dropped: {kept:?}");
    let newest: Vec<usize> = (12 - kept.len()..12).collect();
    assert_eq!(kept, newest, "the newest stay, in order");
    assert!(kept.len() * half.len() <= own_turn::HELD_CAP_BYTES);
    // The turn was put in by hand; nothing would end it.
    rig.actor.threads.get_mut(&thread).unwrap().turn.current = None;
    rig.finish();
}
