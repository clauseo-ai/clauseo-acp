//! Steps Clauseo's pre-write guard stopped, told to the lawyer. Codex runs the guard as its
//! `PreToolUse` hook before each `apply_patch` and shell call, directly or inside Code Mode's
//! `exec`; a call it blocks never starts, so codex-acp sends no `tool_call` for it (it ignores
//! Codex's `hook/completed`), and the lawyer saw nothing. The guard appends each such refusal to
//! the session's record file (`RunFolders::guard`, `pre_write_guard/record.rs`); at each live
//! update and when the turn ends (`updates::flush_rate`) the host reads what is new off the actor,
//! one read at a time, and commits one `Notice` per paper and reason when the read answers
//! ([`on_read`]).
//!
//! Claude gets none: claude-agent-acp sends the blocked call as a `tool_call` and its result as
//! a `failed` update carrying the hook's reason, which the room already shows as a refused step
//! ("Tried to …"), so a notice would tell it twice. Claude Code's entry passes no record file,
//! and a line in the file of a Claude session is never read.
//!
//! The file is in a folder of the session's own no agent's shell or patch tool may write
//! (`launch_checks/folders.rs` `make_guard`), and is read only when the file opened has the
//! identity the host read from the file it made, but its lines are still read as untrusted:
//! complete lines only, [`MAX_READ`] bytes a time, never through a link, from a pipe or a file
//! with two links, a line that does not parse skipped (the next one still read), a file cut short
//! read from its start.
//! A paper is named only when the matter holds it, by its name alone (the last part of its
//! path, [`shown_name`], through #1122's `shown.rs`); any other refusal of a held paper, and a
//! name with an invisible character, is `GuardStepBlocked`, unnamed. A
//! turn names at most [`MAX_TOLD`] papers, reasons and unnamed steps, each once; every other step
//! it stopped (a repeat of one told, named or not, or one past the cap) is counted and told in one
//! `GuardStepsMore` line by the read the turn's end asks for.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use clauseo_protocol::{AgentKind, MatterPath, Notice, NoticeKind, NoticeLevel, ThreadId, TurnId};

use super::super::super::Actor;
use super::super::super::input::Input;
use super::super::{TurnInput, current, state};
use super::{agent, commit};
use crate::agent::codex::prepare::{Job, off_actor};
use crate::log_file::Cause;
use crate::pre_write_guard::record::{Reason, Record};
use crate::write::RecordId;

/// The most bytes read from the file at a time.
const MAX_READ: u64 = 64 * 1024;

/// The most notices of these kinds a turn names; [`NoticeKind::GuardStepsMore`] comes on top.
const MAX_TOLD: usize = 8;

/// The most characters of a paper's name a line shows; a longer one keeps its start and end.
const MAX_NAME: usize = 60;

/// The read's off-actor job: a local file the host made, and a stat in the matter folder, which a
/// network share could hold up. Its wait is 10 s, not the 5 s of `Job::short`, which the turn's
/// cancel ladder also arms (a test moving the clock past a 5 s timer means the cancel's).
const JOB: Job = Job {
    what: "guard record read",
    matter: None,
    wait: std::time::Duration::from_secs(10),
};

/// What the host has read of its agent's record file and told of it.
#[derive(Debug, Default)]
pub(in crate::actor::turn) struct Reads {
    /// How far the file is read, in bytes.
    offset: u64,
    /// A read is off the actor.
    reading: bool,
    /// Another read is due once it answers; `true` when a turn's end asked for one.
    again: Option<bool>,
    /// The turn [`Self::told`] and [`Self::more`] are for.
    turn: Option<TurnId>,
    /// The kinds that turn told.
    told: Vec<NoticeKind>,
    /// The steps that turn refused that got no notice of their own, not told yet.
    more: u32,
}

#[cfg(test)]
impl Reads {
    /// Whether a read is out, or due again.
    pub(in crate::actor::turn) const fn busy(&self) -> bool {
        self.reading || self.again.is_some()
    }
}

/// A read's answer, for [`on_read`].
#[derive(Debug)]
pub(crate) struct Answer {
    thread: ThreadId,
    /// The agent whose file was read.
    agent: u64,
    /// The live turn when the read was asked for.
    turn: Option<TurnId>,
    /// The turn's end asked for it.
    end: bool,
    /// Where the next read starts.
    offset: u64,
    /// One kind per refusal read, in order.
    kinds: Vec<NoticeKind>,
}

/// Reads what is new in `thread`'s record file, off the actor, for the live turn, or for the one
/// ending when `end`. Nothing for a Claude agent; a read already out reads again once it answers.
pub(in crate::actor::turn) fn tell(actor: &mut Actor, thread: ThreadId, end: bool) {
    if claude(actor, thread) {
        return;
    }
    let Some((path, record)) = state(actor, thread)
        .and_then(|state| state.run.as_ref())
        .and_then(|run| run.guard.clone())
    else {
        return;
    };
    let turn = current(actor, thread).map(|current| current.turn);
    let clock = Arc::clone(&actor.clock);
    let Some(agent) = agent(actor, thread) else {
        return;
    };
    let reads = &mut agent.guard_blocks;
    if reads.reading {
        reads.again = Some(reads.again.unwrap_or(false) || end);
        return;
    }
    reads.reading = true;
    let (offset, matter, id) = (reads.offset, agent.folder.clone(), agent.id);
    actor.spawn_post(async move {
        let read = off_actor(clock, JOB, move || {
            Ok(read_new(&path, record, offset, &matter))
        })
        .await;
        let (offset, kinds) = read.unwrap_or((offset, Vec::new()));
        Input::Turn(TurnInput::GuardRead(Box::new(Answer {
            thread,
            agent: id,
            turn,
            end,
            offset,
            kinds,
        })))
    });
}

/// A read answered: each kind its turn has not told is told, up to [`MAX_TOLD`]; every other
/// step (a repeat of a kind told, or one past the cap) is counted, and the count told when the
/// turn's end asked for the read. A read for an agent
/// since replaced is dropped.
pub(in crate::actor::turn) fn on_read(actor: &mut Actor, answer: Answer) {
    let thread = answer.thread;
    let Some(agent) = agent(actor, thread).filter(|agent| agent.id == answer.agent) else {
        return;
    };
    let reads = &mut agent.guard_blocks;
    reads.reading = false;
    reads.offset = answer.offset;
    let turn = answer.turn.or(reads.turn);
    if turn != reads.turn {
        (reads.turn, reads.told, reads.more) = (turn, Vec::new(), 0);
    }
    let mut notices = Vec::new();
    for kind in answer.kinds {
        if reads.told.contains(&kind) || reads.told.len() >= MAX_TOLD {
            reads.more = reads.more.saturating_add(1);
            continue;
        }
        reads.told.push(kind.clone());
        notices.push(notice(kind).into());
    }
    if answer.end && reads.more > 0 {
        notices.push(notice(NoticeKind::GuardStepsMore { more: reads.more }).into());
        reads.more = 0;
    }
    let again = reads.again.take();
    if !notices.is_empty()
        && let Err(error) = commit(actor, thread, notices, Vec::new())
    {
        tracing::warn!(%thread, error = %Cause(&error), "a guard's refusal notice was not logged");
    }
    if let Some(end) = again {
        tell(actor, thread, end);
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

/// The notice of `kind`, its English text the fallback for a view that does not know it: a
/// paper's shown name at most, never its path.
fn notice(kind: NoticeKind) -> Notice {
    let text = if let NoticeKind::HeldOpenBlocked { file } = &kind {
        format!(
            "Clauseo stopped a step: the AI tried to change {file}, which is open in Word. Close \
             it in Word, then ask again."
        )
    } else if kind == NoticeKind::CodexFolderBlocked {
        "Clauseo stopped a step: the AI tried to change its own settings in this matter's folder."
            .to_owned()
    } else if let NoticeKind::GuardStepsMore { more } = kind {
        if more == 1 {
            "Clauseo stopped one more step.".to_owned()
        } else {
            format!("Clauseo stopped {more} more steps.")
        }
    } else {
        "Clauseo stopped a step.".to_owned()
    };
    Notice {
        level: NoticeLevel::Warning,
        text,
        kind: Some(kind),
    }
}

/// The kind `record` is told with: the paper by its shown name when the matter folder `matter`
/// holds it (no folder part, nothing past the matter: a [`MatterPath`] is relative with no
/// `..`), else unnamed.
fn kind_of(record: &Record, matter: &Path) -> NoticeKind {
    let path = match (record.reason, &record.path) {
        (Reason::CodexFolder, _) => return NoticeKind::CodexFolderBlocked,
        (Reason::HeldOpen, Some(path)) => path,
        (Reason::HeldOpen, None) => return NoticeKind::GuardStepBlocked,
    };
    let held = std::fs::symlink_metadata(matter.join(path.as_str())).is_ok();
    let name = path.as_str().rsplit('/').next().unwrap_or_default();
    match MatterPath::new(&shown_name(name)) {
        Ok(file) if held => NoticeKind::HeldOpenBlocked { file },
        _ => NoticeKind::GuardStepBlocked,
    }
}

/// `name` as a line shows it: cut to at most [`MAX_NAME`] characters, the middle elided, then
/// shown by the host's one rule for text from outside it, #1122's
/// [`shown::text`](crate::shown::text) (every invisible, blank, bidi-control and line-breaking
/// character, and `\`, written out as its escape; right-to-left runs isolated). The cut comes
/// first so it never splits an isolate. A written-out character always brings a `\`, which no
/// [`MatterPath`] may hold, so [`kind_of`] tells such a name unnamed, and a name that is shown
/// is never longer than the cut and its isolates.
pub(super) fn shown_name(name: &str) -> String {
    crate::shown::text(&cut(name))
}

/// `text`, or when longer than [`MAX_NAME`] characters, its start and end around `…`,
/// [`MAX_NAME`] in all.
fn cut(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= MAX_NAME {
        return text.to_owned();
    }
    let tail = (MAX_NAME - 1) / 2;
    let head = MAX_NAME - 1 - tail;
    let end = chars.get(chars.len() - tail..).unwrap_or_default();
    let start = chars.get(..head).unwrap_or_default();
    start.iter().chain(['…'].iter()).chain(end).collect()
}

/// The kinds of the records `path` holds past `offset`, complete lines only, at most
/// [`MAX_READ`] bytes, with where the next read starts; `matter` the matter folder. A line
/// longer than that is passed over; a file shorter than `offset` is read from its start;
/// anything but a regular file with one link and the identity `id` the host read when it made
/// the file reads as nothing. File system calls: off the actor.
pub(in crate::actor::turn) fn read_new(
    path: &Path,
    id: RecordId,
    offset: u64,
    matter: &Path,
) -> (u64, Vec<NoticeKind>) {
    let Ok(mut opened) = crate::write::open_read_record(path, id) else {
        return (offset, Vec::new());
    };
    let len = opened.metadata().map_or(offset, |meta| meta.len());
    let offset = if len < offset { 0 } else { offset };
    if len == offset {
        return (offset, Vec::new());
    }
    let mut bytes = Vec::new();
    let read = opened
        .seek(SeekFrom::Start(offset))
        .and_then(|_| opened.take(MAX_READ).read_to_end(&mut bytes));
    if read.is_err() {
        return (offset, Vec::new());
    }
    let whole = match bytes.iter().rposition(|byte| *byte == b'\n') {
        Some(end) => end.saturating_add(1),
        // A line past the cap is passed over; a shorter one may still be being written.
        None if u64::try_from(bytes.len()).is_ok_and(|len| len >= MAX_READ) => {
            return (offset.saturating_add(MAX_READ), Vec::new());
        }
        None => return (offset, Vec::new()),
    };
    let next = offset.saturating_add(u64::try_from(whole).unwrap_or(MAX_READ));
    let kinds = bytes
        .get(..whole)
        .unwrap_or_default()
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice::<Record>(line).ok())
        .map(|record| kind_of(&record, matter))
        .collect();
    (next, kinds)
}

#[cfg(test)]
mod tests;
