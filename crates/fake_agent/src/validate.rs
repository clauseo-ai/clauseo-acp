//! Checks every message a scenario sends against the SDK's typed v1 schema.
//!
//! Deserialising alone is not enough: `ToolKind` has a catch-all `Other` and many capability fields
//! fall back to a default on error, so `"kind":"edit_file"` would deserialise. So each message is
//! deserialised into the concrete type for its method, serialised again, and every key and value
//! the scenario wrote must be present and equal in the result. Extra default keys are fine.
//!
//! The one exception is a key the SDK leaves out. [`SKIPPED_DEFAULTS`] and [`SKIPPED_WHEN_EMPTY`]
//! list each such key by where it sits: its method, its JSON pointer and, where only one variant
//! of a type has it, the value that variant carries. A key anywhere else is refused.

use agent_client_protocol::schema::v1 as acp;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

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

/// Validates one value against the type for its method: the value, the pointer it starts at
/// (`/update`, `/params`, `/result` or `/error`), and the method it belongs to.
type Check = fn(&Value, &str, &str) -> Result<(), String>;

/// Validates one sent message. Its pointers start at `/update`, `/params`, `/result` or `/error`.
pub(crate) fn validate(sent: &Sent<'_>) -> Result<(), Problem> {
    let null = Value::Null;
    let (check, value, base, method): (Option<Check>, &Value, &str, &str) = match *sent {
        Sent::Update(update) => (
            Some(survives::<acp::SessionUpdate>),
            update,
            "/update",
            SESSION_UPDATE,
        ),
        Sent::Request { method, params } => (
            request_check(method),
            params.unwrap_or(&null),
            "/params",
            method,
        ),
        Sent::Notify { method, params } => (
            notify_check(method),
            params.unwrap_or(&null),
            "/params",
            method,
        ),
        Sent::Result { method, result } => (result_check(method), result, "/result", method),
        Sent::Error(error) => (Some(survives::<acp::Error>), error, "/error", ""),
    };
    let check = check.ok_or(Problem::UnknownMethod)?;
    check(value, base, method).map_err(Problem::Invalid)
}

/// Looks `method` up in a table of method names and their checks.
fn lookup(table: &[(&str, Check)], method: &str) -> Option<Check> {
    table
        .iter()
        .find(|(name, _)| *name == method)
        .map(|(_, check)| *check)
}

/// The response type of each stable client-to-agent request, keyed on the SDK's method name.
const RESULT_CHECKS: &[(&str, Check)] = &[
    (
        acp::AGENT_METHOD_NAMES.initialize,
        survives::<acp::InitializeResponse>,
    ),
    (
        acp::AGENT_METHOD_NAMES.authenticate,
        survives::<acp::AuthenticateResponse>,
    ),
    (
        acp::AGENT_METHOD_NAMES.logout,
        survives::<acp::LogoutResponse>,
    ),
    (
        acp::AGENT_METHOD_NAMES.session_new,
        survives::<acp::NewSessionResponse>,
    ),
    (
        acp::AGENT_METHOD_NAMES.session_load,
        survives::<acp::LoadSessionResponse>,
    ),
    (
        acp::AGENT_METHOD_NAMES.session_resume,
        survives::<acp::ResumeSessionResponse>,
    ),
    (
        acp::AGENT_METHOD_NAMES.session_close,
        survives::<acp::CloseSessionResponse>,
    ),
    (
        acp::AGENT_METHOD_NAMES.session_list,
        survives::<acp::ListSessionsResponse>,
    ),
    (
        acp::AGENT_METHOD_NAMES.session_delete,
        survives::<acp::DeleteSessionResponse>,
    ),
    (
        acp::AGENT_METHOD_NAMES.session_set_mode,
        survives::<acp::SetSessionModeResponse>,
    ),
    (
        acp::AGENT_METHOD_NAMES.session_set_config_option,
        survives::<acp::SetSessionConfigOptionResponse>,
    ),
    (
        acp::AGENT_METHOD_NAMES.session_prompt,
        survives::<acp::PromptResponse>,
    ),
];

/// The response type of each stable client-to-agent request.
fn result_check(method: &str) -> Option<Check> {
    lookup(RESULT_CHECKS, method)
}

/// The params type of each stable agent-to-client request, keyed on the SDK's method name.
const REQUEST_CHECKS: &[(&str, Check)] = &[
    (
        acp::CLIENT_METHOD_NAMES.session_request_permission,
        survives::<acp::RequestPermissionRequest>,
    ),
    (
        acp::CLIENT_METHOD_NAMES.fs_read_text_file,
        survives::<acp::ReadTextFileRequest>,
    ),
    (
        acp::CLIENT_METHOD_NAMES.fs_write_text_file,
        survives::<acp::WriteTextFileRequest>,
    ),
    (
        acp::CLIENT_METHOD_NAMES.terminal_create,
        survives::<acp::CreateTerminalRequest>,
    ),
    (
        acp::CLIENT_METHOD_NAMES.terminal_output,
        survives::<acp::TerminalOutputRequest>,
    ),
    (
        acp::CLIENT_METHOD_NAMES.terminal_release,
        survives::<acp::ReleaseTerminalRequest>,
    ),
    (
        acp::CLIENT_METHOD_NAMES.terminal_wait_for_exit,
        survives::<acp::WaitForTerminalExitRequest>,
    ),
    (
        acp::CLIENT_METHOD_NAMES.terminal_kill,
        survives::<acp::KillTerminalRequest>,
    ),
    (
        acp::CLIENT_METHOD_NAMES.elicitation_create,
        survives::<acp::CreateElicitationRequest>,
    ),
];

/// The params type of each stable agent-to-client request.
fn request_check(method: &str) -> Option<Check> {
    lookup(REQUEST_CHECKS, method)
}

/// The params type of each stable notification the agent may send, keyed on the SDK's method name.
const NOTIFY_CHECKS: &[(&str, Check)] = &[
    (
        acp::CLIENT_METHOD_NAMES.session_update,
        survives::<acp::SessionNotification>,
    ),
    (
        acp::CLIENT_METHOD_NAMES.elicitation_complete,
        survives::<acp::CompleteElicitationNotification>,
    ),
    (
        acp::PROTOCOL_LEVEL_METHOD_NAMES.cancel_request,
        survives::<acp::CancelRequestNotification>,
    ),
];

/// The params type of each stable notification the agent may send.
fn notify_check(method: &str) -> Option<Check> {
    lookup(NOTIFY_CHECKS, method)
}

/// Deserialises `value` as `T`, serialises it again, and requires `value` to be contained in it.
fn survives<T: DeserializeOwned + Serialize>(
    value: &Value,
    base: &str,
    method: &str,
) -> Result<(), String> {
    let typed = T::deserialize(value).map_err(|error| format!("{base}: {error}"))?;
    let back = serde_json::to_value(&typed).map_err(|error| format!("{base}: {error}"))?;
    contained(value, &back, base, method)
}

/// Whether every key and value of `sent` is present and equal in `back`. A key of `sent` may be
/// missing in `back` only where [`dropped`] allows it: an empty value or a default, at a place
/// where the SDK leaves it out. `method` is the method of the message.
fn contained(sent: &Value, back: &Value, pointer: &str, method: &str) -> Result<(), String> {
    let lost = |became: &str| {
        format!(
            "{pointer}: {} does not survive the SDK schema (became {became})",
            short(sent)
        )
    };
    match (sent, back) {
        (Value::Object(fields), Value::Object(back_fields)) => {
            for (key, value) in fields {
                let at = child(pointer, key);
                match back_fields.get(key) {
                    Some(got) => contained(value, got, &at, method)?,
                    None => dropped(fields, value, &at, method)?,
                }
            }
            Ok(())
        }
        // Equal lengths only. Zipping unequal arrays would check the common prefix and pass.
        (Value::Array(items), Value::Array(back_items)) if items.len() == back_items.len() => {
            for (index, (item, got)) in items.iter().zip(back_items).enumerate() {
                contained(item, got, &child(pointer, &index.to_string()), method)?;
            }
            Ok(())
        }
        (Value::Array(items), Value::Array(back_items)) => Err(format!(
            "{pointer}: {} items do not survive the SDK schema (became {})",
            items.len(),
            back_items.len()
        )),
        _ if scalar_eq(sent, back) => Ok(()),
        _ => Err(lost(&short(back))),
    }
}

/// The method of the `session/update` notification, which carries the `update` of a session update.
const SESSION_UPDATE: &str = acp::CLIENT_METHOD_NAMES.session_update;
/// The method whose result carries the `authMethods`.
const INITIALIZE: &str = acp::AGENT_METHOD_NAMES.initialize;
/// The method whose params are a `CreateTerminalRequest`.
const TERMINAL_CREATE: &str = acp::CLIENT_METHOD_NAMES.terminal_create;
/// The condition that a session update is a `tool_call`, the only variant with these fields.
const TOOL_CALL: Option<(&str, &str)> = Some(("sessionUpdate", "tool_call"));
/// The condition that an auth method is `terminal`, the only variant with `args` and `env`.
const TERMINAL: Option<(&str, &str)> = Some(("type", "terminal"));

/// Where the SDK leaves a key out of a message: the method of the message, the JSON pointer of
/// the key, and the `(key, value)` the object holding the key must carry, if only some variants
/// of its type have the key.
///
/// The pointer starts at `/update`, `/params` or `/result` as in [`validate`]. A `*` segment
/// matches any array index.
struct Place {
    method: &'static str,
    pointer: &'static str,
    within: Option<(&'static str, &'static str)>,
}

impl Place {
    /// Whether a key at `pointer`, of a `method` message whose object is `parent`, is this place.
    fn holds(&self, method: &str, pointer: &str, parent: &Map<String, Value>) -> bool {
        self.method == method
            && pointer_matches(self.pointer, pointer)
            && self
                .within
                .is_none_or(|(key, value)| parent.get(key).and_then(Value::as_str) == Some(value))
    }
}

/// Whether JSON pointer `pointer` is `pattern`, where a `*` segment matches an array index.
fn pointer_matches(pattern: &str, pointer: &str) -> bool {
    let (pattern, pointer): (Vec<&str>, Vec<&str>) =
        (pattern.split('/').collect(), pointer.split('/').collect());
    pattern.len() == pointer.len()
        && pattern.iter().zip(&pointer).all(|(want, got)| {
            want == got
                || (*want == "*" && !got.is_empty() && got.bytes().all(|b| b.is_ascii_digit()))
        })
}

/// Keys the SDK leaves out when they hold their default, with that default as JSON: `kind` and
/// `status` of a `ToolCall`, in a session update and in the `session/update` notification.
/// `defaults_match_the_sdk` checks each default and that the SDK really leaves the key out.
const SKIPPED_DEFAULTS: &[(Place, &str)] = &[
    (
        Place {
            method: SESSION_UPDATE,
            pointer: "/update/kind",
            within: TOOL_CALL,
        },
        "other",
    ),
    (
        Place {
            method: SESSION_UPDATE,
            pointer: "/update/status",
            within: TOOL_CALL,
        },
        "pending",
    ),
    (
        Place {
            method: SESSION_UPDATE,
            pointer: "/params/update/kind",
            within: TOOL_CALL,
        },
        "other",
    ),
    (
        Place {
            method: SESSION_UPDATE,
            pointer: "/params/update/status",
            within: TOOL_CALL,
        },
        "pending",
    ),
];

/// Keys the SDK leaves out when they are empty: `content` and `locations` of a `ToolCall`, in a
/// session update and in the `session/update` notification; `args` and `env` of a terminal auth
/// method in an `initialize` result; and `args` and `env` of a `terminal/create` request.
/// `skipped_when_empty_matches_the_sdk` proves each is really left out.
const SKIPPED_WHEN_EMPTY: &[Place] = &[
    Place {
        method: SESSION_UPDATE,
        pointer: "/update/content",
        within: TOOL_CALL,
    },
    Place {
        method: SESSION_UPDATE,
        pointer: "/update/locations",
        within: TOOL_CALL,
    },
    Place {
        method: SESSION_UPDATE,
        pointer: "/params/update/content",
        within: TOOL_CALL,
    },
    Place {
        method: SESSION_UPDATE,
        pointer: "/params/update/locations",
        within: TOOL_CALL,
    },
    Place {
        method: INITIALIZE,
        pointer: "/result/authMethods/*/args",
        within: TERMINAL,
    },
    Place {
        method: INITIALIZE,
        pointer: "/result/authMethods/*/env",
        within: TERMINAL,
    },
    Place {
        method: TERMINAL_CREATE,
        pointer: "/params/args",
        within: None,
    },
    Place {
        method: TERMINAL_CREATE,
        pointer: "/params/env",
        within: None,
    },
];

/// Whether leaving out the key at `pointer`, with `value` and in the object `parent`, of a
/// `method` message loses nothing. It may when the value is empty (`null`, `[]`, `{}`) and the
/// place is in [`SKIPPED_WHEN_EMPTY`], or when it holds the default and the place is in
/// [`SKIPPED_DEFAULTS`]. Anything else was refused or replaced, so it fails.
fn dropped(
    parent: &Map<String, Value>,
    value: &Value,
    pointer: &str,
    method: &str,
) -> Result<(), String> {
    let empty = match value {
        Value::Null => true,
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        _ => false,
    };
    if empty
        && SKIPPED_WHEN_EMPTY
            .iter()
            .any(|place| place.holds(method, pointer, parent))
    {
        return Ok(());
    }
    match SKIPPED_DEFAULTS
        .iter()
        .find(|(place, _)| place.holds(method, pointer, parent))
    {
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
mod tests;
