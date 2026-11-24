//! The start-up repair of an answer left open beside others: it is measured from its own
//! before-turn checkpoint, and the papers the other open answers wrote, those on cards logged
//! since, and those logged as outside edits since are left off its card, so each paper is on
//! one card or logged as an outside edit, never on none and never on the wrong one.

use clauseo_protocol::{DomainEvent, FilesChanged, MatterPath, ThreadId, TurnId};

use super::rig::Rig;
use super::together_order::{card, prompt_answer_of, send_prompt};
use super::together_race::{ASK_THEN_A, answer, until_asked, until_ended};
use crate::actor::Actor;
use crate::actor::input::Input;
use crate::actor::turn::{TurnPhase, steps};

/// The `FilesChanged` of `turn` in `log`.
fn changed(log: &[DomainEvent], turn: TurnId) -> Option<FilesChanged> {
    log.iter().find_map(|event| {
        let DomainEvent::FilesChanged(changed) = event else {
            return None;
        };
        (changed.turn == turn).then(|| changed.clone())
    })
}

/// The papers `turn`'s card left off as other answers'.
fn beside(log: &[DomainEvent], turn: TurnId) -> Vec<String> {
    let beside = changed(log, turn)
        .map(|changed| changed.beside)
        .unwrap_or_default();
    beside.iter().map(|path| path.as_str().to_owned()).collect()
}

/// Whether `turn` has a `TurnEnded` in `log`.
fn has_ended(log: &[DomainEvent], turn: TurnId) -> bool {
    log.iter()
        .any(|event| matches!(event, DomainEvent::TurnEnded(ended) if ended.turn == turn))
}

/// Every paper the matter's log holds as an outside edit.
fn outside_papers(rig: &mut Rig) -> Vec<String> {
    let log = rig.matter_log();
    let edits = log.iter().filter_map(|event| {
        let DomainEvent::OutsideEdits(edits) = event else {
            return None;
        };
        Some(edits.files.clone())
    });
    let files = edits.flatten();
    files.map(|file| file.path().as_str().to_owned()).collect()
}

/// The host stops with `threads`' turns open: nothing ends them.
fn crash(mut rig: Rig, threads: &[ThreadId]) -> Rig {
    for thread in threads {
        rig.actor.threads.get_mut(thread).unwrap().turn.current = None;
    }
    // What was held finds no turn to answer.
    rig.release_held();
    rig.restart(1)
}

#[test]
fn an_answer_left_open_after_one_beside_it_ended_keeps_its_papers_on_its_card() {
    let mut rig = Rig::start("together-repair-ended", ASK_THEN_A);
    let (first, second) = (rig.thread(), rig.thread_by(2));
    // A starts and waits on the lawyer; C starts beside it and writes notes/b.txt, running on.
    let turn_a = send_prompt(&mut rig, 10, first, "Ask, then write A.");
    let request = until_asked(&mut rig, first);
    let turn_c = send_prompt(&mut rig, 11, second, "Write B.");
    rig.hold_next("C's prompt answer", |_, input| {
        prompt_answer_of(second, input)
    });
    // A writes notes/a.txt; its prompt's answer is held, so A runs on.
    answer(&mut rig, 12, first, request, "allow-once");
    rig.hold_next("A's prompt answer", |_, input| {
        prompt_answer_of(first, input)
    });
    // C ends: its card leaves A's paper out.
    let at = rig
        .held
        .iter()
        .position(|input| prompt_answer_of(second, input));
    let c_answer = rig.held.remove(at.expect("C's answer is held"));
    rig.actor.on_input(c_answer);
    until_ended(&mut rig, &[second]);
    let log_c = rig.log(second);
    assert_eq!(
        card(&log_c, turn_c),
        [("notes/b.txt".to_owned(), false, vec![])]
    );
    assert_eq!(beside(&log_c, turn_c), ["notes/a.txt"]);

    // The host stops with A open; the next start repairs it.
    let mut rig = crash(rig, &[first]);
    let log_a = rig.log(first);
    assert!(has_ended(&log_a, turn_a), "A's turn is repaired");
    assert_eq!(
        card(&log_a, turn_a),
        [("notes/a.txt".to_owned(), false, vec![])]
    );
    assert_eq!(beside(&log_a, turn_a), ["notes/b.txt"]);
    assert!(!outside_papers(&mut rig).contains(&"notes/a.txt".to_owned()));
    rig.finish();
}

#[test]
fn a_repair_from_its_own_checkpoint_leaves_the_other_open_answers_papers_off() {
    let mut rig = Rig::start("together-repair-own", ASK_THEN_A);
    let (first, second) = (rig.thread(), rig.thread_by(2));
    // A starts and waits on the lawyer; B starts beside it and writes notes/b.txt, running on.
    let turn_a = send_prompt(&mut rig, 10, first, "Ask, then write A.");
    let request = until_asked(&mut rig, first);
    let turn_b = send_prompt(&mut rig, 11, second, "Write B.");
    rig.hold_next("B's prompt answer", |_, input| {
        prompt_answer_of(second, input)
    });
    // A writes notes/a.txt and ends: R8 takes its after-turn checkpoint (holding b.txt too),
    // and both commits of its end fail, so the log never records it.
    rig.fail(2, |event| matches!(event, DomainEvent::TurnEnded(_)));
    answer(&mut rig, 12, first, request, "allow-once");
    rig.run_until("A's end gave up", |actor: &mut Actor| {
        actor.threads[&first].turn.current.is_none() && actor.work.commits == 0
    });
    assert!(!has_ended(&rig.log(first), turn_a), "A's turn is open");

    // The host stops with B open too: A's repair takes its own checkpoint, without b.txt.
    let mut rig = crash(rig, &[second]);
    let (log_a, log_b) = (rig.log(first), rig.log(second));
    assert!(has_ended(&log_a, turn_a) && has_ended(&log_b, turn_b));
    // Its own checkpoint's changes are flagged, no tool call being known for them.
    assert_eq!(
        card(&log_a, turn_a),
        [("notes/a.txt".to_owned(), true, vec![])]
    );
    assert_eq!(beside(&log_a, turn_a), ["notes/b.txt"]);
    assert_eq!(
        card(&log_b, turn_b),
        [("notes/b.txt".to_owned(), false, vec![])]
    );
    assert_eq!(beside(&log_b, turn_b), ["notes/a.txt"]);
    rig.finish();
}

#[test]
fn an_old_log_with_two_open_answers_puts_no_outside_edit_on_the_earlier_card() {
    let mut rig = Rig::start("together-repair-old-log", "");
    let (a, c) = (rig.thread(), rig.thread_by(2));
    // A starts; notes/a.txt changes with no tool call naming it.
    let a_turn = rig.running_turn(a);
    rig.write_file("a.txt", "Alpha");
    // C's before-turn checkpoint logs it as an outside edit, as every log before answers ran
    // together did; the host stops as C's start is logged.
    let c_turn = rig.current_turn(c, TurnPhase::Queued);
    let matter = rig.matter();
    let turns = &mut rig.actor.matters.get_mut(&matter).unwrap().turns;
    turns.running.insert(a);
    turns.running.insert(c);
    steps::before_turn(&mut rig.actor, c, c_turn);
    let started = |actor: &Actor, input: &Input| {
        matches!(input, Input::Committed { ticket, .. }
            if actor.pipeline.events(*ticket).iter()
                .any(|e| matches!(e.event, DomainEvent::TurnStarted(_))))
    };
    rig.hold_next("C's TurnStarted commit", started);
    assert_eq!(outside_papers(&mut rig), ["a.txt"]);

    let mut rig = crash(rig, &[a, c]);
    let log_a = rig.log(a);
    assert!(has_ended(&log_a, a_turn) && has_ended(&rig.log(c), c_turn));
    // a.txt is an outside edit still, and not on A's card again.
    let outside = outside_papers(&mut rig);
    assert_eq!(outside, ["a.txt"]);
    let on_card: Vec<MatterPath> = changed(&log_a, a_turn)
        .map(|changed| changed.files.iter().map(|f| f.path().clone()).collect())
        .unwrap_or_default();
    assert!(
        on_card
            .iter()
            .all(|path| !outside.contains(&path.as_str().to_owned())),
        "{on_card:?}"
    );
    assert_eq!(beside(&log_a, a_turn), ["a.txt"]);
    rig.finish();
}
