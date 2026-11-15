//! Steps Clauseo's pre-write guard stopped, told to the lawyer. Codex runs the guard as its
//! `PreToolUse` hook before each `apply_patch` and shell call, directly or inside Code Mode's
//! `exec`; a call it blocks never starts, so codex-acp sends no `tool_call` for it (it ignores
//! Codex's `hook/completed`), and the lawyer saw nothing. The guard appends each such refusal to
//! the session's [`file`] (`pre_write_guard/record.rs`); before each live update and when the
//! turn ends (`updates::flush_rate`) the host reads what is new and commits one `Notice` per
//! paper and reason, at most [`MAX_TOLD`] a turn: `HeldOpenBlocked` with the paper, or
//! `CodexFolderBlocked`. Its words are the app's (`notice-words.ts`); the notice's `text` is the
//! English fallback.
//!
//! Claude gets none: claude-agent-acp sends the blocked call as a `tool_call` and its result as
//! a `failed` update carrying the hook's reason, which the room already shows as a refused step
//! ("Tried to …"), so a notice would tell it twice. Claude Code's entry passes no record file,
//! and a line in the file of a Claude session is never read.
//!
//! The file is in the run folder, which the agent can write, so a forged line can add a notice
//! but never hide one; it is read as untrusted: complete lines only, [`MAX_READ`] bytes a time,
//! never through a link or from a pipe, each line parsed and dropped when it does not parse. A
//! file cut short or replaced is read again from its start.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use clauseo_protocol::{AgentKind, Notice, NoticeKind, NoticeLevel, ThreadId};

use super::super::super::Actor;
use super::super::{current, state};
use super::{agent, commit};
use crate::log_file::Cause;
use crate::pre_write_guard::record::{Reason, Record};

/// The record file's name in the session's run folder.
const FILE: &str = "guard-blocks.jsonl";

/// The most bytes read from the file at a time.
const MAX_READ: u64 = 64 * 1024;

/// The most notices of this kind a turn gets.
const MAX_TOLD: usize = 8;

/// The record file of the session whose run folder is `run_root`, which Codex's hook command
/// names (`agent/loads/codex.rs`).
pub(in crate::actor::turn) fn file(run_root: &Path) -> PathBuf {
    run_root.join(FILE)
}

/// How far the host has read its agent's record file, in bytes.
#[derive(Debug, Default)]
pub(in crate::actor::turn) struct Offset(u64);

/// Reads what is new in `thread`'s record file and tells the live turn each refusal it has not
/// told yet. Nothing for a Claude agent, or with no live turn (the lines are passed over).
pub(in crate::actor::turn) fn tell(actor: &mut Actor, thread: ThreadId) {
    if claude(actor, thread) {
        return;
    }
    let Some(path) = state(actor, thread)
        .and_then(|state| state.run.as_ref())
        .map(|run| file(&run.root))
    else {
        return;
    };
    let Some(agent) = agent(actor, thread) else {
        return;
    };
    let records = read_new(&path, &mut agent.guard_blocks);
    if records.is_empty() {
        return;
    }
    let Some(current) = current(actor, thread) else {
        return;
    };
    let mut notices = Vec::new();
    for notice in records.iter().filter_map(notice) {
        let Some(kind) = notice.kind.clone() else {
            continue;
        };
        if current.told.guard.len() >= MAX_TOLD || current.told.guard.contains(&kind) {
            continue;
        }
        current.told.guard.push(kind);
        notices.push(notice.into());
    }
    if notices.is_empty() {
        return;
    }
    if let Err(error) = commit(actor, thread, notices, Vec::new()) {
        tracing::warn!(%thread, error = %Cause(&error), "a guard's refusal notice was not logged");
    }
}

/// Whether `thread`'s agent is Claude (in the unit tests, the kind the fake agent plays).
fn claude(actor: &Actor, thread: ThreadId) -> bool {
    let kind = actor
        .check_thread(thread)
        .ok()
        .and_then(|entry| AgentKind::of(&entry.row.agent));
    #[cfg(test)]
    let kind = kind.or(actor.turn_path.kind_for_tests);
    kind == Some(AgentKind::Claude)
}

/// The notice for `record`, its English text the fallback for a view that does not know its
/// kind; `None` for a held file with no path.
fn notice(record: &Record) -> Option<Notice> {
    let (kind, text) = match (record.reason, &record.path) {
        (Reason::HeldOpen, Some(file)) => (
            NoticeKind::HeldOpenBlocked { file: file.clone() },
            format!(
                "Clauseo stopped a step: the AI tried to change {file}, which is open in Word. \
                 Close it in Word, then ask again."
            ),
        ),
        (Reason::HeldOpen, None) => return None,
        (Reason::CodexFolder, _) => (
            NoticeKind::CodexFolderBlocked,
            "Clauseo stopped a step: the AI tried to change its own settings in this matter's \
             folder."
                .to_owned(),
        ),
    };
    Some(Notice {
        level: NoticeLevel::Warning,
        text,
        kind: Some(kind),
    })
}

/// The records `path` holds past `offset`, complete lines only, at most [`MAX_READ`] bytes,
/// moving `offset` past what was read. A line longer than that is passed over; a file shorter
/// than `offset` is read from its start; anything but a regular file reads as nothing.
pub(in crate::actor::turn) fn read_new(path: &Path, offset: &mut Offset) -> Vec<Record> {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Vec::new();
    };
    if !meta.is_file() || meta.len() == offset.0 {
        return Vec::new();
    }
    if meta.len() < offset.0 {
        offset.0 = 0;
    }
    let Ok(mut opened) = crate::write::open_read_regular(path) else {
        return Vec::new();
    };
    let mut bytes = Vec::new();
    let read = opened
        .seek(SeekFrom::Start(offset.0))
        .and_then(|_| opened.take(MAX_READ).read_to_end(&mut bytes));
    if read.is_err() {
        return Vec::new();
    }
    let whole = match bytes.iter().rposition(|byte| *byte == b'\n') {
        Some(end) => end.saturating_add(1),
        // A line past the cap is passed over; a shorter one may still be being written.
        None if u64::try_from(bytes.len()).is_ok_and(|len| len >= MAX_READ) => {
            offset.0 = offset.0.saturating_add(MAX_READ);
            return Vec::new();
        }
        None => return Vec::new(),
    };
    offset.0 = offset
        .0
        .saturating_add(u64::try_from(whole).unwrap_or(MAX_READ));
    bytes
        .get(..whole)
        .unwrap_or_default()
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice(line).ok())
        .collect()
}

#[cfg(test)]
mod tests;
