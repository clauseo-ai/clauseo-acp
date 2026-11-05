//! `TurnFailed` examples for codes added after the 0048 install and sign-in set, which
//! `later_variants` in the parent module chains on before them, R24's case-law records, and the
//! host notices with a kind, which it chains on last, the skill pack notices among them.

use clauseo_protocol::{
    AgentId, DomainEvent, ErrorCode, MatterPath, Notice, NoticeKind, NoticeLevel, PackId,
    PackSkipReason, PageVia, PageVisited, SearchSource, TurnFailed, WebResult, WebSearchRecorded,
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
        failed(
            ErrorCode::MatterFolderTooDeep,
            "too deep for Windows to start programs in",
        ),
        failed(
            ErrorCode::SigninAgentsStillRunning,
            "an agent was still running after the stop wait",
        ),
        failed(
            ErrorCode::AgentKeysLoading,
            "the stored API keys have not loaded yet",
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

/// One host `Notice` for each `NoticeKind`, with its fallback text, and a skill pack notice for
/// each `PackSkipReason`, so the corpus holds every reason.
pub(super) fn notices() -> Vec<DomainEvent> {
    let mut notices = vec![
        Notice {
            level: NoticeLevel::Info,
            text: "Clauseo doesn't remember the earlier questions and answers.".to_owned(),
            kind: Some(NoticeKind::ContextNotRestored),
        },
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
        },
    ];
    notices.extend(
        [
            (
                PackSkipReason::NotTurnedOn,
                "This matter is set to use it, but it isn't turned on.",
            ),
            (
                PackSkipReason::NotAdmitted,
                "The skill pack settings on this computer don't allow it.",
            ),
            (
                PackSkipReason::Changed,
                "Its files changed after it was turned on. Turn it on again.",
            ),
            (
                PackSkipReason::Missing,
                "Clauseo couldn't find it on this computer.",
            ),
            (
                PackSkipReason::Invalid,
                "Its files aren't a valid skill pack.",
            ),
            (
                PackSkipReason::TooMany,
                "Clauseo loads at most 8 skill packs at a time.",
            ),
        ]
        .into_iter()
        .map(|(reason, words)| skill_pack_notice(reason, words)),
    );
    notices.into_iter().map(DomainEvent::from).collect()
}

/// A host notice of kind `SkillPackNotLoaded` for the pack `nda-kit`, left out for `reason`.
fn skill_pack_notice(reason: PackSkipReason, words: &str) -> Notice {
    Notice {
        level: NoticeLevel::Warning,
        text: format!("Clauseo didn't load the skill pack \"nda-kit\". {words}"),
        kind: Some(NoticeKind::SkillPackNotLoaded {
            pack: PackId::new("nda-kit").unwrap(),
            reason,
        }),
    }
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
