//! The approval, elicitation, plan, mode, config and command events of a session, in
//! `examples.rs`'s `every_event`, which chains on them.

use clauseo_protocol::{
    AlwaysKind, AlwaysOffer, AvailableCommand, CommandsAvailable, ConfigCategory, ConfigChoice,
    ConfigOption, ConfigOptionKind, ConfigOptionsReplaced, Deliverable, DomainEvent,
    ElicitationExpired, ElicitationOutcome, ElicitationRequest, ElicitationRequested,
    ElicitationResolved, ExpiryReason, ModeChanged, PermissionAbout, PermissionChoice,
    PermissionExpired, PermissionKind, PermissionOption, PermissionRequested, PermissionResolved,
    PlanEntry, PlanPriority, PlanReplaced, PlanStatus, Principal,
};

use crate::fixtures::{elicitation_id, path, permission_request_id, raw, short, tool_call_id};

/// Approvals and session state.
pub(super) fn session_events() -> Vec<DomainEvent> {
    let select = ConfigOption {
        id: short("model"),
        name: short("Model"),
        description: None,
        category: ConfigCategory::Model,
        kind: ConfigOptionKind::Select {
            current_value: short("opus"),
            options: vec![ConfigChoice {
                value: short("opus"),
                name: short("Opus"),
                description: Some("Largest".to_owned()),
                group: None,
            }],
        },
    };
    vec![
        PermissionRequested {
            request: permission_request_id(10),
            tool_call: Some(tool_call_id(4)),
            options: vec![PermissionOption {
                option_id: short("allow"),
                kind: PermissionKind::AllowOnce,
                agent_label: "Yes".to_owned(),
            }],
            deliverable: Deliverable::Live,
            about: Some(PermissionAbout::AgentPlan),
            always: Some(Box::new(AlwaysOffer {
                kind: AlwaysKind::Command,
                shown: Some("wc -l notes.txt".into()),
                cwd: Some("Drafts".into()),
                server: None,
                tool: None,
            })),
        }
        .into(),
        PermissionResolved {
            request: permission_request_id(10),
            choice: PermissionChoice::Cancelled,
            by: Principal::LocalDesktop,
            held_open: Some(path("Brief.docx")),
            always: None,
        }
        .into(),
        PermissionExpired {
            request: permission_request_id(11),
            reason: ExpiryReason::TurnEnded,
        }
        .into(),
        ElicitationRequested {
            elicitation: elicitation_id(12),
            request: ElicitationRequest::Form {
                message: "Which party?".to_owned(),
                schema: raw(r#"{"type":"object","properties":{"party":{"type":"string"}}}"#),
            },
        }
        .into(),
        ElicitationResolved {
            elicitation: elicitation_id(12),
            outcome: ElicitationOutcome::Accepted,
            by: Principal::LocalDesktop,
        }
        .into(),
        ElicitationExpired {
            elicitation: elicitation_id(13),
            reason: ExpiryReason::AgentExited,
        }
        .into(),
        PlanReplaced {
            entries: vec![PlanEntry {
                content: "Read the lease".to_owned(),
                priority: PlanPriority::High,
                status: PlanStatus::InProgress,
            }],
        }
        .into(),
        ModeChanged {
            mode_id: short("plan"),
        }
        .into(),
        ConfigOptionsReplaced {
            options: vec![select],
        }
        .into(),
        CommandsAvailable {
            commands: vec![AvailableCommand {
                name: short("review"),
                description: "Review a document".to_owned(),
                input_hint: None,
            }],
        }
        .into(),
    ]
}

/// The host's session-only option (`allow_session`, "For this conversation"), on a permission
/// request of its own, so the corpus writer finds a holder for the kind.
pub(super) fn later_permission_kinds() -> Vec<DomainEvent> {
    let option = |id: &str, kind, label: &str| PermissionOption {
        option_id: short(id),
        kind,
        agent_label: label.to_owned(),
    };
    vec![
        PermissionRequested {
            request: permission_request_id(16),
            tool_call: None,
            options: vec![
                option("allow_once", PermissionKind::AllowOnce, "Allow"),
                option(
                    "clauseo.session:allow_session",
                    PermissionKind::AllowSession,
                    "Allow for this session",
                ),
            ],
            deliverable: Deliverable::Live,
            about: None,
            always: None,
        }
        .into(),
    ]
}

/// The kinds of "Always" no example in [`session_events`] holds: a file edit, a site, a tool and
/// a file read, each a permission request of its own, so the corpus writer finds a holder.
pub(super) fn later_always_kinds() -> Vec<DomainEvent> {
    [
        (AlwaysKind::FileEdit, None, None),
        (AlwaysKind::Site, Some("example.com"), None),
        (AlwaysKind::Tool, None, Some(("case-law", "search_law"))),
        (AlwaysKind::Read, Some("Brief.docx"), None),
    ]
    .into_iter()
    .zip(12..)
    .map(|((kind, shown, tool), request)| {
        PermissionRequested {
            request: permission_request_id(request),
            tool_call: None,
            options: vec![PermissionOption {
                option_id: short("allow-once"),
                kind: PermissionKind::AllowOnce,
                agent_label: "Yes".to_owned(),
            }],
            deliverable: Deliverable::Live,
            about: None,
            always: Some(Box::new(AlwaysOffer {
                kind,
                shown: shown.map(str::to_owned),
                cwd: None,
                server: tool.map(|(server, _)| server.to_owned()),
                tool: tool.map(|(_, tool)| tool.to_owned()),
            })),
        }
        .into()
    })
    .collect()
}
