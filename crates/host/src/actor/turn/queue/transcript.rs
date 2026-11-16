//! Claude's transcript as the proof a steer was taken in. claude-agent-acp answers
//! `_session/steering` `injected` once the message is in its own queue, says nothing over ACP when
//! the model reads it, and drops it unread on a cancel; Claude Code, though, writes each message
//! it takes to `<claude home>/projects/<project key>/<session id>.jsonl`. A steer counts as taken
//! in when a line written no earlier than [`SKEW`] before its send holds one of its text blocks,
//! both trimmed, exactly, where the lawyer's own words go: a `user` line's message content (a
//! string, or its `text` blocks; never a `tool_result`, whose content is a tool's output), a
//! `queued_command` attachment's prompt, or the content of a queue operation `remove` the turn
//! absorbed (`absorbed_mid_turn`). An `enqueue` alone is still waiting, and a `dequeue` before an
//! interrupt was dropped, so neither counts.
//!
//! Read only, off the actor, and bounded: each file is marked with its length when the steer
//! goes out ([`mark`]), and each check reads on from where the last one stopped, at most
//! [`CAP`] bytes, up to the last whole line; a line cut by the cap or still being written is read
//! again by the next check, and one longer than the cap is skipped. A file missing, unreadable or
//! not JSON lines shows nothing: a steer is never taken as in on doubt. Nothing read here is
//! logged.

use std::collections::BTreeMap;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_protocol::{AgentSessionId, Timestamp};
use serde_json::Value;

/// The most bytes one check reads from one transcript: the longest prompt text plus 1 MiB (7 MiB),
/// so a steer's own line fits whole. `MAX_PROMPT_TEXT_BYTES` already counts a text as JSON
/// escapes it (`escaped_len`, serde's escaping), which is what JSON.stringify writes to the
/// transcript: `"` `\` and \n \r \t \b \f two bytes each, other control characters six
/// (`\u00XX`), anything else as its UTF-8. A steer's texts, escaped, are at most that limit, so
/// no multiplier is needed; the 1 MiB is for the line's other fields (ids, folder, version,
/// stamps), a few KiB. Memory follows what was appended since the last read, never the cap
/// alone: each check reads on from where the last stopped, so a check holds this many bytes, and
/// the lines parsed from them, only when Claude wrote that much (large tool results) in between.
const CAP: u64 = clauseo_protocol::MAX_PROMPT_TEXT_BYTES as u64 + 1024 * 1024;
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

/// Where the next read of a transcript starts: a byte offset, at the start of a line unless
/// `inside` a line longer than [`CAP`], which is skipped to its end.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Resume {
    at: u64,
    inside: bool,
}

/// One transcript file and where its next read starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Place {
    path: PathBuf,
    resume: Resume,
}

/// `session`'s transcripts, each marked at its length now, just before a steer goes out, so
/// what Claude writes for the steer comes after the mark. A file not there yet is marked at its
/// start. One `stat` per file, in the host's own data folder; nothing is read.
pub(super) fn mark(session: &Session) -> Vec<Place> {
    session
        .transcripts()
        .into_iter()
        .map(|path| {
            let at = std::fs::metadata(&path).map_or(0, |meta| meta.len());
            Place {
                path,
                resume: Resume { at, inside: false },
            }
        })
        .collect()
}

/// What one check saw for one steer: whether it was taken in, where each of its transcripts is
/// read on from, and whether a read stopped at the cap with more to read.
#[derive(Debug, Clone)]
pub(super) struct Seen {
    pub(super) found: bool,
    pub(super) places: Vec<Place>,
    pub(super) behind: bool,
}

/// What one read of a file from one place gave.
#[derive(Debug, Default)]
struct Read {
    lines: Vec<Value>,
    next: Resume,
    behind: bool,
}

/// For each of `asks`, its places and what it looks for, what its transcripts show. A file is
/// read once per place, however many steers read it from there.
pub(super) fn taken(asks: Vec<(Vec<Place>, Wanted)>) -> Vec<Seen> {
    let mut reads: BTreeMap<(PathBuf, Resume), Read> = BTreeMap::new();
    asks.into_iter()
        .map(|(places, wanted)| {
            let mut seen = Seen {
                found: false,
                places: Vec::with_capacity(places.len()),
                behind: false,
            };
            for Place { path, resume } in places {
                let read = reads.entry((path.clone(), resume)).or_insert_with(|| {
                    read_on(&path, resume, CAP).unwrap_or_else(|_| Read::unread(resume))
                });
                seen.found |= read.lines.iter().any(|line| shows(line, &wanted));
                seen.behind |= read.behind;
                seen.places.push(Place {
                    path,
                    resume: read.next,
                });
            }
            seen
        })
        .collect()
}

impl Read {
    /// A file that could not be read: nothing, read again from the same place next time.
    fn unread(resume: Resume) -> Self {
        Self {
            lines: Vec::new(),
            next: resume,
            behind: false,
        }
    }
}

/// The JSON lines of `path` from `resume`, at most `cap` bytes, up to the last whole line, and
/// where the next read starts. A line cut at the end is read again next time, from its start; a
/// line with no end within `cap` bytes is skipped to its end. A file shorter than its mark was
/// replaced: it is read from its start.
fn read_on(path: &Path, resume: Resume, cap: u64) -> std::io::Result<Read> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let resume = if len < resume.at {
        Resume::default()
    } else {
        resume
    };
    file.seek(SeekFrom::Start(resume.at))?;
    // Sized to what is there, so a read near the cap does not grow to twice it.
    let there = usize::try_from(len.saturating_sub(resume.at).min(cap)).unwrap_or(0);
    let mut bytes = Vec::with_capacity(there);
    file.take(cap).read_to_end(&mut bytes)?;
    let read = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let full = read >= cap;
    let newline = |bytes: &[u8]| bytes.iter().position(|byte| *byte == b'\n');
    // Inside a line too long to read, the read starts after its end, if that is in these bytes.
    let skipped = if resume.inside {
        newline(&bytes).map(|at| at + 1)
    } else {
        Some(0)
    };
    let Some(start) = skipped else {
        let next = Resume {
            at: resume.at.saturating_add(read),
            inside: true,
        };
        return Ok(Read {
            lines: Vec::new(),
            next,
            behind: full,
        });
    };
    let body = bytes.get(start..).unwrap_or_default();
    let whole = body
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |at| at + 1);
    let next = if whole == 0 && start == 0 && full {
        // One line longer than the cap: never a steer's own, skipped to its end.
        Resume {
            at: resume.at.saturating_add(read),
            inside: true,
        }
    } else {
        let done = u64::try_from(start + whole).unwrap_or(u64::MAX);
        Resume {
            at: resume.at.saturating_add(done),
            inside: false,
        }
    };
    let lines = String::from_utf8_lossy(body.get(..whole).unwrap_or_default())
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    Ok(Read {
        lines,
        next,
        behind: full,
    })
}

/// Whether `line` shows `wanted` taken in: a line of a kind that means taken, stamped no earlier
/// than the steer allows, whose words hold one of its texts.
pub(super) fn shows(line: &Value, wanted: &Wanted) -> bool {
    let stamped = line
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(clauseo_limits::parse_rfc3339)
        .is_some_and(|at| at.as_millis() >= wanted.since);
    stamped
        && words(line).is_some_and(|words| {
            texts(words).any(|text| wanted.texts.iter().any(|want| want == text.trim()))
        })
}

/// Where a line that means taken keeps the lawyer's words: a `user` line's message content, a
/// `queued_command` attachment's prompt, or the content of a queue operation `remove` the turn
/// absorbed. None for any other line.
fn words(line: &Value) -> Option<&Value> {
    fn field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
        value.get(key).and_then(Value::as_str)
    }
    match field(line, "type")? {
        "user" => line.get("message")?.get("content"),
        "attachment" => {
            let attachment = line.get("attachment")?;
            (field(attachment, "type")? == "queued_command")
                .then(|| attachment.get("prompt"))
                .flatten()
        }
        "queue-operation" => {
            let absorbed = line.as_object().is_some_and(|fields| {
                fields
                    .values()
                    .any(|value| value.as_str() == Some("absorbed_mid_turn"))
            });
            (field(line, "operation")? == "remove" && absorbed)
                .then(|| line.get("content"))
                .flatten()
        }
        _ => None,
    }
}

/// The texts of a message's content: the string itself, or each of its `text` blocks' text.
/// Every other block (a `tool_result`, an image) holds none of the lawyer's words.
fn texts(content: &Value) -> impl Iterator<Item = &str> {
    let blocks = content.as_array().map(Vec::as_slice).unwrap_or_default();
    let blocks = blocks.iter().filter_map(|block| {
        let text = block.get("text").and_then(Value::as_str);
        (block.get("type").and_then(Value::as_str) == Some("text"))
            .then_some(text)
            .flatten()
    });
    content.as_str().into_iter().chain(blocks)
}

#[cfg(test)]
mod tests;
