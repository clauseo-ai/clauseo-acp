//! What `Effective::load` reports instead of failing.

use std::fmt;
use std::path::PathBuf;

/// A problem `Effective::load` worked round: the value or file it names was left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// Where the problem is.
    pub origin: Origin,
    /// What it is.
    pub kind: WarningKind,
}

/// Where a [`Warning`] comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// A JSON file: `settings.json` or a `policy.json`.
    File {
        /// The file.
        path: PathBuf,
        /// The value's place in it; `None` when the problem is the whole file.
        position: Option<Position>,
    },
    /// An OS policy store, named as [`crate::PolicySource::name`] names it.
    Store {
        /// The store's name.
        name: String,
    },
}

/// A place in a file. Both count from 1; `column` counts bytes, as the JSON parser does. A
/// syntax error at the end of the input (an unclosed object, say) has column 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// The line.
    pub line: usize,
    /// The column.
    pub column: usize,
}

/// What a [`Warning`] says.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WarningKind {
    /// The file exists but could not be read; defaults are used.
    Unreadable {
        /// What the OS said.
        message: String,
    },
    /// The file is not valid JSON with comments; defaults are used.
    Syntax {
        /// What the parser said.
        message: String,
    },
    /// A key this version does not know, left out. A newer version may know it.
    UnknownKey {
        /// The dotted key, such as `ui.colour`.
        key: String,
    },
    /// A known key with a value it cannot take, left out.
    InvalidValue {
        /// The dotted key.
        key: String,
        /// Why the value was refused.
        message: String,
    },
    /// An agent whose `command` is not an absolute path, left out.
    RelativeAgentCommand {
        /// The agent's id.
        id: String,
        /// The command as written.
        command: String,
    },
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.origin {
            Origin::File {
                path,
                position: Some(Position { line, column }),
            } => write!(f, "{}:{line}:{column}: ", path.display())?,
            Origin::File {
                path,
                position: None,
            } => write!(f, "{}: ", path.display())?,
            Origin::Store { name } => write!(f, "{name}: ")?,
        }
        match &self.kind {
            WarningKind::Unreadable { message } => write!(f, "cannot read the file: {message}"),
            WarningKind::Syntax { message } => write!(f, "not valid JSON: {message}"),
            WarningKind::UnknownKey { key } => write!(f, "unknown key {key}, left out"),
            WarningKind::InvalidValue { key, message } => {
                write!(f, "{key} left out: {message}")
            }
            WarningKind::RelativeAgentCommand { id, command } => write!(
                f,
                "agent {id} left out: its command {command} is not an absolute path"
            ),
        }
    }
}
