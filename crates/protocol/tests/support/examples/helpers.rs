//! Examples of a helper's events: its start with every field filled, a run of its words, and
//! its end, one per outcome.

use clauseo_protocol::{
    DomainEvent, HelperEnded, HelperOutcome, HelperStarted, HelperText, TextOrBlob,
};

use crate::fixtures::{helper_id, short, tool_call_id, turn_id};

/// A helper the lead launched with its `Agent` call, every field filled.
pub fn helper_started() -> HelperStarted {
    HelperStarted {
        turn: turn_id(3),
        helper: helper_id(5),
        acp_id: short("toolu_02"),
        parent: Some(helper_id(6)),
        call: Some(tool_call_id(4)),
        name: short("paper-reader"),
        label: Some(short("Read the lease")),
        task: Some(TextOrBlob::Inline(
            "Read lease.docx and list the break clauses.".to_owned(),
        )),
        model: Some(short("haiku")),
    }
}

/// A run of the helper's words.
pub fn helper_text() -> HelperText {
    HelperText {
        turn: turn_id(3),
        helper: helper_id(5),
        text: "The lease runs to 2031. \"Break\" next.\n".to_owned(),
    }
}

/// The helper's end with `outcome`.
pub fn helper_ended(outcome: HelperOutcome) -> HelperEnded {
    HelperEnded {
        helper: helper_id(5),
        outcome,
        summary: Some(TextOrBlob::Inline(
            "Two break clauses: 2027 and 2029.".to_owned(),
        )),
    }
}

/// The ends no example in `every_event` holds, for the corpus writer.
pub fn other_ends() -> Vec<DomainEvent> {
    vec![
        helper_ended(HelperOutcome::Failed).into(),
        HelperEnded {
            summary: None,
            ..helper_ended(HelperOutcome::Stopped)
        }
        .into(),
    ]
}
