use std::str::FromStr;

use clauseo_protocol::{
    CheckpointId, CheckpointKind, MatterPath, SkippedFile, SkippedFileReason, ThreadId, TurnId,
};

use super::{first_reads, message, parse};
use crate::first_read::FirstReads;
use crate::types::TimelineEntry;

const ID: &str = "0190a6c8-6f8e-7c3a-8b1e-2f4d5a6b7c8d";
const THREAD: &str = "0190a6c8-6f8e-7c3a-8b1e-2f4d5a6b7c8e";
const TURN: &str = "0190a6c8-6f8e-7c3a-8b1e-2f4d5a6b7c8f";

fn entry(kind: CheckpointKind, with_turn: bool) -> TimelineEntry {
    TimelineEntry {
        id: CheckpointId::from_str(ID).unwrap(),
        kind,
        thread: with_turn.then(|| ThreadId::from_str(THREAD).unwrap()),
        turn: with_turn.then(|| TurnId::from_str(TURN).unwrap()),
    }
}

#[test]
fn an_after_turn_message_is_a_summary_then_four_trailers_in_order() {
    let text = message(
        &entry(CheckpointKind::AfterTurn, true),
        3,
        &FirstReads::default(),
    );
    let expected = format!(
        "After turn: 3 files changed\n\nClauseo-Checkpoint: {ID}\nClauseo-Kind: after-turn\n\
         Clauseo-Thread: {THREAD}\nClauseo-Turn: {TURN}\n"
    );
    assert_eq!(text, expected);
}

#[test]
fn an_import_message_counts_files_and_has_no_thread_or_turn() {
    let text = message(
        &entry(CheckpointKind::Import, false),
        1,
        &FirstReads::default(),
    );
    assert_eq!(
        text,
        format!("Import: 1 file\n\nClauseo-Checkpoint: {ID}\nClauseo-Kind: import\n")
    );
}

#[test]
fn every_kind_round_trips_through_its_message() {
    for kind in [
        CheckpointKind::Import,
        CheckpointKind::BeforeTurn,
        CheckpointKind::AfterTurn,
        CheckpointKind::OutsideEdits,
        CheckpointKind::Undo,
        CheckpointKind::Restore,
        CheckpointKind::Revisions,
    ] {
        for with_turn in [false, true] {
            let written = entry(kind, with_turn);
            assert_eq!(
                parse(&message(&written, 2, &FirstReads::default())),
                Some(written),
                "{kind:?}"
            );
        }
    }
}

#[test]
fn trailers_read_in_any_order_with_unknown_keys_and_kinds() {
    let text = format!(
        "Something newer\n\nbody\n\nClauseo-Future: x\nClauseo-Kind: squash\n\
         Clauseo-Turn: {TURN}\nClauseo-Checkpoint: {ID}\n"
    );
    let read = parse(&text).unwrap();
    assert_eq!(read.id, CheckpointId::from_str(ID).unwrap());
    assert_eq!(read.kind, CheckpointKind::Unknown);
    assert_eq!(read.thread, None);
    assert_eq!(read.turn, Some(TurnId::from_str(TURN).unwrap()));
}

#[test]
fn a_message_without_a_valid_checkpoint_id_is_not_a_checkpoint() {
    assert_eq!(parse("Initial commit\n"), None);
    assert_eq!(parse("x\n\nClauseo-Checkpoint: not-a-uuid\n"), None);
    // The id must be in the last paragraph.
    let earlier = format!("x\n\nClauseo-Checkpoint: {ID}\n\nSigned-off-by: someone\n");
    assert_eq!(parse(&earlier), None);
}

fn skipped(path: &str, reason: SkippedFileReason) -> SkippedFile {
    SkippedFile {
        path: MatterPath::new(path).unwrap(),
        reason,
    }
}

#[test]
fn files_read_for_the_first_time_follow_the_turn_and_round_trip() {
    let reads = FirstReads {
        baseline: vec![
            skipped("notes.txt", SkippedFileReason::CloudOnly),
            skipped("Exhibits/scan 2.pdf ", SkippedFileReason::OverSizeCap),
        ],
        changed: vec![skipped("brief: v2.docx", SkippedFileReason::CloudOnly)],
    };
    let text = message(&entry(CheckpointKind::AfterTurn, true), 1, &reads);
    let expected = format!(
        "After turn: 1 file changed\n\nClauseo-Checkpoint: {ID}\nClauseo-Kind: after-turn\n\
         Clauseo-Thread: {THREAD}\nClauseo-Turn: {TURN}\n\
         Clauseo-Baseline: cloud-only notes.txt\n\
         Clauseo-Baseline: over-size-cap Exhibits/scan 2.pdf \n\
         Clauseo-Was-Skipped: cloud-only brief: v2.docx\n"
    );
    assert_eq!(text, expected);
    // A path's spaces, even a last one, and a `: ` in it are kept.
    assert_eq!(first_reads(&text), reads);
    // Older readers see the same checkpoint: the new keys are unknown to them.
    assert_eq!(parse(&text), Some(entry(CheckpointKind::AfterTurn, true)));
    assert_eq!(
        first_reads(&message(
            &entry(CheckpointKind::Undo, false),
            0,
            &FirstReads::default()
        )),
        FirstReads::default()
    );
}

#[test]
fn a_reason_from_a_newer_clauseo_reads_as_unknown_and_a_bad_value_is_left_out() {
    let text = format!(
        "Before turn: 0 files changed\n\nClauseo-Checkpoint: {ID}\n\
         Clauseo-Was-Skipped: too-deep a/b.txt\nClauseo-Baseline: no-path\n\
         Clauseo-Baseline: cloud-only /rooted\n"
    );
    assert_eq!(
        first_reads(&text),
        FirstReads {
            baseline: Vec::new(),
            changed: vec![skipped("a/b.txt", SkippedFileReason::Unknown)],
        }
    );
}
