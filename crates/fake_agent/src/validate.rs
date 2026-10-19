//! Checks every message a scenario sends against the SDK's typed v1 schema.
//!
//! Deserialising alone is not enough: `ToolKind` has a catch-all `Other` and many capability fields
//! fall back to a default on error, so `"kind":"edit_file"` would deserialise. So each message is
//! deserialised into the concrete type for its method, serialised again, and every key and value
//! the scenario wrote must be present and equal in the result. Extra default keys are fine.

use agent_client_protocol::schema::v1 as acp;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::pattern::{child, scalar_eq, short};

/// What a step sends, as validation needs to see it.
pub(crate) enum Sent<'a> {
    /// The `update` object of a `session/update`.
    Update(&'a Value),
    /// A request from the agent.
    Request {
        method: &'a str,
        params: Option<&'a Value>,
    },
    /// A notification from the agent.
    Notify {
        method: &'a str,
        params: Option<&'a Value>,
    },
    /// The result of a response to the client's request `method`.
    Result { method: &'a str, result: &'a Value },
    /// The error of a response.
    Error(&'a Value),
}

/// Why a sent message failed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Problem {
    /// The SDK has no stable type for this method; the step must carry `"raw": true`.
    UnknownMethod,
    /// The message does not survive the schema; the text names the first pointer that differs.
    Invalid(String),
}

type Check = fn(&Value, &str) -> Result<(), String>;

/// Validates one sent message. Its pointers start at `/update`, `/params`, `/result` or `/error`.
pub(crate) fn validate(sent: &Sent<'_>) -> Result<(), Problem> {
    let null = Value::Null;
    let (check, value, base): (Option<Check>, &Value, &str) = match *sent {
        Sent::Update(update) => (Some(survives::<acp::SessionUpdate>), update, "/update"),
        Sent::Request { method, params } => {
            (request_check(method), params.unwrap_or(&null), "/params")
        }
        Sent::Notify { method, params } => {
            (notify_check(method), params.unwrap_or(&null), "/params")
        }
        Sent::Result { method, result } => (result_check(method), result, "/result"),
        Sent::Error(error) => (Some(survives::<acp::Error>), error, "/error"),
    };
    let check = check.ok_or(Problem::UnknownMethod)?;
    check(value, base).map_err(Problem::Invalid)
}

/// The response type of each stable client-to-agent request.
fn result_check(method: &str) -> Option<Check> {
    Some(match method {
        "initialize" => survives::<acp::InitializeResponse>,
        "authenticate" => survives::<acp::AuthenticateResponse>,
        "logout" => survives::<acp::LogoutResponse>,
        "session/new" => survives::<acp::NewSessionResponse>,
        "session/load" => survives::<acp::LoadSessionResponse>,
        "session/resume" => survives::<acp::ResumeSessionResponse>,
        "session/close" => survives::<acp::CloseSessionResponse>,
        "session/list" => survives::<acp::ListSessionsResponse>,
        "session/delete" => survives::<acp::DeleteSessionResponse>,
        "session/set_mode" => survives::<acp::SetSessionModeResponse>,
        "session/set_config_option" => survives::<acp::SetSessionConfigOptionResponse>,
        "session/prompt" => survives::<acp::PromptResponse>,
        _ => return None,
    })
}

/// The params type of each stable agent-to-client request.
fn request_check(method: &str) -> Option<Check> {
    Some(match method {
        "session/request_permission" => survives::<acp::RequestPermissionRequest>,
        "fs/read_text_file" => survives::<acp::ReadTextFileRequest>,
        "fs/write_text_file" => survives::<acp::WriteTextFileRequest>,
        "terminal/create" => survives::<acp::CreateTerminalRequest>,
        "terminal/output" => survives::<acp::TerminalOutputRequest>,
        "terminal/release" => survives::<acp::ReleaseTerminalRequest>,
        "terminal/wait_for_exit" => survives::<acp::WaitForTerminalExitRequest>,
        "terminal/kill" => survives::<acp::KillTerminalRequest>,
        "elicitation/create" => survives::<acp::CreateElicitationRequest>,
        _ => return None,
    })
}

/// The params type of each stable notification the agent may send.
fn notify_check(method: &str) -> Option<Check> {
    Some(match method {
        "session/update" => survives::<acp::SessionNotification>,
        "elicitation/complete" => survives::<acp::CompleteElicitationNotification>,
        "$/cancel_request" => survives::<acp::CancelRequestNotification>,
        _ => return None,
    })
}

/// Deserialises `value` as `T`, serialises it again, and requires `value` to be contained in it.
fn survives<T: DeserializeOwned + Serialize>(value: &Value, base: &str) -> Result<(), String> {
    let typed = T::deserialize(value).map_err(|error| format!("{base}: {error}"))?;
    let back = serde_json::to_value(&typed).map_err(|error| format!("{base}: {error}"))?;
    contained(value, &back, base)
}

/// Whether every key and value of `sent` is present and equal in `back`. A `null` in `sent` may be
/// missing in `back`, since the SDK skips `None` fields.
fn contained(sent: &Value, back: &Value, pointer: &str) -> Result<(), String> {
    let lost = |became: &str| {
        format!(
            "{pointer}: {} does not survive the SDK schema (became {became})",
            short(sent)
        )
    };
    match (sent, back) {
        (Value::Object(fields), Value::Object(back_fields)) => {
            for (key, value) in fields {
                match back_fields.get(key) {
                    Some(got) => contained(value, got, &child(pointer, key))?,
                    None => dropped(key, value, &child(pointer, key))?,
                }
            }
            Ok(())
        }
        (Value::Array(items), Value::Array(back_items)) if items.len() == back_items.len() => {
            for (index, (item, got)) in items.iter().zip(back_items).enumerate() {
                contained(item, got, &child(pointer, &index.to_string()))?;
            }
            Ok(())
        }
        _ if scalar_eq(sent, back) => Ok(()),
        _ => Err(lost(&short(back))),
    }
}

/// Fields the SDK leaves out when they hold their default, with that default as JSON:
/// `ToolCall.kind` and `ToolCall.status`. `defaults_match_the_sdk` checks the values.
const SKIPPED_DEFAULTS: &[(&str, &str)] = &[("kind", "other"), ("status", "pending")];

/// Whether leaving out key `key` with value `value` loses nothing. The SDK skips `None`, empty
/// lists and maps, and the fields in [`SKIPPED_DEFAULTS`] when they hold their default; a value
/// that is none of those was refused or replaced.
fn dropped(key: &str, value: &Value, pointer: &str) -> Result<(), String> {
    let empty = match value {
        Value::Null => true,
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        _ => false,
    };
    if empty {
        return Ok(());
    }
    match SKIPPED_DEFAULTS.iter().find(|(name, _)| *name == key) {
        Some((_, default)) if value.as_str() == Some(*default) => Ok(()),
        Some((_, default)) => Err(format!(
            "{pointer}: {} does not survive the SDK schema (became \"{default}\")",
            short(value)
        )),
        None => Err(format!(
            "{pointer}: {} does not survive the SDK schema (dropped)",
            short(value)
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_match_the_sdk() {
        let kind = serde_json::to_value(acp::ToolKind::default()).unwrap();
        let status = serde_json::to_value(acp::ToolCallStatus::default()).unwrap();
        assert_eq!(
            SKIPPED_DEFAULTS,
            &[
                ("kind", kind.as_str().unwrap()),
                ("status", status.as_str().unwrap())
            ]
        );
        let update = json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Look",
            "kind": "other", "status": "pending", "content": [], "unknownKey": {}});
        assert_eq!(validate(&Sent::Update(&update)), Ok(()));
        let update = json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Look",
            "extra": "x"});
        assert_eq!(
            validate(&Sent::Update(&update)),
            Err(Problem::Invalid(
                r#"/update/extra: "x" does not survive the SDK schema (dropped)"#.to_owned()
            ))
        );
    }

    #[test]
    fn edit_file_kind_does_not_survive() {
        let update = json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Edit",
            "kind": "edit_file"});
        assert_eq!(
            validate(&Sent::Update(&update)),
            Err(Problem::Invalid(
                r#"/update/kind: "edit_file" does not survive the SDK schema (became "other")"#
                    .to_owned()
            ))
        );
        let good = json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Edit",
            "kind": "edit"});
        assert_eq!(validate(&Sent::Update(&good)), Ok(()));
    }

    #[test]
    fn unknown_variant_and_method_fail() {
        let update = json!({"sessionUpdate": "_clauseo_test_unknown"});
        assert!(matches!(
            validate(&Sent::Update(&update)),
            Err(Problem::Invalid(_))
        ));
        let result = json!({"outcome": "injected"});
        assert_eq!(
            validate(&Sent::Result {
                method: "_session/steering",
                result: &result
            }),
            Err(Problem::UnknownMethod)
        );
        let error = json!({"code": -32000, "message": "Authentication required"});
        assert_eq!(validate(&Sent::Error(&error)), Ok(()));
    }

    /// Every stable method name the SDK lists has a type here, so a new SDK method is noticed.
    #[test]
    fn every_stable_method_has_a_type() {
        let agent = acp::AGENT_METHOD_NAMES;
        let requests = [
            agent.initialize,
            agent.authenticate,
            agent.logout,
            agent.session_new,
            agent.session_load,
            agent.session_resume,
            agent.session_close,
            agent.session_list,
            agent.session_delete,
            agent.session_set_mode,
            agent.session_set_config_option,
            agent.session_prompt,
        ];
        for method in requests {
            assert!(result_check(method).is_some(), "{method}");
        }
        let client = acp::CLIENT_METHOD_NAMES;
        let agent_requests = [
            client.session_request_permission,
            client.fs_read_text_file,
            client.fs_write_text_file,
            client.terminal_create,
            client.terminal_output,
            client.terminal_release,
            client.terminal_wait_for_exit,
            client.terminal_kill,
            client.elicitation_create,
        ];
        for method in agent_requests {
            assert!(request_check(method).is_some(), "{method}");
        }
        let notifications = [
            client.session_update,
            client.elicitation_complete,
            acp::PROTOCOL_LEVEL_METHOD_NAMES.cancel_request,
        ];
        for method in notifications {
            assert!(notify_check(method).is_some(), "{method}");
        }
    }
}
