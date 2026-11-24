//! The step `UndoStep` and `PreviewUndo` read from a thread's log: the answer's card is its
//! `only`, change by change, with the papers it left off as other answers' (its `beside`),
//! taken only when some `beside` names a paper.

use clauseo_protocol::{
    ChangeKind, CheckpointId, ContentId, Count, DomainEvent, Envelope, EventId, FileChange,
    FilesChanged, MatterPath, Seq, StopReason, TurnEnded, TurnId, TurnStarted,
};

use super::step_of;
use crate::matter_worker::TurnCard;
use crate::pipeline::tests::support::{id, thread_stream, ts};

fn path(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}

fn content(hex: &str) -> ContentId {
    ContentId::new(hex).unwrap()
}

/// A turn's log: started at checkpoint 5, ended at checkpoint 6, with `files` as its
/// `FilesChanged` when given.
fn log(turn: TurnId, files: Option<Vec<FileChange>>) -> Vec<Envelope> {
    log_beside(turn, files, Vec::new())
}

/// [`log`], the `FilesChanged` naming `beside` as other answers' papers.
fn log_beside(
    turn: TurnId,
    files: Option<Vec<FileChange>>,
    beside: Vec<MatterPath>,
) -> Vec<Envelope> {
    let mut events = vec![DomainEvent::TurnStarted(TurnStarted {
        turn,
        checkpoint_before: id::<CheckpointId>(5),
    })];
    if let Some(files) = files {
        events.push(DomainEvent::FilesChanged(FilesChanged {
            turn,
            files,
            skipped: Vec::new(),
            beside,
        }));
    }
    events.push(DomainEvent::TurnEnded(TurnEnded {
        turn,
        stop: StopReason::EndTurn,
        usage: None,
        checkpoint_after: Some(id::<CheckpointId>(6)),
    }));
    events
        .into_iter()
        .zip(1_u64..)
        .map(|(event, n)| {
            let seq = Seq::new(n).unwrap();
            let at = ts(i64::try_from(n).unwrap());
            Envelope::new(id::<EventId>(n), thread_stream(), seq, at, None, event)
        })
        .collect()
}

#[test]
fn the_card_in_the_log_is_the_steps_only() {
    let turn = id::<TurnId>(3);
    let size = Count::new(3).unwrap();
    let files = vec![
        FileChange::new(
            path("a.docx"),
            ChangeKind::Added,
            None,
            Some(content("a1")),
            size,
        )
        .unwrap(),
        FileChange::new(
            path("b.docx"),
            ChangeKind::Renamed {
                from: path("old.docx"),
            },
            Some(content("b1")),
            Some(content("b1")),
            size,
        )
        .unwrap(),
    ];
    let beside = vec![path("notes/c.txt")];
    let step = step_of(&log_beside(turn, Some(files.clone()), beside.clone()), turn).unwrap();
    assert_eq!(step.before, id::<CheckpointId>(5));
    assert_eq!(step.after, id::<CheckpointId>(6));
    let card = TurnCard {
        changes: files,
        others: beside.into_iter().collect(),
    };
    assert_eq!(step.only, Some(card));
}

#[test]
fn the_papers_left_off_the_card_as_other_answers_are_its_others() {
    let turn = id::<TurnId>(3);
    let beside = vec![path("notes/b.txt"), path("notes/c.txt")];
    let step = step_of(&log_beside(turn, Some(Vec::new()), beside.clone()), turn).unwrap();
    let card = TurnCard {
        changes: Vec::new(),
        others: beside.into_iter().collect(),
    };
    assert_eq!(step.only, Some(card));
}

#[test]
fn a_log_without_the_card_undoes_the_whole_step() {
    let turn = id::<TurnId>(3);
    let step = step_of(&log(turn, None), turn).unwrap();
    assert_eq!(step.after, id::<CheckpointId>(6));
    assert_eq!(step.only, None);
}

/// The reviewer's case: a file grown over the cap (skipped) and its old bytes copied to a new
/// path, nothing `beside`: the answer ran alone, so the undo takes no card and is today's.
#[test]
fn an_answer_with_nothing_beside_takes_no_card() {
    let turn = id::<TurnId>(3);
    let copy = FileChange::new(
        path("copy.docx"),
        ChangeKind::Added,
        None,
        Some(content("c1")),
        Count::new(5).unwrap(),
    )
    .unwrap();
    let step = step_of(&log(turn, Some(vec![copy])), turn).unwrap();
    assert_eq!(step.paths, [path("copy.docx")]);
    assert!(step.only.is_none());
}
