//! R18's events: the thread stream's denials and the matter stream's allowances, in
//! `DomainEvent::KINDS` order, and the events that hold the variants those examples do not.

use clauseo_protocol::{
    AllowTarget, AllowanceForgotten, AllowanceRecorded, DenialDismissed, DenialKind,
    DenialRecorded, DenialSource, DenialsPaused, DomainEvent, EscalationAnswer, EscalationAnswered,
    GrantMode, OtherMatter, ProtectionSwitched, ReadGrantForgotten, ReadGrantedTo,
};

use crate::fixtures::{
    allowance_id, denial_id, escalation_request_id, matter_id, tool_call_id, turn_id,
};

/// Another matter, as a denial names it.
fn smith() -> OtherMatter {
    OtherMatter {
        matter: matter_id(3),
        name: "Smith v Jones".to_owned(),
    }
}

/// One example of each thread-stream kind, every optional field filled.
pub(super) fn denial_events() -> Vec<DomainEvent> {
    vec![
        DenialRecorded {
            turn: Some(turn_id(3)),
            denial: denial_id(1),
            kind: DenialKind::Folder {
                path: "/Users/asha/Matters/Smith v Jones/Pleadings".to_owned(),
                other_matter: Some(smith()),
            },
            source: DenialSource::Sandbox,
            offered: true,
            count: 2,
        }
        .into(),
        EscalationAnswered {
            request: escalation_request_id(1),
            denial: Some(denial_id(1)),
            answer: EscalationAnswer::AllowForMatter,
        }
        .into(),
        DenialDismissed {
            denial: denial_id(1),
        }
        .into(),
        DenialsPaused {
            turn: turn_id(3),
            refused: 3,
        }
        .into(),
    ]
}

/// One example of each matter-stream kind.
pub(super) fn allowance_events() -> Vec<DomainEvent> {
    vec![
        AllowanceRecorded {
            allowance: allowance_id(1),
            target: AllowTarget::Folder {
                path: "/Users/asha/Deals".to_owned(),
            },
            mode: GrantMode::ReadOnly,
            allowed: true,
        }
        .into(),
        AllowanceForgotten {
            allowance: allowance_id(1),
        }
        .into(),
        ReadGrantedTo {
            by_matter: matter_id(4),
            by_name: "Jones estate".to_owned(),
            allowance: allowance_id(2),
        }
        .into(),
        ReadGrantForgotten {
            by_matter: matter_id(4),
            allowance: allowance_id(2),
        }
        .into(),
        ProtectionSwitched { off: true }.into(),
    ]
}

/// Events holding each R18 variant the examples above do not: the other denial kinds and
/// sources, the other answers and the other allow targets.
pub(super) fn variants() -> Vec<DomainEvent> {
    let denial = |n, kind, source, offered| -> DomainEvent {
        DenialRecorded {
            turn: Some(turn_id(3)),
            denial: denial_id(n),
            kind,
            source,
            offered,
            count: 1,
        }
        .into()
    };
    let answer = |n, answer| -> DomainEvent {
        EscalationAnswered {
            request: escalation_request_id(n),
            denial: Some(denial_id(n)),
            answer,
        }
        .into()
    };
    let allow = |n, target, allowed| -> DomainEvent {
        AllowanceRecorded {
            allowance: allowance_id(n),
            target,
            mode: GrantMode::ReadOnly,
            allowed,
        }
        .into()
    };
    vec![
        denial(
            2,
            DenialKind::File {
                path: "C:\\Users\\asha\\notes.docx".to_owned(),
                other_matter: None,
            },
            DenialSource::ToolCall {
                tool_call: tool_call_id(4),
            },
            true,
        ),
        denial(
            3,
            DenialKind::Site {
                host: "files.example-host.net".to_owned(),
                shared_hosting: true,
            },
            DenialSource::Proxy,
            true,
        ),
        denial(
            4,
            DenialKind::UpstreamAuth {
                upstream: "proxy.firm.example".to_owned(),
            },
            DenialSource::Proxy,
            false,
        ),
        answer(2, EscalationAnswer::AllowOnce),
        answer(3, EscalationAnswer::Refuse),
        answer(4, EscalationAnswer::RefuseForMatter),
        allow(
            3,
            AllowTarget::File {
                path: "/Users/asha/notes.docx".to_owned(),
            },
            true,
        ),
        allow(
            4,
            AllowTarget::OtherMatterPath {
                matter: matter_id(3),
                path: "/Users/asha/Matters/Smith v Jones/brief.docx".to_owned(),
                is_file: true,
            },
            true,
        ),
        allow(
            5,
            AllowTarget::Site {
                host: "example.com".to_owned(),
            },
            false,
        ),
    ]
}
