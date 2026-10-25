//! Turns one JSON line into a header or a step. A step is an externally tagged serde enum, so its
//! one key picks the variant, and every body is a struct that refuses unknown keys.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use super::{Expect, ExpectResponse, FileOp, OnStdinClose, Reply, StepKind, Wait};
use crate::{EXIT_LOAD, EXIT_MISMATCH};

/// The most times one `stderr` step may repeat its line. Well above every bundled scenario (200)
/// and the largest performance workload (5,000), and small enough that the one buffer the step
/// fills stays a few megabytes.
pub(crate) const MAX_REPEAT: u32 = 100_000;

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
    on_stdin_close: OnStdinClose,
}

/// Parses the header line of scenario `name`.
pub(super) fn header(name: &str, value: Value) -> Result<OnStdinClose, String> {
    let header: Header =
        serde_json::from_value(value).map_err(|error| format!("header: {error}"))?;
    if header.scenario != name {
        return Err(format!(
            "header: scenario `{}` differs from the file stem `{name}`",
            header.scenario
        ));
    }
    if header.description.trim().is_empty() {
        return Err("header: description is empty".to_owned());
    }
    Ok(header.on_stdin_close)
}

/// One step line without its `note`: exactly one of these keys.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum StepBody {
    Expect(ExpectBody),
    ExpectResponse(ExpectResponseBody),
    All(Vec<Map<String, Value>>),
    Respond(RespondBody),
    Update(UpdateBody),
    Notify(NotifyBody),
    Request(RequestBody),
    SendLine(String),
    WriteFile(WriteFileBody),
    DeleteFile(PathBody),
    MoveFile(MoveFileBody),
    Stderr(StderrBody),
    SpawnChild(SpawnChildBody),
    Exit(ExitBody),
    Include(String),
}

/// One item of an `all` list.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum WaitBody {
    Expect(ExpectBody),
    ExpectResponse(ExpectResponseBody),
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
    let (kind, raw) = match tagged::<StepBody>(map, "unknown step")? {
        StepBody::Expect(b) => (StepKind::Wait(Wait::Expect(expect(b)?)), false),
        StepBody::ExpectResponse(b) => (StepKind::Wait(Wait::Response(expect_response(b)?)), false),
        StepBody::All(items) => (StepKind::All(all(items)?), false),
        StepBody::Respond(b) => {
            let reply = reply("respond", b.result, b.error)?;
            (StepKind::Respond { to: b.to, reply }, b.raw)
        }
        StepBody::Update(b) => {
            let (update, session) = (b.update, b.session);
            (StepKind::Update { update, session }, b.raw)
        }
        StepBody::Notify(b) => {
            let (method, params) = (b.method, b.params);
            (StepKind::Notify { method, params }, b.raw)
        }
        StepBody::Request(b) => {
            let (id, method, params) = (b.id, b.method, b.params);
            (StepKind::Request { id, method, params }, b.raw)
        }
        StepBody::SendLine(text) => (StepKind::SendLine(text), false),
        StepBody::WriteFile(b) => {
            let (path, content) = (b.path, b.content);
            (StepKind::File(FileOp::Write { path, content }), false)
        }
        StepBody::DeleteFile(b) => (StepKind::File(FileOp::Delete { path: b.path }), false),
        StepBody::MoveFile(b) => {
            let (from, to) = (b.from, b.to);
            (StepKind::File(FileOp::Move { from, to }), false)
        }
        StepBody::Stderr(b) => (stderr(b)?, false),
        StepBody::SpawnChild(b) => (
            StepKind::SpawnChild {
                pid_file: b.pid_file,
            },
            false,
        ),
        StepBody::Exit(b) => (exit(b.code)?, false),
        StepBody::Include(file) => {
            return Ok(Body {
                kind: Parsed::Include(file),
                raw: false,
            });
        }
    };
    Ok(Body {
        kind: Parsed::Step(kind),
        raw,
    })
}

/// Deserialises externally tagged enum `T` from `map`, which must hold exactly one of its keys.
/// A wrong key gets `unknown` and the valid keys; an error in the body names its key.
fn tagged<T: DeserializeOwned>(map: Map<String, Value>, unknown: &str) -> Result<T, String> {
    // Sorted, so the message is the same whether serde_json keeps key order or not.
    let mut keys: Vec<String> = map.keys().cloned().collect();
    keys.sort();
    serde_json::from_value(Value::Object(map)).map_err(|error| {
        let valid = keys::variants::<T>();
        match keys.as_slice() {
            [key] if valid.contains(&key.as_str()) => format!("{key}: {error}"),
            [key] => format!("{unknown} `{key}`; expected one of: {}", valid.join(", ")),
            [] => format!("a step needs one of: {}", valid.join(", ")),
            [first, second, ..] => format!(
                "a step has exactly one step key, found `{first}` and `{second}`; expected one \
                 of: {}",
                valid.join(", ")
            ),
        }
    })
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
fn all(items: Vec<Map<String, Value>>) -> Result<Vec<Wait>, String> {
    if items.is_empty() {
        return Err("all: the list is empty".to_owned());
    }
    items
        .into_iter()
        .map(|item| {
            match tagged::<WaitBody>(item, "cannot wait on").map_err(|m| format!("all: {m}"))? {
                WaitBody::Expect(b) => Ok(Wait::Expect(expect(b)?)),
                WaitBody::ExpectResponse(b) => Ok(Wait::Response(expect_response(b)?)),
            }
        })
        .collect()
}

fn stderr(b: StderrBody) -> Result<StepKind, String> {
    if b.repeat > MAX_REPEAT {
        return Err(format!(
            "stderr: repeat {} is over the cap of {MAX_REPEAT}",
            b.repeat
        ));
    }
    Ok(StepKind::Stderr {
        line: b.line,
        repeat: b.repeat,
    })
}

fn exit(code: i64) -> Result<StepKind, String> {
    let reserved = |name: &str| {
        format!(
            "exit: code {code} is {name}, the code fake_agent itself exits with; an exit step may \
             not use {EXIT_LOAD} (EXIT_LOAD) or {EXIT_MISMATCH} (EXIT_MISMATCH)"
        )
    };
    // Unix keeps only the low 8 bits of an exit code, so refuse what it would change.
    match i32::try_from(code) {
        Ok(EXIT_LOAD) => Err(reserved("EXIT_LOAD")),
        Ok(EXIT_MISMATCH) => Err(reserved("EXIT_MISMATCH")),
        Ok(code) if (0..=255).contains(&code) => Ok(StepKind::Exit { code }),
        _ => Err(format!("exit: code {code} is outside 0..=255")),
    }
}

/// The variant names serde gives an externally tagged enum, read from its derived
/// `Deserialize`, so no list of step keys is kept by hand.
mod keys {
    use std::fmt;

    use serde::de::{self, Deserialize, Deserializer, Visitor};

    /// A deserializer that only reports the variant names it is asked for.
    struct Probe;

    /// The variants a `deserialize_enum` call named; empty for anything else.
    #[derive(Debug)]
    struct Found(&'static [&'static str]);

    impl fmt::Display for Found {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "variants {:?}", self.0)
        }
    }

    impl std::error::Error for Found {}

    impl de::Error for Found {
        fn custom<T: fmt::Display>(_: T) -> Self {
            Found(&[])
        }
    }

    impl<'de> Deserializer<'de> for Probe {
        type Error = Found;

        fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Found> {
            Err(Found(&[]))
        }

        fn deserialize_enum<V: Visitor<'de>>(
            self,
            _: &'static str,
            variants: &'static [&'static str],
            _: V,
        ) -> Result<V::Value, Found> {
            Err(Found(variants))
        }

        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
            option unit unit_struct newtype_struct seq tuple tuple_struct map struct identifier
            ignored_any
        }
    }

    /// The keys externally tagged enum `T` accepts, in declaration order.
    pub(super) fn variants<'de, T: Deserialize<'de>>() -> &'static [&'static str] {
        T::deserialize(Probe).err().map_or(&[], |found| found.0)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn found_displays_the_variants_it_holds() {
            assert_eq!(Found(&["a", "b"]).to_string(), r#"variants ["a", "b"]"#);
        }
    }
}
