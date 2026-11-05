//! `TurnFailed` examples for codes added after the 0048 install and sign-in set, which
//! `later_variants` in the parent module chains on before them, R24's case-law records, and the
//! host notices with a kind, which it chains on last.

use clauseo_protocol::{
    AgentId, DomainEvent, ErrorCode, MatterPath, Notice, NoticeKind, NoticeLevel, PageVia,
    PageVisited, SearchSource, TurnFailed, WebResult, WebSearchRecorded,
};

use crate::fixtures::{tool_call_id, turn_id};

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
        Notice {
            level: NoticeLevel::Info,
            text: "2 papers came back after the undo. Clauseo didn't remove them again.".to_owned(),
            kind: Some(NoticeKind::FilesCameBack {
                files: vec![
                    MatterPath::new("Drafts/memo.docx").unwrap(),
                    MatterPath::new("notes.txt").unwrap(),
                ],
                more: 0,
            }),
        }
        .into(),
    ]
}

/// [`failures`], then [`case_law`], as the corpus first saw them.
pub(super) fn failures_then_case_law() -> Vec<DomainEvent> {
    failures().into_iter().chain(case_law()).collect()
}

/// R24: a document read through the case-law connector, and the connector's search.
fn case_law() -> Vec<DomainEvent> {
    let link = "https://indiankanoon.org/doc/1234567/";
    vec![
        PageVisited::new(
            turn_id(3),
            tool_call_id(4),
            link,
            Some("A v. B".to_owned()),
            PageVia::CaseLaw,
        )
        .into(),
        Box::new(
            WebSearchRecorded::new(
                turn_id(3),
                tool_call_id(4),
                AgentId::new("claude-acp").unwrap(),
                "section 138 NI Act; dishonour of cheque",
                vec![WebResult::new("A v. B", link)],
            )
            .with_source(SearchSource::CaseLaw, Some("search_law")),
        )
        .into(),
    ]
}
