//! `TurnFailed` examples for codes added after the 0048 install and sign-in set, which
//! `later_variants` in the parent module chains on before them, and the host notices with a
//! kind, which it chains on last.

use clauseo_protocol::{DomainEvent, ErrorCode, Notice, NoticeKind, NoticeLevel, TurnFailed};

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
        failed(
            ErrorCode::SigninLoginShared,
            "the Codex home shares the ChatGPT login file",
        ),
        failed(
            ErrorCode::SigninFirmManaged,
            "claude-acp's sign-in is refused: a firm endpoint is set by policy",
        ),
        failed(
            ErrorCode::SigninKeyInUse,
            "claude-acp's sign-in is refused: an API key is stored",
        ),
        failed(
            ErrorCode::SigninEndpointInUse,
            "claude-acp's sign-in is refused: an endpoint is set in Settings",
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

/// One host `Notice` for each `NoticeKind`, with its fallback text.
pub(super) fn notices() -> Vec<DomainEvent> {
    vec![
        Notice {
            level: NoticeLevel::Info,
            text: "Clauseo doesn't remember the earlier questions and answers.".to_owned(),
            kind: Some(NoticeKind::ContextNotRestored),
        }
        .into(),
    ]
}
