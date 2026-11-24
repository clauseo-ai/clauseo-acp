//! A turn's own picks that could not be used get their notice (`prompt_picks/send.rs`): picks
//! to set with no session attached at step 4, and picks answered by an agent since replaced.

use std::collections::BTreeMap;

use clauseo_protocol::{CompactString, Notice, NoticeKind, ThreadId, TurnId};

use super::rig::Rig;
use crate::actor::turn::{TurnPhase, prompt_picks};

/// The notice `turn`'s `TurnStarted` would be followed by.
fn notice(rig: &mut Rig, thread: ThreadId, turn: TurnId) -> Option<Notice> {
    let (_, _, notice) = prompt_picks::started_with(&mut rig.actor, thread, turn, Vec::new());
    notice
}

fn is_not_used(notice: Option<&Notice>) -> bool {
    matches!(
        notice.and_then(|notice| notice.kind.as_ref()),
        Some(NoticeKind::PickNotUsed { .. })
    )
}

#[test]
fn picks_with_no_session_attached_at_step_4_are_told() {
    let mut rig = Rig::start("prompt-picks-unattached", "");
    let thread = rig.thread();
    let turn = rig.current_turn(thread, TurnPhase::Queued);
    let picks: BTreeMap<CompactString, CompactString> =
        [("model".into(), "opus".into())].into_iter().collect();
    prompt_picks::restored(&mut rig.actor, thread, turn, &picks);
    assert!(
        !prompt_picks::send_first(&mut rig.actor, thread, turn),
        "no session takes them"
    );
    let told = notice(&mut rig, thread, turn);
    assert!(is_not_used(told.as_ref()), "{told:?}");
    rig.finish();
}

#[test]
fn picks_answered_by_an_agent_since_replaced_are_told() {
    let mut rig = Rig::start("prompt-picks-replaced", "");
    let thread = rig.thread();
    let turn = rig.current_turn(thread, TurnPhase::Prompting);
    prompt_picks::sets_out(&mut rig.actor, thread, turn);
    let picked = prompt_picks::Picked::answered(Vec::new());
    prompt_picks::on_picked(&mut rig.actor, thread, turn, false, picked);
    let told = notice(&mut rig, thread, turn);
    assert!(is_not_used(told.as_ref()), "{told:?}");
    rig.finish();
}
