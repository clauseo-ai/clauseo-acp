//! `TurnFailed` examples for codes added after the 0048 install and sign-in set, which
//! `later_variants` in the parent module chains on before them, R24's case-law records, and the
//! host notices with a kind, which it chains on next, the skill pack notices among them, and the
//! later stop reasons (`NotAllowed`'s moved here from it), which it chains on next, and the later
//! steer outcomes, which it chains on last.

use clauseo_protocol::{
    AgentId, DomainEvent, ErrorCode, MatterPath, Notice, NoticeKind, NoticeLevel, PackId,
    PackSkipReason, PageVia, PageVisited, SearchSource, SteerOutcome, SteerResolved, StopReason,
    TurnEnded, TurnFailed, WebResult, WebSearchRecorded,
};

use crate::fixtures::{checkpoint_id, message_id, tool_call_id, turn_id};

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
        failed(
            ErrorCode::AgentKeyUnreadable,
            "a stored API key could not be read",
        ),
        failed(
            ErrorCode::MatterDocumentPathTooLong,
            "the document's path is too long for Word",
        ),
        failed(
            ErrorCode::MatterProtectedLink,
            "sub\\AGENTS.md is a shortcut, and Clauseo can't protect a shortcut",
        ),
        failed(
            ErrorCode::SandboxSetupRefused,
            "the administrator prompt was declined",
        ),
        failed(ErrorCode::SandboxSetupFailed, "the sandbox check failed"),
        failed(
            ErrorCode::SandboxSetupRequired,
            "the sandbox set-up is not done",
        ),
        failed(
            ErrorCode::SandboxRequired,
            "the firm requires the sandbox; this computer is at tier C",
        ),
        failed(
            ErrorCode::MatterFolderNotAMatter,
            "the matter folder has no .clauseo/matter.json",
        ),
        failed(
            ErrorCode::SandboxBlocked,
            "the sandbox can't run: clauseo-sandbox-setup.exe is missing or was blocked",
        ),
        failed(
            ErrorCode::BlocklistOverrideOff,
            "allowing a site on the unsafe-site list is turned off",
        ),
        failed(
            ErrorCode::BlocklistNoticeExpired,
            "the blocked-site notice is no longer kept",
        ),
        failed(
            ErrorCode::BlocklistAllowNotSaved,
            "the allow is kept until Clauseo closes",
        ),
        failed(
            ErrorCode::BlocklistRevokeNotSaved,
            "the data folder is read-only",
        ),
        failed(
            ErrorCode::SandboxCleaningUp,
            "the sandbox's clean-up after an earlier session isn't finished",
        ),
        failed(ErrorCode::BlacklineNoWordTool, "stetdoc is not configured"),
        failed(ErrorCode::BlacklineNotWord, "the path is not a .docx"),
        failed(ErrorCode::BlacklineFailed, "stetdoc compare exited 1"),
        // Never a turn's failure in practice (tracked-change codes); here for the corpus.
        failed(ErrorCode::WordStetdocMissing, "no stetdoc program"),
        failed(
            ErrorCode::WordRevisionsStale,
            "the file changed since the list",
        ),
        failed(ErrorCode::WordNotDocx, "not a Word document"),
        failed(ErrorCode::WordEditFailed, "stetdoc timed out"),
        failed(
            ErrorCode::MatterFolderPermission,
            "the privacy settings keep Clauseo from opening the matter folder",
        ),
        failed(
            ErrorCode::AgentOffline,
            "the window said the computer is offline",
        ),
        failed(
            ErrorCode::PromptTooLong,
            "SendPrompt: prompt text in JSON-escaped bytes is 6291457, over the limit of 6291456",
        ),
    ]
    .into_iter()
    .chain(mcp_failures())
    .collect()
}

/// The Settings › Tools codes (`mcp.*`): never a turn's failure in practice; here for the corpus.
fn mcp_failures() -> Vec<DomainEvent> {
    vec![
        failed(ErrorCode::McpNameEmpty, "the server has no name"),
        failed(ErrorCode::McpNameTaken, "another server has this name"),
        failed(
            ErrorCode::McpNameReserved,
            "the name is kept for Clauseo's own tool",
        ),
        failed(
            ErrorCode::McpNameInvalid,
            "the name holds a character it may not",
        ),
        failed(
            ErrorCode::McpCommandInvalid,
            "the program is a relative path",
        ),
        failed(
            ErrorCode::McpCommandNotFound,
            "the program is not on this computer",
        ),
        failed(ErrorCode::McpUrlInvalid, "the URL is not https"),
        failed(
            ErrorCode::McpHeaderNameInvalid,
            "a header name is not a token",
        ),
        failed(
            ErrorCode::McpEnvNameInvalid,
            "a variable name is not an identifier",
        ),
        failed(
            ErrorCode::McpSecretStoreFailed,
            "the keychain refused the write",
        ),
        failed(ErrorCode::McpServerUnknown, "no server of that name"),
        failed(
            ErrorCode::McpSaveDeclined,
            "the lawyer did not allow the program",
        ),
    ]
}

/// A `TurnFailed` of turn 3 with `code` and `detail`.
/// Folder PR 2's refusal codes, never a turn's failure in practice; here for the corpus.
fn folder_failures() -> Vec<DomainEvent> {
    vec![
        failed(
            ErrorCode::MatterFolderInClauseoData,
            "inside Clauseo's data folder",
        ),
        failed(
            ErrorCode::MatterFolderInsideMatter,
            "inside another matter's folder",
        ),
        failed(
            ErrorCode::MatterFolderHoldsMatter,
            "holds another matter's folder",
        ),
    ]
}

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
        Notice {
            level: NoticeLevel::Info,
            text: "Claude's automatic mode isn't available for this account, so Claude asks \
                   before running commands."
                .to_owned(),
            kind: Some(NoticeKind::AutoModeUnavailable),
        },
        Notice {
            level: NoticeLevel::Warning,
            text: "Clauseo blocked a request: it reaches outside this matter.".to_owned(),
            kind: Some(NoticeKind::OutsideMatterBlocked),
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

/// [`failures`], then [`folder_failures`], then [`case_law`], as the corpus first saw them.
pub(super) fn failures_then_case_law() -> Vec<DomainEvent> {
    failures()
        .into_iter()
        .chain(folder_failures())
        .chain(case_law())
        .collect()
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

/// One `TurnEnded` of turn 3 for each stop reason added after v1: `NotAllowed`, `OutsideMatter`,
/// `TooLong`.
pub(super) fn stops() -> Vec<DomainEvent> {
    vec![
        TurnEnded {
            turn: turn_id(3),
            stop: StopReason::NotAllowed,
            usage: None,
            checkpoint_after: Some(checkpoint_id(14)),
        }
        .into(),
        TurnEnded {
            turn: turn_id(3),
            stop: StopReason::OutsideMatter,
            usage: None,
            checkpoint_after: Some(checkpoint_id(14)),
        }
        .into(),
        TurnEnded {
            turn: turn_id(3),
            stop: StopReason::TooLong,
            usage: None,
            checkpoint_after: Some(checkpoint_id(14)),
        }
        .into(),
    ]
}

/// One `SteerResolved` for each steer outcome added after v1: `NotSent`.
pub(super) fn steers() -> Vec<DomainEvent> {
    vec![
        SteerResolved {
            message: message_id(5),
            outcome: SteerOutcome::NotSent { turn: turn_id(3) },
            aside: Vec::new(),
        }
        .into(),
    ]
}
