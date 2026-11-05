//! One transaction of index writes with its stream's watermark, and why a write stopped.

use std::fmt;

use clauseo_protocol::{MatterId, Seq, StreamId, Timestamp};
use clauseo_search::{Batch, SearchIndex};

use super::fold::Write;
use crate::log_file::Cause;

/// Why a walk or a write stopped; logged, and the stream is read again later.
#[derive(Debug)]
pub(super) enum Failure {
    Store(clauseo_store::Error),
    Index(clauseo_search::Error),
}

/// For the log: a store error by its codes only (its text can hold a path); the index's errors
/// hold SQLite's messages and io errors, no matter content.
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(f, "store: {}", Cause(error)),
            Self::Index(error) => write!(f, "index: {error}"),
        }
    }
}

impl From<clauseo_store::Error> for Failure {
    fn from(error: clauseo_store::Error) -> Self {
        Self::Store(error)
    }
}

impl From<clauseo_search::Error> for Failure {
    fn from(error: clauseo_search::Error) -> Self {
        Self::Index(error)
    }
}

/// One transaction: `chunk`'s writes, the file names, and the watermark.
pub(super) fn write_chunk(
    index: &mut SearchIndex,
    chunk: &[Write],
    listing: Option<&(MatterId, Timestamp, Vec<String>)>,
    mark: Option<(StreamId, Seq)>,
) -> Result<(), Failure> {
    if chunk.is_empty() && listing.is_none() && mark.is_none() {
        return Ok(());
    }
    index.batch(|batch| {
        for write in chunk {
            put(batch, write)?;
        }
        if let Some((matter, at, paths)) = listing {
            batch.put_file_names(matter.to_owned(), *at, paths)?;
        }
        if let Some((stream, seq)) = mark {
            batch.set_watermark(stream, Some(seq))?;
        }
        Ok(())
    })?;
    Ok(())
}

fn put(batch: &mut Batch<'_>, write: &Write) -> Result<(), clauseo_search::Error> {
    match write {
        Write::Entry {
            matter,
            thread,
            order,
            kind,
            at,
            text,
        } => batch.put_entry(*matter, *thread, *order, *kind, *at, text),
        Write::ThreadTitle {
            matter,
            thread,
            at,
            title,
        } => batch.put_thread_title(*matter, *thread, *at, title),
        Write::MatterTitle { matter, at, title } => batch.put_matter_title(*matter, *at, title),
        Write::Archived { matter, thread } => batch.set_archived(*matter, *thread, true),
        Write::Undone { .. } | Write::Files { .. } => Ok(()),
    }
}
