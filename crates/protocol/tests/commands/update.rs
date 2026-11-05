//! The updater's commands and query (decision record 0037): their JSON, and `validate()`
//! refusing `SetUpdateMode { mode: none }`, which comes only from policy.

use clauseo_protocol::{
    CommandEnvelope, ErrorCode, HostCommand, HostError, HostQuery, InstallWhen, UpdateMode,
};

use super::Validate;
use super::fixtures::{command_id, round_trips};

#[test]
fn get_update_is_a_plain_string() {
    assert_eq!(
        serde_json::to_string(&HostQuery::GetUpdate).unwrap(),
        r#""get_update""#
    );
    round_trips(&HostQuery::GetUpdate);
}

#[test]
fn the_update_commands_have_their_json_and_pass_validate() {
    for (command, json) in [
        (HostCommand::CheckForUpdate {}, r#"{"check_for_update":{}}"#),
        (
            HostCommand::InstallUpdate {
                when: InstallWhen::Now,
            },
            r#"{"install_update":{"when":"now"}}"#,
        ),
        (
            HostCommand::InstallUpdate {
                when: InstallWhen::AfterTurn,
            },
            r#"{"install_update":{"when":"after_turn"}}"#,
        ),
        (
            HostCommand::CancelScheduledUpdate {},
            r#"{"cancel_scheduled_update":{}}"#,
        ),
        (
            HostCommand::SetUpdateMode {
                mode: UpdateMode::Auto,
            },
            r#"{"set_update_mode":{"mode":"auto"}}"#,
        ),
        (
            HostCommand::SetUpdateMode {
                mode: UpdateMode::Notify,
            },
            r#"{"set_update_mode":{"mode":"notify"}}"#,
        ),
    ] {
        assert_eq!(serde_json::to_string(&command).unwrap(), json);
        round_trips(&command);
        assert!(command.validate().is_ok(), "{json} refused");
    }
}

/// `none` decodes (it is a value of `update.mode`) but a window may not set it: policy alone
/// does, so `validate()` refuses it as `command.invalid`.
#[test]
fn set_update_mode_none_is_refused_as_invalid() {
    let command = HostCommand::SetUpdateMode {
        mode: UpdateMode::None,
    };
    let error = HostError::invalid_command(command.validate().unwrap_err());
    assert_eq!(error.code(), ErrorCode::CommandInvalid);
    assert_eq!(
        error.message(),
        "SetUpdateMode: a window sets auto or notify; none comes only from policy"
    );
}

#[test]
fn an_unknown_mode_or_when_or_field_is_refused() {
    for body in [
        r#"{"set_update_mode":{"mode":"never"}}"#,
        r#"{"set_update_mode":{}}"#,
        r#"{"install_update":{"when":"later"}}"#,
        r#"{"install_update":{"when":"now","version":"0.4.1"}}"#,
        r#"{"check_for_update":{"now":true}}"#,
        r#""check_for_update""#,
    ] {
        let json = format!(r#"{{"command_id":"{}","command":{body}}}"#, command_id(1));
        assert!(
            serde_json::from_str::<CommandEnvelope>(&json).is_err(),
            "{body} decoded"
        );
    }
}
