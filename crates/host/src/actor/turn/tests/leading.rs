//! The display batch's leading flush on the actor, with the manual clock: the first text after
//! a quiet window is shown when it arrives, text inside the window is held, and the turn's end
//! shows what is held before it commits.

use std::time::Duration;

use clauseo_protocol::{DomainEvent, TextKind};

use super::rig::Rig;
use crate::actor::turn::{Then, out::Out, text};
use crate::coalesce::TextIn;

#[test]
fn first_text_after_a_quiet_window_is_shown_without_a_timer() {
    let mut rig = Rig::start("turn-leading-text", "");
    let thread = rig.thread();
    let turn = rig.running_turn(thread);
    let message = rig.actor.ids.mint(rig.actor.now().wall).unwrap();
    rig.clock.advance(Duration::from_millis(100));

    let delta = TextIn {
        turn,
        message,
        acp_message_id: None,
        kind: TextKind::Answer,
        text: "The lease ".to_owned(),
    };
    text::on_delta(&mut rig.actor, thread, delta).unwrap();
    // The clock has not moved since: the text went to the view at once, not at a timer.
    assert!(
        !rig.actor.threads[&thread].turn.text.has_display(),
        "the first text waited for a timer"
    );
    rig.finish();
}

#[test]
fn text_inside_the_window_is_held_and_the_turn_end_shows_it_first() {
    let mut rig = Rig::start("turn-leading-held", "");
    let thread = rig.thread();
    let turn = rig.running_turn(thread);
    let message = rig.actor.ids.mint(rig.actor.now().wall).unwrap();
    rig.clock.advance(Duration::from_millis(100));
    let delta = |text: &str| TextIn {
        turn,
        message,
        acp_message_id: None,
        kind: TextKind::Answer,
        text: text.to_owned(),
    };
    text::on_delta(&mut rig.actor, thread, delta("The lease ")).unwrap();

    // 4 ms later, inside the window: held for the 16 ms timer.
    rig.clock.advance(Duration::from_millis(4));
    text::on_delta(&mut rig.actor, thread, delta("ends ")).unwrap();
    assert!(
        rig.actor.threads[&thread].turn.text.has_display(),
        "text inside the window was shown early"
    );

    // The turn's end commits through `Out`, which shows the held text before its events.
    Out::new(&mut rig.actor, thread, None)
        .unwrap()
        .send(&mut rig.actor, Then::Nothing)
        .unwrap();
    rig.settle_commits();
    assert!(!rig.actor.threads[&thread].turn.text.has_display());
    let log = rig.log(thread);
    let answers: Vec<&str> = log
        .iter()
        .filter_map(|event| {
            if let DomainEvent::TextAppended(appended) = event
                && appended.message == message
            {
                Some(appended.text.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(answers, ["The lease ends "]);
    rig.finish();
}

#[test]
fn a_burst_in_the_window_is_flushed_once_at_the_window_end() {
    let mut rig = Rig::start("turn-leading-burst", "");
    let thread = rig.thread();
    let turn = rig.running_turn(thread);
    let message = rig.actor.ids.mint(rig.actor.now().wall).unwrap();
    rig.clock.advance(Duration::from_millis(100));
    let delta = |text: &str| TextIn {
        turn,
        message,
        acp_message_id: None,
        kind: TextKind::Answer,
        text: text.to_owned(),
    };
    let held = |rig: &Rig| rig.actor.threads[&thread].turn.text.has_display();

    // The leading delta is shown at once. The deltas after it, inside the window, are held for
    // the one timer, which is due at the window's end: 100 ms + 16 ms = 116 ms.
    text::on_delta(&mut rig.actor, thread, delta("The lease ")).unwrap();
    assert!(!held(&rig));
    for (advance, text) in [(4, "ends "), (4, "in "), (3, "May.")] {
        rig.clock.advance(Duration::from_millis(advance));
        text::on_delta(&mut rig.actor, thread, delta(text)).unwrap();
        assert!(held(&rig), "{text:?} was shown before the window's end");
    }
    // At 115 ms the window is still open, so the timer must not have flushed anything yet.
    rig.clock.advance(Duration::from_millis(4));
    assert_eq!(rig.actor.now().mono, Duration::from_millis(115));
    assert!(held(&rig), "flushed before the window's end");
    // At 116 ms the timer fires and shows the whole burst in one flush.
    rig.clock.advance(Duration::from_millis(1));
    rig.run_until("the trailing display flush", |actor| {
        !actor.threads[&thread].turn.text.has_display()
    });
    assert_eq!(rig.actor.now().mono, Duration::from_millis(116));
    rig.finish();
}
