//! The approval, elicitation, plan, mode, config and command events of a session, in
//! `examples.rs`'s `every_event`, which chains on them.

use clauseo_protocol::{
    AvailableCommand, CommandsAvailable, ConfigCategory, ConfigChoice, ConfigOption,
    ConfigOptionKind, ConfigOptionsReplaced, Deliverable, DomainEvent, ElicitationExpired,
    ElicitationOutcome, ElicitationRequest, ElicitationRequested, ElicitationResolved,
    ExpiryReason, ModeChanged, PermissionAbout, PermissionChoice, PermissionExpired,
    PermissionKind, PermissionOption, PermissionRequested, PermissionResolved, PlanEntry,
    PlanPriority, PlanReplaced, PlanStatus, Principal,
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
        }
        .into(),
        PermissionResolved {
            request: permission_request_id(10),
            choice: PermissionChoice::Cancelled,
            by: Principal::LocalDesktop,
            held_open: Some(path("Brief.docx")),
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
