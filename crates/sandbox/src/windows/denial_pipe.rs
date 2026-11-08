//! The denial pipe (S15 tier B): how the outer runner tells the host what its git watch set
//! aside and what the lawyer is to be told, on a channel the agent can't write to.
//!
//! The outer's stderr carries the agent's stderr too (the relay in `runner/outer.rs`), so
//! nothing read from it is trusted: a line there never makes a [`Denial`] or a notice. Instead:
//!
//! - **The pipe:** the host makes `\\.\pipe\clauseo-denials-<32 hex>` ([`random_pipe_name`],
//!   128 random bits per launch) before it starts the runner, with [`denial_pipe_sddl`]: the
//!   lawyer alone, protected, and a Medium label with no-read-up and no-write-up, so neither a
//!   pool user nor a Low process opens it. One instance, made first or refused, inbound, local
//!   clients only ([`os::DenialServer::create`] on Windows). Its name goes in the launch message
//!   (`LaunchMessage::denial_pipe`), which only the lawyer can read; the pool user never sees it.
//! - **The client:** the outer checks the name ([`check_pipe_name`]), opens it write-only with
//!   identification-only quality of service (so the host can't act as the lawyer through it) and
//!   checks it is a pipe, before the inner runner starts. A launch with no name, or a pipe that
//!   can't be opened, leaves the records in the outer's log only, never on stderr.
//! - **The host** takes records only from a client whose pid is the runner it started
//!   (`DenialServer::client_pid`), reads them with [`read_record`] and stops at the first error.
//! - **The records:** one JSON object per line, `{"v":1,"record":{"kind":...}}` ([`encode`]),
//!   at most [`RECORD_CAP`] bytes with its newline. [`decode`] is strict: this version only,
//!   no unknown field or kind, nothing longer. A [`Record::SetAside`] becomes the session's
//!   [`Denial::SetAside`] ([`Record::denial`]); a [`Record::Notice`] is for the lawyer.
//!
//! The names, the DACL and the records hold no Windows call, so their tests run on every OS.

use std::fmt;
use std::io::{self, BufRead, Read as _};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::pipe::{PIPE_RIGHTS, PIPE_ROOT, hex};
use super::sids::Sid;
use crate::{Denial, SetAsideWhat};

#[cfg(windows)]
pub mod os;
#[cfg(any(windows, test))]
pub(crate) mod sink;

/// The start of a denial pipe's name, after `\\.\pipe\`.
pub const DENIAL_PIPE_PREFIX: &str = "clauseo-denials-";
/// The records' format version.
pub const RECORD_VERSION: u32 = 1;
/// The longest record, its newline included: two Windows paths of 32767 UTF-16 units, each at
/// most three bytes in UTF-8, with room for JSON's escapes.
pub const RECORD_CAP: usize = 256 * 1024;

/// Why a denial pipe's name was refused. It never quotes the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a denial pipe's name")]
pub struct NotADenialPipe;

/// A new denial pipe's full name, `\\.\pipe\clauseo-denials-<32 lowercase hex>`, with 128
/// random bits from the OS.
pub fn random_pipe_name() -> io::Result<String> {
    let mut bits = [0u8; 16];
    getrandom::fill(&mut bits).map_err(|error| io::Error::other(error.to_string()))?;
    Ok(format!("{PIPE_ROOT}{DENIAL_PIPE_PREFIX}{}", hex(&bits)))
}

/// Refuses `name` unless it is exactly what [`random_pipe_name`] makes: the local pipe root,
/// the prefix and 32 lowercase hex, nothing else (no server, no other folder, no `..`).
pub fn check_pipe_name(name: &str) -> Result<(), NotADenialPipe> {
    let hex = name
        .strip_prefix(PIPE_ROOT)
        .and_then(|rest| rest.strip_prefix(DENIAL_PIPE_PREFIX))
        .ok_or(NotADenialPipe)?;
    let lower_hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
    if hex.len() == 32 && hex.bytes().all(lower_hex) {
        Ok(())
    } else {
        Err(NotADenialPipe)
    }
}

/// The denial pipe's descriptor: protected, the lawyer (the desktop user the host and the outer
/// run as) with [`PIPE_RIGHTS`] and no one else, and a Medium label with no-write-up and
/// no-read-up.
pub fn denial_pipe_sddl(lawyer: &Sid) -> String {
    format!("D:P(A;;0x{PIPE_RIGHTS:x};;;{lawyer})S:(ML;;NWNR;;;ME)")
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
    /// The record of `what` moved from `path` to `to`; a part that isn't Unicode is replaced.
    pub fn set_aside(path: &Path, to: &Path, what: SetAsideWhat) -> Record {
        Record::SetAside {
            path: path.to_string_lossy().into_owned(),
            to: to.to_string_lossy().into_owned(),
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
    /// Another format version.
    #[error("the record is version {0}, this reads {RECORD_VERSION}")]
    Version(u32),
    /// Not a record of this shape; where it failed.
    #[error("not a record (column {0})")]
    Malformed(usize),
    /// The stream ended inside a record.
    #[error("the stream ended inside a record")]
    Truncated,
    /// The pipe failed.
    #[error("the denial pipe failed: {0}")]
    Io(io::ErrorKind),
}

/// One line on the pipe: the version, then the record.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Line {
    v: u32,
    record: Record,
}

/// A line's version alone, read first so another version is named as such.
#[derive(Deserialize)]
struct Version {
    v: u32,
}

/// `record` as one line of the pipe, its newline included; refused past [`RECORD_CAP`], so
/// nothing is written that the host would refuse.
pub fn encode(record: &Record) -> Result<Vec<u8>, RecordError> {
    let line = Line {
        v: RECORD_VERSION,
        record: record.clone(),
    };
    let mut bytes =
        serde_json::to_vec(&line).map_err(|error| RecordError::Malformed(error.column()))?;
    bytes.push(b'\n');
    if bytes.len() > RECORD_CAP {
        return Err(RecordError::TooLong);
    }
    Ok(bytes)
}

/// The record one line holds, with or without its newline: [`RECORD_VERSION`] only, no
/// unknown field or kind, at most [`RECORD_CAP`] bytes.
pub fn decode(line: &[u8]) -> Result<Record, RecordError> {
    if line.len() > RECORD_CAP {
        return Err(RecordError::TooLong);
    }
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let malformed = |error: serde_json::Error| RecordError::Malformed(error.column());
    let version: Version = serde_json::from_slice(line).map_err(malformed)?;
    if version.v != RECORD_VERSION {
        return Err(RecordError::Version(version.v));
    }
    let line: Line = serde_json::from_slice(line).map_err(malformed)?;
    Ok(line.record)
}

/// The next record on `reader`, or `None` at the end of the stream. It reads at most
/// [`RECORD_CAP`] bytes and one more for a line; after an error the stream is out of step, so
/// the host stops reading it.
pub fn read_record(reader: &mut impl BufRead) -> Result<Option<Record>, RecordError> {
    let mut line = Vec::new();
    let cap = u64::try_from(RECORD_CAP)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    reader
        .take(cap)
        .read_until(b'\n', &mut line)
        .map_err(|error| RecordError::Io(error.kind()))?;
    if line.is_empty() {
        return Ok(None);
    }
    if line.last() != Some(&b'\n') {
        return Err(if line.len() > RECORD_CAP {
            RecordError::TooLong
        } else {
            RecordError::Truncated
        });
    }
    decode(&line).map(Some)
}

#[cfg(test)]
mod tests;

// Two attributes, not `all(windows, test)`: clippy's test allowances look for `cfg(test)`.
#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
