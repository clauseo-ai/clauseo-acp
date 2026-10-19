//! Turns one JSON line into a header or a step. Bodies are serde structs that refuse unknown keys.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use super::{Expect, ExpectResponse, OnStdinClose, Reply, StepKind, Wait};

/// The step keys, in the order the crate docs list them.
const STEP_KEYS: &[&str] = &[
    "expect",
    "expect_response",
    "all",
    "respond",
    "update",
    "notify",
    "request",
    "send_line",
    "write_file",
    "delete_file",
    "move_file",
    "stderr",
    "spawn_child",
    "exit",
    "include",
];

/// A step, or an `include` the parser expands.
pub(super) enum Parsed {
    Step(StepKind),
    Include(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    scenario: String,
    description: String,
    #[serde(default)]
    on_stdin_close: Option<String>,
}

/// Parses the header line of scenario `name`.
pub(super) fn header(name: &str, value: Value) -> Result<OnStdinClose, String> {
    let header: Header = body("header", value)?;
    if header.scenario != name {
        return Err(format!(
            "header: scenario `{}` differs from the file stem `{name}`",
            header.scenario
        ));
    }
    if header.description.trim().is_empty() {
        return Err("header: description is empty".to_owned());
    }
    match header.on_stdin_close.as_deref() {
        None | Some("exit") => Ok(OnStdinClose::Exit),
        Some("hang") => Ok(OnStdinClose::Hang),
        Some(other) => Err(format!(
            "header: on_stdin_close is `{other}`, expected exit or hang"
        )),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectBody {
    method: String,
    #[serde(rename = "as")]
    label: Option<String>,
    params: Option<Value>,
    #[serde(default)]
    off: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectResponseBody {
    id: String,
    result: Option<Value>,
    error: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RespondBody {
    to: String,
    result: Option<Value>,
    error: Option<Value>,
    #[serde(default)]
    raw: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateBody {
    update: Value,
    session: Option<String>,
    #[serde(default)]
    raw: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NotifyBody {
    method: String,
    params: Option<Value>,
    #[serde(default)]
    raw: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestBody {
    id: String,
    method: String,
    params: Option<Value>,
    #[serde(default)]
    raw: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteFileBody {
    path: String,
    content: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathBody {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveFileBody {
    from: String,
    to: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StderrBody {
    line: String,
    #[serde(default = "one")]
    repeat: u32,
}

fn one() -> u32 {
    1
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpawnChildBody {
    pid_file: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExitBody {
    code: i64,
}

/// A step after parsing, with the `raw` flag that only the static checks read.
pub(super) struct Body {
    pub(super) kind: Parsed,
    pub(super) raw: bool,
}

/// Parses one step line.
pub(super) fn step(value: Value) -> Result<Body, String> {
    let Value::Object(mut map) = value else {
        return Err("a step is a JSON object".to_owned());
    };
    match map.remove("note") {
        None | Some(Value::String(_)) => {}
        Some(_) => return Err("note: expected a string".to_owned()),
    }
    let key = step_key(&map)?;
    let value = map.remove(&key).unwrap_or_default();
    let mut raw = false;
    let kind = match key.as_str() {
        "expect" => StepKind::Expect(expect(body(&key, value)?)?),
        "expect_response" => StepKind::ExpectResponse(expect_response(body(&key, value)?)?),
        "all" => StepKind::All(all(value)?),
        "respond" => {
            let b: RespondBody = body(&key, value)?;
            raw = b.raw;
            StepKind::Respond {
                to: b.to,
                reply: reply(&key, b.result, b.error)?,
            }
        }
        "update" => {
            let b: UpdateBody = body(&key, value)?;
            raw = b.raw;
            StepKind::Update {
                update: b.update,
                session: b.session,
            }
        }
        "notify" => {
            let b: NotifyBody = body(&key, value)?;
            raw = b.raw;
            StepKind::Notify {
                method: b.method,
                params: b.params,
            }
        }
        "request" => {
            let b: RequestBody = body(&key, value)?;
            raw = b.raw;
            StepKind::Request {
                id: b.id,
                method: b.method,
                params: b.params,
            }
        }
        "send_line" => StepKind::SendLine(body(&key, value)?),
        "write_file" => {
            let b: WriteFileBody = body(&key, value)?;
            StepKind::WriteFile {
                path: b.path,
                content: b.content,
            }
        }
        "delete_file" => StepKind::DeleteFile {
            path: body::<PathBody>(&key, value)?.path,
        },
        "move_file" => {
            let b: MoveFileBody = body(&key, value)?;
            StepKind::MoveFile {
                from: b.from,
                to: b.to,
            }
        }
        "stderr" => {
            let b: StderrBody = body(&key, value)?;
            StepKind::Stderr {
                line: b.line,
                repeat: b.repeat,
            }
        }
        "spawn_child" => StepKind::SpawnChild {
            pid_file: body::<SpawnChildBody>(&key, value)?.pid_file,
        },
        "exit" => {
            let code = body::<ExitBody>(&key, value)?.code;
            // Unix keeps only the low 8 bits of an exit code, so refuse what it would change.
            match i32::try_from(code) {
                Ok(code) if (0..=255).contains(&code) => StepKind::Exit { code },
                _ => return Err(format!("exit: code {code} is outside 0..=255")),
            }
        }
        "include" => {
            return Ok(Body {
                kind: Parsed::Include(body(&key, value)?),
                raw,
            });
        }
        _ => return Err(format!("unknown step `{key}`")),
    };
    Ok(Body {
        kind: Parsed::Step(kind),
        raw,
    })
}

/// The one step key in `map`, which has had `note` removed.
fn step_key(map: &Map<String, Value>) -> Result<String, String> {
    let mut keys = map.keys();
    match (keys.next(), keys.next()) {
        (None, _) => Err(format!("a step needs one of: {}", STEP_KEYS.join(", "))),
        (Some(first), Some(second)) => Err(format!(
            "a step has exactly one step key, found `{first}` and `{second}`"
        )),
        (Some(key), None) if STEP_KEYS.contains(&key.as_str()) => Ok(key.clone()),
        (Some(key), None) => Err(format!(
            "unknown step `{key}`; expected one of: {}",
            STEP_KEYS.join(", ")
        )),
    }
}

fn body<T: DeserializeOwned>(key: &str, value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| format!("{key}: {error}"))
}

fn expect(b: ExpectBody) -> Result<Expect, String> {
    if let Some(bad) = b.off.iter().find(|pointer| !pointer.starts_with('/')) {
        return Err(format!("expect: off pointer `{bad}` must start with /"));
    }
    Ok(Expect {
        method: b.method,
        label: b.label,
        params: b.params,
        off: b.off,
    })
}

fn expect_response(b: ExpectResponseBody) -> Result<ExpectResponse, String> {
    Ok(ExpectResponse {
        id: b.id,
        reply: reply("expect_response", b.result, b.error)?,
    })
}

fn reply(key: &str, result: Option<Value>, error: Option<Value>) -> Result<Reply, String> {
    match (result, error) {
        (Some(result), None) => Ok(Reply::Result(result)),
        (None, Some(error)) => Ok(Reply::Error(error)),
        _ => Err(format!("{key}: needs exactly one of `result` and `error`")),
    }
}

/// An `all` list: each item is `{"expect": {...}}` or `{"expect_response": {...}}`.
fn all(value: Value) -> Result<Vec<Wait>, String> {
    let Value::Array(items) = value else {
        return Err("all: expected a list".to_owned());
    };
    if items.is_empty() {
        return Err("all: the list is empty".to_owned());
    }
    items
        .into_iter()
        .map(|item| {
            let Value::Object(mut map) = item else {
                return Err(
                    "all: each item is {\"expect\": ...} or {\"expect_response\": ...}".to_owned(),
                );
            };
            let key = step_key(&map)?;
            let value = map.remove(&key).unwrap_or_default();
            match key.as_str() {
                "expect" => Ok(Wait::Expect(expect(body(&key, value)?)?)),
                "expect_response" => Ok(Wait::Response(expect_response(body(&key, value)?)?)),
                _ => Err(format!(
                    "all: `{key}` cannot wait; use expect or expect_response"
                )),
            }
        })
        .collect()
}
