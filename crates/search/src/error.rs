//! The one error type.

use std::fmt;

/// What can go wrong.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// SQLite failed.
    Sqlite(rusqlite::Error),
    /// Creating or removing the file failed.
    Io(std::io::Error),
    /// [`Interrupter::interrupt`](crate::Interrupter::interrupt) stopped the query.
    Interrupted,
    /// A cursor's text did not decode: not one [`Cursor::encode`](crate::Cursor::encode) made.
    Cursor,
    /// A row holds a value this code never writes.
    Corrupt(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(f, "search index: {error}"),
            Self::Io(error) => write!(f, "search index file: {error}"),
            Self::Interrupted => f.write_str("search interrupted"),
            Self::Cursor => f.write_str("search cursor text is not a cursor"),
            Self::Corrupt(what) => write!(f, "search index row: {what}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Interrupted | Self::Cursor | Self::Corrupt(_) => None,
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        match error.sqlite_error_code() {
            Some(rusqlite::ErrorCode::OperationInterrupted) => Self::Interrupted,
            _ => Self::Sqlite(error),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
