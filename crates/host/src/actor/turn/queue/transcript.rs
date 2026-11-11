//! Claude's transcript as the proof a steer was taken in. claude-agent-acp answers
//! `_session/steering` `injected` once the message is in its own queue, says nothing over ACP when
//! the model reads it, and drops it unread on a cancel; Claude Code, though, writes each message
//! it takes to `<claude home>/projects/<project key>/<session id>.jsonl`. A steer counts as taken
//! in when a line written no earlier than [`SKEW`] before its send holds one of its text blocks,
//! both trimmed, exactly: a `user` line, a `queued_command` attachment, or a queue operation
//! `remove` the turn absorbed (`absorbed_mid_turn`). An `enqueue` alone is still waiting, and a
//! `dequeue` before an interrupt was dropped, so neither counts.
//!
//! Read only, off the actor, and bounded: the last [`TAIL`] bytes of each file, whose first,
//! partial line is skipped. A file missing, unreadable or not JSON lines shows nothing: a steer
//! is never taken as in on doubt. Nothing read here is logged.

use std::collections::BTreeMap;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_protocol::{AgentSessionId, Timestamp};
use serde_json::Value;

/// The most bytes read from the end of one transcript.
const TAIL: u64 = 256 * 1024;
/// How much earlier than the steer's send a line may be stamped, for the two clocks.
const SKEW: Duration = Duration::from_secs(2);

/// One steer to look for: its text blocks, trimmed, and when it was sent.
#[derive(Debug, Clone)]
pub(super) struct Wanted {
    texts: Vec<String>,
    since: i64,
}

impl Wanted {
    /// A steer of `texts` sent at `sent`. Blank texts are left out, so a steer with no text
    /// is never found.
    pub(super) fn new<'a>(texts: impl Iterator<Item = &'a str>, sent: Timestamp) -> Self {
        let skew = i64::try_from(SKEW.as_millis()).unwrap_or(i64::MAX);
        Self {
            texts: texts
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
                .collect(),
            since: sent.as_millis().saturating_sub(skew),
        }
    }
}

/// Where a Claude session's transcript may be: its home, the folder it was started in (as
/// stored) and its session id.
#[derive(Debug, Clone)]
pub(super) struct Session {
    pub(super) home: PathBuf,
    pub(super) folder: PathBuf,
    pub(super) id: AgentSessionId,
}

impl Session {
    /// The transcript files this session may write: one per project key of its working folder
    /// (`agent::session_folder`) and of the folder as stored, as the data fence has them. None
    /// for a session id that is not a plain file name.
    fn transcripts(&self) -> Vec<PathBuf> {
        let id = self.id.as_str();
        let plain = id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
        if !plain {
            return Vec::new();
        }
        let cwd = crate::agent::session_folder(&self.folder);
        crate::agent::data_fence::claude_projects(&self.home, &[&cwd, &self.folder])
            .into_iter()
            .map(|project| project.join(format!("{id}.jsonl")))
            .collect()
    }
}

/// For each of `asks`, whether its session's transcript shows it taken in. Each file is read
/// once, however many steers look in it.
pub(super) fn taken(asks: &[(Session, Wanted)]) -> Vec<bool> {
    let mut lines: BTreeMap<PathBuf, Vec<Value>> = BTreeMap::new();
    asks.iter()
        .map(|(session, wanted)| {
            session.transcripts().into_iter().any(|path| {
                lines
                    .entry(path)
                    .or_insert_with_key(|path| read_lines(path))
                    .iter()
                    .any(|line| shows(line, wanted))
            })
        })
        .collect()
}

/// The JSON lines of the last [`TAIL`] bytes of `path`; none when it cannot be read.
pub(super) fn read_lines(path: &Path) -> Vec<Value> {
    let Ok(bytes) = read_tail(path) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// The last [`TAIL`] bytes of `path`, from the first line that starts in them.
fn read_tail(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    let start = file.metadata()?.len().saturating_sub(TAIL);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(TAIL).read_to_end(&mut bytes)?;
    if start > 0 {
        let first = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |at| at + 1);
        bytes.drain(..first);
    }
    Ok(bytes)
}

/// Whether `line` shows `wanted` taken in: a line of a kind that means taken, stamped no earlier
/// than the steer allows, holding one of its texts.
pub(super) fn shows(line: &Value, wanted: &Wanted) -> bool {
    let stamped = line
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(clauseo_limits::parse_rfc3339)
        .is_some_and(|at| at.as_millis() >= wanted.since);
    stamped && means_taken(line) && holds(line, &wanted.texts)
}

/// A `user` line, a `queued_command` attachment, or a queue operation `remove` the turn
/// absorbed.
fn means_taken(line: &Value) -> bool {
    let field =
        |value: &Value, key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    match field(line, "type").as_deref() {
        Some("user") => true,
        Some("attachment") => line.get("attachment").is_some_and(|attachment| {
            field(attachment, "type").as_deref() == Some("queued_command")
        }),
        Some("queue-operation") => {
            let absorbed = line.as_object().is_some_and(|fields| {
                fields
                    .values()
                    .any(|value| value.as_str() == Some("absorbed_mid_turn"))
            });
            field(line, "operation").as_deref() == Some("remove") && absorbed
        }
        _ => false,
    }
}

/// Whether a string anywhere in `value`, trimmed, is one of `texts`.
fn holds(value: &Value, texts: &[String]) -> bool {
    match value {
        Value::String(text) => texts.iter().any(|wanted| wanted == text.trim()),
        Value::Array(values) => values.iter().any(|value| holds(value, texts)),
        Value::Object(fields) => fields.values().any(|value| holds(value, texts)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

#[cfg(test)]
mod tests;
