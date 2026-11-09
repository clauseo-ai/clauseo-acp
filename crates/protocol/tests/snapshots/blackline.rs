//! Golden snapshots of the Word blackline's wire types: the query, with a checkpoint and with
//! the folder now, the answer, and the values of its two unit enums. A child of `snapshots.rs`,
//! so its snapshots sit in `snapshots/` with the rest.

use clauseo_protocol::{
    BlacklineParagraph, BlacklineRun, HostQuery, ParagraphChange, QueryResult, RunKind,
    WordBlackline,
};

use super::*;

/// `WordBlackline` to a checkpoint, and to the folder now (no `to`).
#[test]
fn host_query_word_blackline() {
    let query = |to| HostQuery::WordBlackline {
        matter: matter_id(1),
        path: path("Lease.docx"),
        from: checkpoint_id(2),
        to,
    };
    snapshot!("host_query_word_blackline", query(Some(checkpoint_id(3))));
    snapshot!("host_query_word_blackline_folder_now", query(None));
}

/// An answer with one paragraph of each change.
#[test]
fn query_result_word_blackline() {
    let run = |kind, text: &str| BlacklineRun {
        kind,
        text: text.to_owned(),
    };
    let paragraph = |change, runs| BlacklineParagraph { change, runs };
    let answer = QueryResult::WordBlackline(WordBlackline {
        paragraphs: vec![
            paragraph(
                ParagraphChange::Same,
                vec![run(RunKind::Same, "Lease Agreement")],
            ),
            paragraph(
                ParagraphChange::Changed,
                vec![
                    run(RunKind::Same, "Governing law is "),
                    run(RunKind::Del, "New York"),
                    run(RunKind::Ins, "Delaware"),
                    run(RunKind::Same, "."),
                ],
            ),
            paragraph(
                ParagraphChange::Added,
                vec![run(RunKind::Ins, "Notices go by email.")],
            ),
            paragraph(
                ParagraphChange::Removed,
                vec![run(RunKind::Del, "Old clause.")],
            ),
        ],
        truncated: false,
    });
    snapshot!("query_result_word_blackline", answer);
    round_trips(&answer);
}

/// Each value of the blackline's unit enums, in order.
#[test]
fn word_blackline_values() {
    snapshot!(
        "word_blackline_values",
        (ParagraphChange::ALL, RunKind::ALL)
    );
}
