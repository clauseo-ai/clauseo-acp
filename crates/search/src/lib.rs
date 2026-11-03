//! `clauseo-search`: search inside matters and conversations, in a derived SQLite FTS5 index.
//!
//! # Owns
//!
//! `search.sqlite` (with its `-wal` and `-shm`) in the folder the caller passes: its schema
//! ([`FORMAT`] and [`TOKENIZER`], kept in its `meta` table), the private file mode, the FTS5
//! query text built from what the lawyer typed, ranking, paging and snippets.
//! `docs/decisions/0043-search.md` records the format and the reason for each choice.
//!
//! # Interface
//!
//! Synchronous: the caller runs it on a thread of its own. [`SearchIndex::open`] opens or creates
//! the file and returns an [`OpenState`]; `needs_rebuild` is true when the file was missing, did
//! not open, had another [`FORMAT`] or [`TOKENIZER`], or holds an unfinished build, and the index
//! is then empty. Writes: [`SearchIndex::put_entry`], [`SearchIndex::put_thread_title`],
//! [`SearchIndex::put_matter_title`], [`SearchIndex::put_file_names`],
//! [`SearchIndex::set_archived`], [`SearchIndex::delete_entries`] (an undone step),
//! [`SearchIndex::delete_thread`] and [`SearchIndex::delete_matter`], each its own transaction,
//! or many in one through [`SearchIndex::batch`] and a [`Batch`]. [`Batch::set_watermark`]
//! records, in the same transaction as the rows, the last `seq` of a stream the index reflects;
//! [`SearchIndex::watermarks`] reads them back, for a caller that catches up from the log. A rebuild runs between
//! [`SearchIndex::start_rebuild`] and [`SearchIndex::finish_rebuild`]; each batch counts its
//! conversations with [`Batch::conversation_done`], and [`Progress`] reports them.
//! Read with [`SearchIndex::query`]: a [`Query`] gives a [`Page`] of [`Hit`]s, each with a
//! snippet as [`SnippetPart`]s, and a [`Cursor`] for the next page, which
//! [`Cursor::encode`] and [`Cursor::decode`] turn into text and back; a cursor that does not
//! belong to its query starts over from the first page. [`Interrupter`] stops a
//! running query from another thread.
//!
//! # Invariants
//!
//! - The file is derived: everything in it can be rebuilt from the store's log. A format or
//!   tokenizer change bumps [`FORMAT`]; nothing is migrated, the file is emptied and rebuilt.
//! - Text the lawyer typed never reaches `MATCH` as FTS5 syntax: every word is quoted.
//! - A row is keyed by what it shows: an entry or a tool title by (thread, entry), a title by
//!   its thread or matter, file names by matter. A put replaces the row with the same key.
//! - Deletes remove the text from the index too: FTS5 `secure-delete` and SQLite's
//!   `secure_delete` are on, and a delete outside a batch ends with a WAL checkpoint.
//! - The control characters U+0002 and U+0003 mark matches inside snippets, so they are
//!   stripped from text before it is stored.
//! - No clock: [`Query::now`] is the time recency is measured from.

mod cursor;
mod error;
mod fts;
mod index;
mod open;
mod query;
mod snippet;
mod write;

pub use cursor::Cursor;
pub use error::Error;
pub use index::{Batch, Interrupter, SearchIndex};
pub use open::{FILE_NAME, FORMAT, OpenState, TOKENIZER};
pub use query::{MAX_LIMIT, Mode, Page, Progress, Query, WINDOW};
pub use snippet::{MAX_SNIPPET_CHARS, SNIPPET_TOKENS};

use clauseo_protocol::{MatterId, Seq, ThreadId, Timestamp};

/// What a row holds. The number is its `kind` column, part of [`FORMAT`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A matter's name.
    MatterTitle,
    /// A conversation's title.
    ThreadTitle,
    /// The lawyer's prompt.
    Prompt,
    /// One agent message of kind answer.
    Answer,
    /// A tool call's title.
    ToolTitle,
    /// A document's path in the matter folder.
    FileName,
    /// A part of a document's text (reserved for papers).
    PaperText,
    /// An agent message of kind thought (reserved: not indexed on day one).
    Thought,
}

impl Kind {
    pub(crate) const fn code(self) -> i64 {
        match self {
            Self::MatterTitle => 1,
            Self::ThreadTitle => 2,
            Self::Prompt => 3,
            Self::Answer => 4,
            Self::ToolTitle => 5,
            Self::FileName => 6,
            Self::PaperText => 7,
            Self::Thought => 8,
        }
    }

    pub(crate) const fn from_code(code: i64) -> Option<Self> {
        Some(match code {
            1 => Self::MatterTitle,
            2 => Self::ThreadTitle,
            3 => Self::Prompt,
            4 => Self::Answer,
            5 => Self::ToolTitle,
            6 => Self::FileName,
            7 => Self::PaperText,
            8 => Self::Thought,
            _ => return None,
        })
    }
}

/// The two kinds of conversation entry [`SearchIndex::put_entry`] takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    /// The lawyer's prompt.
    Prompt,
    /// An agent message of kind answer.
    Answer,
    /// A tool call's title, keyed by the tool call's order like an entry.
    ToolTitle,
}

impl EntryKind {
    pub(crate) const fn kind(self) -> Kind {
        match self {
            Self::Prompt => Kind::Prompt,
            Self::Answer => Kind::Answer,
            Self::ToolTitle => Kind::ToolTitle,
        }
    }
}

/// One result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// What matched.
    pub kind: Kind,
    /// The matter it belongs to.
    pub matter: MatterId,
    /// The conversation, for a thread title or an entry.
    pub thread: Option<ThreadId>,
    /// The entry's order (the seq of its first event), for a prompt or an answer.
    pub entry: Option<Seq>,
    /// The document's path, for a file name.
    pub path: Option<String>,
    /// The matter's name as last put, if one was.
    pub matter_title: Option<String>,
    /// The conversation's title as last put, for a hit in a titled conversation.
    pub thread_title: Option<String>,
    /// When the text was written.
    pub at: Timestamp,
    /// The text around the match, at most [`MAX_SNIPPET_CHARS`] characters.
    pub snippet: Vec<SnippetPart>,
}

/// A run of snippet text: joined in order, the parts are the snippet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnippetPart {
    /// The text.
    pub text: String,
    /// Whether it is a match of a query word.
    pub hit: bool,
}
