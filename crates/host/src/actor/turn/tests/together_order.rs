//! Whether two turns of one matter ran beside each other follows the order of their checkpoints
//! on the matter's thread, not the turns' phases: a turn whose after-turn checkpoint was sent
//! before another's before-turn checkpoint ran before it, even if it ends after that one starts.
//! The later turn's card is as it always was, and a lawyer edit to the earlier turn's paper
//! between the two checkpoints is in the later turn's outside edits. A turn whose after-turn
//! checkpoint fails leaves its papers on the card of the turn that ran beside it, flagged.

use clauseo_protocol::{
    CommandOutcome, DomainEvent, FilesChanged, HostCommand, PromptBlock, ThreadId, TurnId,
};

use super::rig::{Rig, kind};
use crate::actor::input::{Input, MatterTag};
use crate::actor::turn::inputs::{AgentReply, TurnInput};
use crate::actor::turn::{JobKind, TurnJob, TurnPhase};
use crate::error::Error;
use crate::matter_worker::MatterReply;

/// A prompt "Write A." writes notes/a.txt, "Write B." notes/b.txt, each beside a completed edit
/// tool call naming it; each turn then ends. A new agent plays it from the start.
pub(super) const WRITES: &str = r#"{"one_of":[[{"expect":{"method":"session/prompt","as":"pa","params":{"sessionId":"sess-1","prompt":[{"type":"text","text":"Write A."}]}}},{"write_file":{"path":"notes/a.txt","content":"Alpha\n"}},{"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"t-a","title":"Write notes/a.txt","kind":"edit","status":"completed","locations":[{"path":"{{cwd}}/notes/a.txt"}]}}},{"respond":{"to":"pa","result":{"stopReason":"end_turn"}}}],[{"expect":{"method":"session/prompt","as":"pb","params":{"sessionId":"sess-1","prompt":[{"type":"text","text":"Write B."}]}}},{"write_file":{"path":"notes/b.txt","content":"Beta\n"}},{"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"t-b","title":"Write notes/b.txt","kind":"edit","status":"completed","locations":[{"path":"{{cwd}}/notes/b.txt"}]}}},{"respond":{"to":"pb","result":{"stopReason":"end_turn"}}}]]}
"#;

pub(super) fn send_prompt(rig: &mut Rig, n: u128, thread: ThreadId, text: &str) -> TurnId {
    let command = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: text.to_owned(),
        }],
        delivery: clauseo_protocol::DeliveryRequest::Queue,
        config: None,
        agent: None,
    };
    let accepted = rig.dispatch(n, command).unwrap();
    let CommandOutcome::PromptAccepted { turn, .. } = accepted else {
        panic!("not accepted: {accepted:?}");
    };
    turn
}

/// Each change of `turn`'s card in `log`: its path, its flag, and the other threads it names.
pub(super) fn card(log: &[DomainEvent], turn: TurnId) -> Vec<(String, bool, Vec<ThreadId>)> {
    let changed: Option<&FilesChanged> = log.iter().find_map(|event| {
        if let DomainEvent::FilesChanged(changed) = event
            && changed.turn == turn
        {
            return Some(changed);
        }
        None
    });
    let kinds = log.iter().map(kind).collect::<Vec<_>>();
    let changed = changed.unwrap_or_else(|| panic!("no FilesChanged: {kinds:?}"));
    changed
        .files
        .iter()
        .map(|file| {
            let path = file.path().as_str().to_owned();
            (
                path,
                file.may_include_lawyer_edits(),
                file.also_changed_by().to_vec(),
            )
        })
        .collect()
}

#[test]
fn a_turn_whose_after_turn_checkpoint_was_sent_first_ran_before_the_next() {
    let mut rig = Rig::start("together-after-sent", WRITES);
    let (first, second) = (rig.thread(), rig.thread_by(2));
    // A's whole turn runs; the reply to its after-turn checkpoint is held, so A has not ended.
    let turn_a = send_prompt(&mut rig, 10, first, "Write A.");
    rig.hold_next("A's after-turn checkpoint", |_, input| {
        matches!(
            input,
            Input::Matter {
                tag: MatterTag::Turn(TurnJob {
                    kind: JobKind::After(_),
                    ..
                }),
                ..
            }
        )
    });
    // The lawyer edits A's paper after A's after-turn checkpoint.
    rig.write_file("notes/a.txt", "Alpha, and the lawyer's line\n");

    // B passes step 3 and sends its before-turn checkpoint while A still runs.
    let turn_b = send_prompt(&mut rig, 11, second, "Write B.");
    let matter = rig.matter();
    rig.run_until("B's before-turn checkpoint sent", |actor| {
        let phase = actor.threads[&second]
            .turn
            .current
            .as_ref()
            .map(|c| c.phase);
        phase.is_some_and(|phase| phase != TurnPhase::Launching && phase != TurnPhase::Accepting)
    });
    assert_eq!(rig.actor.matters[&matter].turns.running.len(), 2);

    // A ends now, then B.
    rig.release_held();
    rig.run_until("both turns ended", |actor| {
        [first, second].iter().all(|thread| {
            let state = &actor.threads[thread].turn;
            state.current.is_none() && state.closing.is_none()
        }) && actor.work.commits == 0
            && actor.work.matter_jobs == 0
    });

    // A's card holds its note as its checkpoint found it; B's is as it always was.
    let (log_a, log_b) = (rig.log(first), rig.log(second));
    let a_txt = ("notes/a.txt".to_owned(), false, vec![]);
    assert_eq!(card(&log_a, turn_a), [a_txt]);
    let b_txt = ("notes/b.txt".to_owned(), false, vec![]);
    assert_eq!(card(&log_b, turn_b), [b_txt]);
    // The lawyer's edit is B's before-turn checkpoint's outside edit, not left out as A's.
    let outside: Vec<Vec<String>> = rig
        .matter_log()
        .iter()
        .filter_map(|event| {
            let DomainEvent::OutsideEdits(edits) = event else {
                return None;
            };
            Some(
                edits
                    .files
                    .iter()
                    .map(|f| f.path().as_str().to_owned())
                    .collect(),
            )
        })
        .collect();
    assert_eq!(outside, [["notes/a.txt".to_owned()]]);
    rig.finish();
}

/// Whether `input` is the answer to `thread`'s prompt.
pub(super) fn prompt_answer_of(thread: ThreadId, input: &Input) -> bool {
    matches!(
        input,
        Input::Turn(TurnInput::Agent {
            thread: of,
            reply: AgentReply::Prompt { .. },
            ..
        }) if *of == thread
    )
}

#[test]
fn a_failed_answers_paper_stays_on_the_card_of_the_answer_beside_it() {
    let mut rig = Rig::start("together-failed", WRITES);
    let (first, second) = (rig.thread(), rig.thread_by(2));
    // B writes notes/b.txt; the answer to its prompt is held, so B runs on.
    let turn_b = send_prompt(&mut rig, 10, second, "Write B.");
    rig.hold_next("B's prompt answer", |_, input| {
        prompt_answer_of(second, input)
    });
    // A starts beside B, writes notes/a.txt and ends; its after-turn checkpoint fails.
    let turn_a = send_prompt(&mut rig, 11, first, "Write A.");
    rig.hold_next("A's after-turn checkpoint", |_, input| {
        matches!(
            input,
            Input::Matter {
                tag: MatterTag::Turn(TurnJob {
                    kind: JobKind::After(_),
                    ..
                }),
                ..
            }
        )
    });
    let Some(Input::Matter { matter, tag, .. }) = rig.held.pop() else {
        panic!("A's after-turn checkpoint is not held");
    };
    let failed = Err(Error::Internal("a test fails this checkpoint".to_owned()));
    rig.actor.on_input(Input::Matter {
        matter,
        tag,
        reply: MatterReply::AfterTurn(failed),
    });
    rig.run_until("A ended", |actor| {
        let state = &actor.threads[&first].turn;
        state.current.is_none() && state.closing.is_none() && actor.work.commits == 0
    });

    // B ends: A's note, on no card of A's, is on B's, flagged, naming A.
    rig.release_held();
    rig.run_until("B ended", |actor| {
        let state = &actor.threads[&second].turn;
        state.current.is_none()
            && state.closing.is_none()
            && actor.work.commits == 0
            && actor.work.matter_jobs == 0
    });
    let log_a = rig.log(first);
    let no_card = !log_a
        .iter()
        .any(|event| matches!(event, DomainEvent::FilesChanged(changed) if changed.turn == turn_a));
    assert!(no_card, "A has a card");
    let log_b = rig.log(second);
    let a_txt = ("notes/a.txt".to_owned(), true, vec![first]);
    let b_txt = ("notes/b.txt".to_owned(), false, vec![]);
    assert_eq!(card(&log_b, turn_b), [a_txt, b_txt]);
    rig.finish();
}
