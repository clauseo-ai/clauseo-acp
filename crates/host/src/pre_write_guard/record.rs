//! The guard's record of the refusals the lawyer is told about. Codex sends no event for a call
//! its hook blocked (codex-acp 2.6.0 ignores `hook/completed`), so with [`FLAG`] and a file
//! (Codex's hook passes `<data>/run/<session>/guard-blocks.jsonl`, `agent/loads/codex.rs`), a
//! refusal for a held-open file or the `.codex` folder is appended to that file as one JSON line,
//! a [`Record`], and the host reads it back during the turn to show "Clauseo stopped a step"
//! (`actor/turn/updates/guard_blocks.rs`). Claude Code's entry passes no file: Claude shows the
//! blocked call as a failed step of its own.
//!
//! Best effort: a file that cannot be opened or written (no folder, a link, a pipe, no room) is
//! skipped without a word, and the exit code and the reason never change. The line holds no agent
//! text but the tool's name: the path is the guard's own listing of the held file.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use clauseo_protocol::{MatterPath, Timestamp};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Refusal;
use crate::clock::{Clock, SystemClock};

/// The argument that names the record file, followed by the file; after any other argument.
pub(crate) const FLAG: &str = "--record";

/// The longest tool name a record keeps; a longer one is cut.
const MAX_TOOL: usize = 64;

/// Why a refusal was recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Reason {
    /// The file is open in Word or another program.
    HeldOpen,
    /// The call writes in, or under Codex's hook names, the matter's `.codex` folder.
    CodexFolder,
}

/// Whose hook refused the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Agent {
    /// Claude Code's entry, the one that passes `--matter`.
    Claude,
    /// Codex's hook, with no `--matter`.
    Codex,
}

/// One recorded refusal, one JSON line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Record {
    /// When, by the wall clock.
    pub(crate) at: Timestamp,
    /// Whose hook it was.
    pub(crate) agent: Agent,
    /// The hook input's tool name (`apply_patch`, `Bash`, ...), at most [`MAX_TOOL`] bytes.
    pub(crate) tool: String,
    /// Why it was refused.
    pub(crate) reason: Reason,
    /// The held file as the matter lists it; none for the `.codex` folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) path: Option<MatterPath>,
}

/// The absolute file after [`FLAG`] in `args`, if any.
pub(super) fn file_arg(args: &[OsString]) -> Option<PathBuf> {
    let at = args.iter().position(|arg| arg == FLAG)?;
    args.get(at.saturating_add(1))
        .map(PathBuf::from)
        .filter(|file| file.is_absolute())
}

/// The record of `refusal` for the hook input `bytes`, from Claude Code's entry when `claude`;
/// `None` for a refusal the lawyer is not told about (`stetdoc mcp`).
pub(super) fn record_of(refusal: &Refusal, bytes: &[u8], claude: bool) -> Option<Record> {
    let (reason, path) = match refusal {
        Refusal::HeldOpen(path) => (Reason::HeldOpen, Some(path.clone())),
        Refusal::CodexFolder => (Reason::CodexFolder, None),
        Refusal::StetdocServe => return None,
    };
    let tool = serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|hook| hook.get("tool_name")?.as_str().map(cut))
        .unwrap_or_default();
    Some(Record {
        at: SystemClock.now().wall,
        agent: if claude { Agent::Claude } else { Agent::Codex },
        tool,
        reason,
        path,
    })
}

/// `tool`, cut to at most [`MAX_TOOL`] bytes at a character boundary.
fn cut(tool: &str) -> String {
    let mut end = tool.len().min(MAX_TOOL);
    while !tool.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    tool.get(..end).unwrap_or_default().to_owned()
}

/// Appends `record` to `file` as one line, in one write; any failure is dropped.
pub(super) fn append(file: &Path, record: &Record) {
    let Ok(mut line) = serde_json::to_vec(record) else {
        return;
    };
    line.push(b'\n');
    if let Ok(mut out) = crate::write::open_append_regular(file) {
        drop(out.write_all(&line));
    }
}
