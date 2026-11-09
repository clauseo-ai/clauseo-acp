//! The records the outer writes on the denial pipe (`feed.rs`), one per line: what the git
//! watch set aside while the agent ran, which the host passes to the launch's sink as a
//! [`Denial::SetAside`]. Nothing read from the outer's stderr makes one: the agent's stderr is
//! relayed on it, so the agent could forge any line there.
//!
//! TODO(#789): its record-format module (`git_rename_watch3`) defines the records the outer
//! writes. Until it lands this is a stand-in: one JSON object per line,
//! `{"kind":"set_aside","path":…,"to":…,"what":"git_repository"}`. Swap [`record_denial`]
//! for that module's reader, and route its notice records as the host decides (the lawyer's
//! notice can come from the set-aside itself, `SetAsideWhat::notice`).

use std::path::PathBuf;

use serde::Deserialize;

use crate::{Denial, SetAsideWhat};

/// The longest record read; a longer line is dropped whole.
pub(super) const RECORD_CAP: usize = 64 * 1024;

/// One record, as the stand-in format writes it.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Record {
    /// The watch set an item aside.
    SetAside {
        /// Where it was, below the watched folder.
        path: PathBuf,
        /// Where it is now, in the set-aside.
        to: PathBuf,
        /// What it was.
        what: What,
    },
}

/// [`SetAsideWhat`], as the record names it.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum What {
    GitRepository,
    LinkOut,
    UncheckedFolder,
}

impl From<What> for SetAsideWhat {
    fn from(what: What) -> SetAsideWhat {
        match what {
            What::GitRepository => SetAsideWhat::GitRepository,
            What::LinkOut => SetAsideWhat::LinkOut,
            What::UncheckedFolder => SetAsideWhat::UncheckedFolder,
        }
    }
}

/// The denial `line` (one record, without its `\n`) reports; `None` for one this build can't
/// read.
pub(super) fn record_denial(line: &[u8]) -> Option<Denial> {
    match serde_json::from_slice(line).ok()? {
        Record::SetAside { path, to, what } => Some(Denial::SetAside {
            path,
            to,
            what: what.into(),
        }),
    }
}
