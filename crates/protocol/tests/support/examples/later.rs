//! `TurnFailed` examples for codes added after the 0048 install and sign-in set, which
//! `later_variants` in the parent module chains on before them.

use clauseo_protocol::{DomainEvent, ErrorCode, TurnFailed};

use crate::fixtures::turn_id;

/// One `TurnFailed` of turn 3 for each later code, in the order the corpus first saw them.
pub(super) fn failures() -> Vec<DomainEvent> {
    vec![
        failed(
            ErrorCode::SettingsRefused,
            "plans.claude-acp.endpoint: not an https address",
        ),
        failed(
            ErrorCode::SettingsKeyStored,
            "a key is already stored for claude-acp",
        ),
    ]
}

/// A `TurnFailed` of turn 3 with `code` and `detail`.
fn failed(code: ErrorCode, detail: &str) -> DomainEvent {
    TurnFailed {
        turn: turn_id(3),
        code,
        detail: Some(detail.to_owned()),
    }
    .into()
}
