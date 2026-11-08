//! R18's commands (denials and widening): their JSON, `validate()`'s one rule, the closed
//! `EscalationRequest`, `ListAllowances` and its answer, a required `is_file`, and the four
//! new error codes.

use clauseo_protocol::{
    AllowTarget, ErrorCode, EscalationRequest, GrantMode, HostCommand, HostQuery, MatterAllowance,
    QueryResult, Timestamp,
};

use super::Validate;
use super::fixtures::{allowance_id, denial_id, matter_id, round_trips, thread_id, v7_text};

fn escalate(request: EscalationRequest) -> HostCommand {
    HostCommand::RequestEscalation { request }
}

#[test]
fn each_command_has_its_json_and_passes_validate() {
    let cases = [
        (
            escalate(EscalationRequest::Denial {
                thread: thread_id(2),
                denial: denial_id(1),
            }),
            format!(
                r#"{{"request_escalation":{{"request":{{"denial":{{"thread":"{}","denial":"{}"}}}}}}}}"#,
                v7_text(2),
                v7_text(1)
            ),
        ),
        (
            escalate(EscalationRequest::SwitchProtectionOff {
                matter: matter_id(1),
            }),
            format!(
                r#"{{"request_escalation":{{"request":{{"switch_protection_off":{{"matter":"{}"}}}}}}}}"#,
                v7_text(1)
            ),
        ),
        (
            escalate(EscalationRequest::FilesFromOtherMatter {
                matter: matter_id(1),
                from: matter_id(3),
            }),
            format!(
                r#"{{"request_escalation":{{"request":{{"files_from_other_matter":{{"matter":"{}","from":"{}"}}}}}}}}"#,
                v7_text(1),
                v7_text(3)
            ),
        ),
        (
            HostCommand::SwitchProtectionOn {
                matter: matter_id(1),
            },
            format!(
                r#"{{"switch_protection_on":{{"matter":"{}"}}}}"#,
                v7_text(1)
            ),
        ),
        (
            HostCommand::ForgetAllowance {
                matter: matter_id(1),
                allowance: allowance_id(2),
            },
            format!(
                r#"{{"forget_allowance":{{"matter":"{}","allowance":"{}"}}}}"#,
                v7_text(1),
                v7_text(2)
            ),
        ),
        (
            HostCommand::DismissDenial {
                thread: thread_id(2),
                denial: denial_id(1),
            },
            format!(
                r#"{{"dismiss_denial":{{"thread":"{}","denial":"{}"}}}}"#,
                v7_text(2),
                v7_text(1)
            ),
        ),
        (
            HostCommand::GoOnInsideMatter {
                thread: thread_id(2),
            },
            format!(r#"{{"go_on_inside_matter":{{"thread":"{}"}}}}"#, v7_text(2)),
        ),
    ];
    for (command, json) in &cases {
        assert_eq!(&serde_json::to_string(command).unwrap(), json);
        round_trips(command);
        assert!(command.validate().is_ok(), "{json}");
    }
}

/// Another matter's files come from another matter: the same matter twice is a shape error.
#[test]
fn files_from_the_same_matter_are_refused() {
    let same = escalate(EscalationRequest::FilesFromOtherMatter {
        matter: matter_id(1),
        from: matter_id(1),
    });
    let refused = same.validate().unwrap_err().to_string();
    assert!(refused.contains("RequestEscalation"), "{refused}");
}

/// `EscalationRequest` is a command's, so closed: a newer kind, a newer field, a path or a
/// missing field is refused as the command decodes.
#[test]
fn a_request_is_closed() {
    let m = v7_text(1);
    for bad in [
        r#"{"request_escalation":{"request":"switch_protection_off"}}"#.to_owned(),
        format!(r#"{{"request_escalation":{{"request":{{"widen_later":{{"matter":"{m}"}}}}}}}}"#),
        format!(
            r#"{{"request_escalation":{{"request":{{"switch_protection_off":{{"matter":"{m}","for":"ever"}}}}}}}}"#
        ),
        format!(
            r#"{{"request_escalation":{{"request":{{"files_from_other_matter":{{"matter":"{m}","path":"/etc"}}}}}}}}"#
        ),
        format!(
            r#"{{"request_escalation":{{"request":{{"denial":{{"thread":"{m}","denial":"{m}"}}}},"path":"/etc"}}}}"#
        ),
        // The denial without its thread, and the old bare id.
        format!(r#"{{"request_escalation":{{"request":{{"denial":{{"denial":"{m}"}}}}}}}}"#),
        format!(r#"{{"request_escalation":{{"request":{{"denial":"{m}"}}}}}}"#),
        format!(r#"{{"dismiss_denial":{{"thread":"{m}","denial":"{m}","forget":true}}}}"#),
        format!(r#"{{"dismiss_denial":{{"denial":"{m}"}}}}"#),
        r#"{"go_on_inside_matter":{}}"#.to_owned(),
        format!(r#"{{"go_on_inside_matter":{{"thread":"{m}","text":"go on"}}}}"#),
        format!(r#"{{"forget_allowance":{{"allowance":"{m}"}}}}"#),
    ] {
        assert!(serde_json::from_str::<HostCommand>(&bad).is_err(), "{bad}");
    }
}

#[test]
fn the_query_is_a_plain_string_and_its_answer_names_each_field() {
    assert_eq!(
        serde_json::to_string(&HostQuery::ListAllowances).unwrap(),
        r#""list_allowances""#
    );
    round_trips(&HostQuery::ListAllowances);
    let answer = QueryResult::Allowances(vec![MatterAllowance {
        matter: matter_id(1),
        allowance: allowance_id(2),
        target: AllowTarget::OtherMatterPath {
            matter: matter_id(3),
            path: "/Users/a/Matters/Smith".to_owned(),
            is_file: false,
        },
        mode: GrantMode::ReadOnly,
        allowed: true,
        at: Timestamp::new(1_794_031_200_000).unwrap(),
    }]);
    assert_eq!(
        serde_json::to_string(&answer).unwrap(),
        format!(
            r#"{{"allowances":[{{"matter":"{}","allowance":"{}","target":{{"other_matter_path":{{"matter":"{}","path":"/Users/a/Matters/Smith","is_file":false}}}},"mode":"read_only","allowed":true,"at":1794031200000}}]}}"#,
            v7_text(1),
            v7_text(2),
            v7_text(3)
        )
    );
    round_trips(&answer);
}

/// `is_file` is required: a missing field would read as a folder, the wider grant, so a
/// target without it is refused, in a list's answer as in the log.
#[test]
fn a_target_without_is_file_is_refused() {
    let m = v7_text(1);
    let missing = format!(r#"{{"other_matter_path":{{"matter":"{m}","path":"/Users/a/x"}}}}"#);
    assert!(
        serde_json::from_str::<AllowTarget>(&missing).is_err(),
        "{missing}"
    );
    let given =
        format!(r#"{{"other_matter_path":{{"matter":"{m}","path":"/Users/a/x","is_file":true}}}}"#);
    assert!(matches!(
        serde_json::from_str::<AllowTarget>(&given),
        Ok(AllowTarget::OtherMatterPath { is_file: true, .. })
    ));
}

#[test]
fn the_four_codes_are_not_retryable() {
    for (code, text) in [
        (ErrorCode::EscalationNotPending, "escalation.not_pending"),
        (
            ErrorCode::EscalationNotOfferable,
            "escalation.not_offerable",
        ),
        (
            ErrorCode::SandboxUnprotectedAutoForbidden,
            "sandbox.unprotected_auto_forbidden",
        ),
        (
            ErrorCode::EscalationNetworkLocation,
            "escalation.network_location",
        ),
    ] {
        assert_eq!(code.as_str(), text);
        assert!(!code.is_retryable());
        assert_eq!(ErrorCode::from_str_lossy(text), code);
    }
}
