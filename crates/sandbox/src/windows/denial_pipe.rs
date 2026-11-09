//! The denial pipe (S15 tier B): how the outer runner tells the host what its git watch set
//! aside and what the lawyer is to be told, on a channel the agent can't write to.
//!
//! The outer's stderr carries the agent's stderr too (the relay in `runner/outer.rs`), so
//! nothing read from it is trusted: a line there never makes a [`Denial`] or a notice. Instead:
//!
//! - **The pipe** is the host's (PR6's launcher): a runner pipe set's `denials` pipe,
//!   `\\.\pipe\clauseo-run-<32 hex>-denials` ([`denial_pipe_name`], the same name as the
//!   launcher's `PipeSet::name(Role::Denials)`), the desktop user alone in its DACL, the Medium
//!   label, one instance made first and local clients only. Its name goes in the launch message
//!   (`LaunchMessage::denial_pipe`), which only the desktop user can read.
//! - **The client:** the outer checks the name ([`check_pipe_name`]) and opens it, write-only,
//!   not inheritable, with identification-only quality of service, and checks it is a pipe
//!   (`os::open_writer`), before anything runs as the pool user (`runner/start.rs`). It takes
//!   the one instance, so nothing of the agent's can race for it. When the launch names a pipe
//!   that is taken, gone or can't be reached, the launch fails, in plain words. A launch that
//!   names none leaves the records in the outer's log only, never on stderr.
//! - **The records:** one JSON object per line, tagged by `kind` (`{"kind":"set_aside","path":
//!   …,"to":…,"what":"git_repository"}`, `{"kind":"notice","text":…}`; [`encode`]), at most
//!   [`RECORD_CAP`] bytes with its newline. Kinds are appended, never renamed; the runner and the
//!   host ship together, and a change of shape bumps `LAUNCH_VERSION`. [`decode`] parses one
//!   line, strictly: no unknown field or kind, nothing longer. A [`Record::SetAside`] becomes the
//!   session's [`Denial::SetAside`] ([`Record::denial`]); a [`Record::Notice`] is for the lawyer.
//!   The host reads them with a [`RecordReader`], which skips a line it can't parse and reads
//!   on, so one bad record never drops the denials after it.
//!
//! This module is the format's one source: the launcher (PR6) reads with it, not a copy.
//!
//! The names and the records hold no Windows call, so their tests run on every OS.

use std::fmt;
use std::io::{self, BufRead, Read as _};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::pipe::{PIPE_ROOT, PipeSet};
use crate::denial::shown_name;
use crate::{Denial, SetAsideWhat};

#[cfg(windows)]
pub mod os;
#[cfg(any(windows, test))]
pub(crate) mod sink;

/// The denial pipe's name after its set's base: the launcher's `Role::Denials` suffix.
pub const DENIALS_SUFFIX: &str = "-denials";
/// The longest record, its newline included: two Windows paths of 32767 UTF-16 units, each at
/// most three bytes in UTF-8, with room for JSON's escapes.
pub const RECORD_CAP: usize = 256 * 1024;

/// Why a denial pipe's name was refused. It never quotes the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a denial pipe's name")]
pub struct NotADenialPipe;

/// The denial pipe of the runner pipe set `set`: `\\.\pipe\clauseo-run-<32 hex>-denials`.
pub fn denial_pipe_name(set: &PipeSet) -> String {
    format!("{PIPE_ROOT}{}{DENIALS_SUFFIX}", set.base())
}

/// Refuses `name` unless it is exactly what [`denial_pipe_name`] makes: the local pipe root, a
/// pipe set's base (`clauseo-run-` and 32 lowercase hex) and `-denials`, nothing else (no
/// server, no other folder, no `..`).
pub fn check_pipe_name(name: &str) -> Result<(), NotADenialPipe> {
    let base = name
        .strip_prefix(PIPE_ROOT)
        .and_then(|rest| rest.strip_suffix(DENIALS_SUFFIX))
        .ok_or(NotADenialPipe)?;
    PipeSet::parse(base)
        .map(drop)
        .map_err(|_bad| NotADenialPipe)
}

/// What the outer tells the host. Kinds are appended, never renamed: the host reads them.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Record {
    /// An item the git watch set aside while the agent ran.
    SetAside {
        /// Where it was, below the watched folder.
        path: String,
        /// Where it is now, in the set-aside.
        to: String,
        /// What it was.
        what: SetAsideWhat,
    },
    /// A notice for the lawyer, in plain words: an item moved, or why the session stopped.
    Notice {
        /// The words.
        text: String,
    },
}

impl Record {
    /// The record of `what` moved from `path` to `to`, each as the lawyer is shown it
    /// ([`shown_name`]): a part that isn't Unicode replaced, a control or bidi character the
    /// agent put in the name escaped, at most 512 characters.
    pub fn set_aside(path: &Path, to: &Path, what: SetAsideWhat) -> Record {
        Record::SetAside {
            path: shown_name(path),
            to: shown_name(to),
            what,
        }
    }

    /// A notice for the lawyer.
    pub fn notice(text: impl Into<String>) -> Record {
        Record::Notice { text: text.into() }
    }

    /// The denial a [`Record::SetAside`] reports, for the launch's sink.
    pub fn denial(&self) -> Option<Denial> {
        match self {
            Record::SetAside { path, to, what } => Some(Denial::SetAside {
                path: path.into(),
                to: to.into(),
                what: *what,
            }),
            Record::Notice { .. } => None,
        }
    }

    /// The record's kind, for a log line: never a path.
    pub fn kind(&self) -> &'static str {
        match self {
            Record::SetAside { .. } => "set_aside",
            Record::Notice { .. } => "notice",
        }
    }
}

impl fmt::Debug for Record {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Paths stay out of logs; tests compare records whole.
        f.debug_struct("Record")
            .field("kind", &self.kind())
            .finish_non_exhaustive()
    }
}

/// Why a record couldn't be written or read. It never quotes the record.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordError {
    /// Longer than [`RECORD_CAP`] bytes.
    #[error("the record is longer than {RECORD_CAP} bytes")]
    TooLong,
    /// Not a record of this shape; where it failed.
    #[error("not a record (column {0})")]
    Malformed(usize),
    /// The stream ended inside a record.
    #[error("the stream ended inside a record")]
    Truncated,
    /// The pipe failed.
    #[error("the denial pipe failed: {0}")]
    Io(std::io::ErrorKind),
}

/// `record` as one line of the pipe, its newline included; refused past [`RECORD_CAP`], so
/// nothing is written that the host would refuse.
pub fn encode(record: &Record) -> Result<Vec<u8>, RecordError> {
    let mut bytes =
        serde_json::to_vec(record).map_err(|error| RecordError::Malformed(error.column()))?;
    bytes.push(b'\n');
    if bytes.len() > RECORD_CAP {
        return Err(RecordError::TooLong);
    }
    Ok(bytes)
}

/// The record one line holds, with or without its newline: no unknown field or kind, at most
/// [`RECORD_CAP`] bytes.
pub fn decode(line: &[u8]) -> Result<Record, RecordError> {
    if line.len() > RECORD_CAP {
        return Err(RecordError::TooLong);
    }
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    serde_json::from_slice(line).map_err(|error| RecordError::Malformed(error.column()))
}

/// The host's reader of the pipe: each record in turn. A line it can't parse (malformed, an
/// unknown kind, past [`RECORD_CAP`], or cut short by the stream's end) is skipped and the
/// next one read, so one bad record never drops the denials after it; the first skip is logged,
/// once for the reader (one session), and never with the line's words.
pub struct RecordReader<R> {
    reader: R,
    skipped: u64,
}

impl<R: BufRead> RecordReader<R> {
    /// A reader of `reader`, the host's end of one session's pipe.
    pub fn new(reader: R) -> RecordReader<R> {
        RecordReader { reader, skipped: 0 }
    }

    /// The next record, or `None` at the end of the stream; an error only when the pipe fails.
    pub fn read(&mut self) -> io::Result<Option<Record>> {
        let cap = u64::try_from(RECORD_CAP)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        loop {
            let mut line = Vec::new();
            (&mut self.reader).take(cap).read_until(b'\n', &mut line)?;
            if line.is_empty() {
                return Ok(None);
            }
            let record = if line.last() == Some(&b'\n') {
                decode(&line)
            } else if line.len() > RECORD_CAP {
                // The rest of the over-long line goes too, up to its newline.
                self.reader.skip_until(b'\n')?;
                Err(RecordError::TooLong)
            } else {
                Err(RecordError::Truncated)
            };
            match record {
                Ok(record) => return Ok(Some(record)),
                Err(error) => self.skip(&error),
            }
        }
    }

    /// How many lines were skipped so far.
    pub fn skipped(&self) -> u64 {
        self.skipped
    }

    /// Counts a skipped line, logging the first: the error never quotes it.
    fn skip(&mut self, error: &RecordError) {
        if self.skipped == 0 {
            tracing::warn!(%error, "the denial pipe sent a line that isn't a record; skipped");
        }
        self.skipped = self.skipped.saturating_add(1);
    }
}

#[cfg(test)]
mod tests;

// Two attributes, not `all(windows, test)`: clippy's test allowances look for `cfg(test)`.
#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
