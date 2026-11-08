//! The set-up pipe's messages (0049, "The set-up task"): one request from the app, one reply
//! from the server, each a JSON object with the protocol's version `v` and a tag naming what it
//! is, so a later version adds ops and replies without changing these. No Windows call; tested
//! on every OS.
//!
//! Requests, tagged `op`:
//!
//! - `{"v":1,"op":"set_up"}`: set up, or re-run, the asking user's pool;
//! - `{"v":1,"op":"add","n":<1..32>}`: extend it by `n` users;
//! - `{"v":1,"op":"ensure_slots","want":<n>}`: reserved for several owners on one machine, which
//!   this server answers `unsupported`;
//! - any other op, or another `v`, is answered `unsupported`, never refused as malformed.
//!
//! Replies, tagged `reply`:
//!
//! - `{"v":1,"reply":"ran","result":{...}}`: the set-up's outcome (`setup-result.json`'s shape),
//!   `exit` 0 when it was current and nothing ran;
//! - `{"v":1,"reply":"refused","why":"..."}`: refused before anything ran;
//! - `{"v":1,"reply":"unsupported","what":"..."}`: an op or version this server doesn't serve.
//!
//! A reader ignores fields it doesn't know, so a later reply may add some (a slot list).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::super::result_file::{self, SetupResult};
use crate::SetupError;
use crate::windows::pool::POOL_CAP;

/// The protocol's version, in every request and reply.
pub const PROTOCOL: u32 = 1;
/// The most bytes a request may hold.
pub const REQUEST_CAP: usize = 256;
/// The most bytes a reply may hold, for the app's read.
pub const REPLY_CAP: usize = 64 * 1024;

/// What the client asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// `set_up`: a set-up or re-run.
    SetUp,
    /// `add`: an extend by this many users, 1 to [`POOL_CAP`].
    Add(u8),
    /// `ensure_slots`: reserved for several owners on one machine (the multi-owner work); this
    /// server answers it `unsupported`.
    EnsureSlots(u32),
    /// An op or a version this server doesn't know: what, for the `unsupported` reply.
    Unsupported(String),
}

impl Op {
    /// The request in `bytes`: a JSON object of at most [`REQUEST_CAP`] bytes with a number `v`
    /// and a string `op`. Another `v`, or an op this server doesn't know, is
    /// [`Op::Unsupported`]; a known op with fields wrong for it, or anything that isn't such an
    /// object, is why not.
    pub fn parse(bytes: &[u8]) -> Result<Op, String> {
        if bytes.len() > REQUEST_CAP {
            return Err(format!("a request of more than {REQUEST_CAP} bytes"));
        }
        let value: Value = serde_json::from_slice(bytes)
            .map_err(|error| format!("a request that isn't JSON: {error}"))?;
        let object = value.as_object().ok_or("a request that isn't an object")?;
        let v = object
            .get("v")
            .and_then(Value::as_u64)
            .ok_or("a request with no version")?;
        let op = object
            .get("op")
            .and_then(Value::as_str)
            .ok_or("a request with no op")?;
        if v != u64::from(PROTOCOL) {
            return Ok(Op::Unsupported(format!("protocol {v}")));
        }
        let only = |fields: &[&str]| {
            let extra = object.keys().find(|key| !fields.contains(&key.as_str()));
            match extra {
                Some(key) => Err(format!("the {op} request has a field {key:?}")),
                None => Ok(()),
            }
        };
        let number = |field: &str| {
            object
                .get(field)
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("the {op} request has no number {field}"))
        };
        match op {
            "set_up" => {
                only(&["v", "op"])?;
                Ok(Op::SetUp)
            }
            "add" => {
                only(&["v", "op", "n"])?;
                let n = number("n")?;
                match u8::try_from(n) {
                    Ok(n) if (1..=POOL_CAP).contains(&n) => Ok(Op::Add(n)),
                    _ => Err(format!("an extend by {n} (1 to {POOL_CAP})")),
                }
            }
            "ensure_slots" => {
                only(&["v", "op", "want"])?;
                let want = number("want")?;
                Ok(Op::EnsureSlots(u32::try_from(want).unwrap_or(u32::MAX)))
            }
            other => Ok(Op::Unsupported(format!("the op {other:?}"))),
        }
    }

    /// Its tag for the log: the op's name, or `unsupported` for anything this server doesn't
    /// know, so no text the client sent reaches the Event Log.
    pub fn tag(&self) -> &'static str {
        match self {
            Op::SetUp => "set_up",
            Op::Add(_) => "add",
            Op::EnsureSlots(_) => "ensure_slots",
            Op::Unsupported(_) => "unsupported",
        }
    }

    /// The request as the client sends it: [`Op::Unsupported`] has none.
    pub fn to_bytes(&self) -> Option<Vec<u8>> {
        let value = match self {
            Op::SetUp => serde_json::json!({"v": PROTOCOL, "op": "set_up"}),
            Op::Add(n) => serde_json::json!({"v": PROTOCOL, "op": "add", "n": n}),
            Op::EnsureSlots(want) => {
                serde_json::json!({"v": PROTOCOL, "op": "ensure_slots", "want": want})
            }
            Op::Unsupported(_) => return None,
        };
        serde_json::to_vec(&value).ok()
    }
}

/// What the server answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// `ran`: the set-up ran, or was current and nothing ran (`exit` 0): its outcome.
    Ran(SetupResult),
    /// `refused`: refused before anything ran: why, in words naming no SID or path.
    Refused(String),
    /// `unsupported`: an op or version this server doesn't serve: which.
    Unsupported(String),
}

/// [`Reply`] on the wire, tagged `reply`.
#[derive(Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "snake_case")]
enum Tagged {
    Ran { result: SetupResult },
    Refused { why: String },
    Unsupported { what: String },
}

/// The whole reply: the version and the tagged body.
#[derive(Serialize, Deserialize)]
struct Wire {
    v: u32,
    #[serde(flatten)]
    reply: Tagged,
}

impl Reply {
    /// The reply as one line: JSON and a newline.
    pub fn to_line(&self) -> Vec<u8> {
        let reply = match self.clone() {
            Reply::Ran(result) => Tagged::Ran { result },
            Reply::Refused(why) => Tagged::Refused { why },
            Reply::Unsupported(what) => Tagged::Unsupported { what },
        };
        let mut line = serde_json::to_vec(&Wire { v: PROTOCOL, reply }).unwrap_or_default();
        line.push(b'\n');
        line
    }

    /// The reply in `line` (its newline optional), at [`PROTOCOL`]; fields it doesn't know are
    /// ignored.
    pub fn parse(line: &[u8]) -> Result<Reply, String> {
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        let wire: Wire = serde_json::from_slice(line)
            .map_err(|error| format!("a reply that isn't one: {error}"))?;
        if wire.v != PROTOCOL {
            return Err(format!("a reply at protocol {}", wire.v));
        }
        Ok(match wire.reply {
            Tagged::Ran { result } => Reply::Ran(result),
            Tagged::Refused { why } => Reply::Refused(why),
            Tagged::Unsupported { what } => Reply::Unsupported(what),
        })
    }

    /// What the app makes of the reply, as `SandboxSetup::run` returns it: a set-up's exit and
    /// result as `result_file::outcome` reads the exe's (`Blocked` included); a refusal, or an op
    /// the server doesn't serve, a failure saying why.
    pub fn outcome(&self) -> Result<(), SetupError> {
        match self {
            Reply::Ran(result) => {
                result_file::outcome(u32::from(result.exit), Some(&Ok(Some(result.clone()))))
            }
            Reply::Refused(why) => Err(SetupError::Failed(format!(
                "the set-up task refused the request: {why}"
            ))),
            Reply::Unsupported(what) => Err(SetupError::Failed(format!(
                "the set-up task doesn't serve {what}"
            ))),
        }
    }
}

/// Why the app's request got no reply (`windows::setup_task`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SetUpError {
    /// The install has no set-up task: the set-up with a prompt is the one left.
    #[error("this install has no set-up task")]
    NotRegistered,
    /// An extend by a number out of range.
    #[error("the request isn't one the set-up task takes: {0}")]
    Request(String),
    /// The task couldn't be started.
    #[error("the set-up task couldn't be started: {0}")]
    Task(String),
    /// The pipe wasn't there in time (`CONNECT_WAIT`).
    #[error("the set-up task didn't open its pipe in time")]
    NoServer,
    /// Who serves the pipe isn't the set-up task; nothing was sent.
    #[error("the set-up pipe isn't served by Clauseo's set-up task: {0}")]
    NotTheServer(String),
    /// The pipe failed: what failed.
    #[error("the set-up pipe failed: {0}")]
    Pipe(String),
    /// Another process held the pipe's name, still after the pool's processes were stopped
    /// ([`retry_squatted`]); nothing was sent.
    #[error(
        "another process holds the set-up pipe's name (a squatted pipe), so nothing was sent; \
         restarting Windows frees it: {0}"
    )]
    Squatted(String),
}

/// What `request_set_up` makes of a first answer from a pipe someone else serves: a pool user's
/// process can make the pipe's name before the task does, and keeps it while it lives. The
/// pool's processes are stopped (`stop_pool`, the stop task: whether it started), `settle`
/// waits for that, and `ask` runs once more; a pipe still someone else's, or a stop that
/// didn't start, is [`SetUpError::Squatted`]. Any other answer is the first one's.
pub fn retry_squatted(
    mut ask: impl FnMut() -> Result<Reply, SetUpError>,
    stop_pool: impl FnOnce() -> bool,
    settle: impl FnOnce(),
) -> Result<Reply, SetUpError> {
    let why = match ask() {
        Err(SetUpError::NotTheServer(why)) => why,
        other => return other,
    };
    tracing::warn!(%why, "the set-up pipe is someone else's; stopping the sandbox users' processes");
    if !stop_pool() {
        return Err(SetUpError::Squatted(format!(
            "{why}; the sandbox users' processes couldn't be stopped"
        )));
    }
    settle();
    match ask() {
        Err(SetUpError::NotTheServer(why)) => Err(SetUpError::Squatted(format!(
            "{why}, after the sandbox users' processes were stopped"
        ))),
        other => other,
    }
}

/// What `UserSetup::run` (and its extend) return for `asked`, the set-up task's answer: its reply
/// read as the exe's result is ([`Reply::outcome`]); an install with no set-up task, or a
/// refusal (one the gate didn't foresee for a lawyer it should serve), runs `elevated`, the
/// set-up with its prompt, whose declined prompt is still `SetupError::Refused`. The prompt
/// needs an administrator's credentials, so falling back to it grants no one anything. Any
/// other failure to ask is a failure saying why, with no prompt raised.
pub fn via_task(
    asked: Result<Reply, SetUpError>,
    elevated: impl FnOnce() -> Result<(), SetupError>,
) -> Result<(), SetupError> {
    match asked {
        Ok(Reply::Refused(why)) => {
            tracing::warn!(%why, "the set-up task refused this user; asking with the prompt");
            elevated()
        }
        Ok(reply) => reply.outcome(),
        Err(SetUpError::NotRegistered) => elevated(),
        Err(error) => Err(SetupError::Failed(error.to_string())),
    }
}
