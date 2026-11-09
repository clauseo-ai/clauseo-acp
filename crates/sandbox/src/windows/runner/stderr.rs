//! What the runner says on its stderr, and the one place it says it.
//!
//! The host copies a redacted tail of the outer runner's stderr into the session's detach reason,
//! which reaches the matter's event log and the app's details view. Its redactor hides the home
//! folder and secrets, not every path, so the runner never puts a path, a name or a value there:
//! each line it writes is one of
//!
//! ```text
//! ERR clauseo-sandbox-run kind=<kind>: <sentence>
//! NOTE clauseo-sandbox-run kind=<kind>: <sentence>
//! ```
//!
//! [`refuse`] writes the first (a refusal: the run fails with the kind's [`RunExit`]), [`notice`]
//! the second (something went wrong but refuses nothing). The kind is a [`RunnerKind`], whose
//! `snake_case` name the host maps to its own error codes, so a name, once given, never changes;
//! the sentence is the kind's own, fixed, plain English. The agent's own stderr, relayed byte for
//! byte, goes through [`agent_stream`].
//!
//! What failed in detail (a call and its code, an error's text, a pid) is a [`Detail`]: a closure
//! the caller hands over. A build with the `vm-harness-detail` feature, which only the s15 VM
//! harness enables, calls it and writes its text on the lines after, each prefixed `DETAIL `. Any
//! other build never calls it, so the text is never even formatted; no environment variable
//! changes that. The `runner-stderr` xtask check fails any other write to stderr in the runner's
//! files, and any formatting of an error outside a [`Detail::new`] closure.

use std::io::{self, Write};

// The detail lines are the VM harness's, in its debug builds: never a release's.
#[cfg(all(feature = "vm-harness-detail", not(debug_assertions)))]
compile_error!("vm-harness-detail is for the s15 VM harness's debug builds, never a release");

#[cfg(doc)]
use super::RunExit;
pub use kinds::{KindClass, RunnerKind};

mod kinds;

/// The lawyer's notice when the git watch's pairing is lost for the session
/// (`git_watch::lose_pairing`), on the denial pipe and as [`RunnerKind::GitPairingLost`]'s
/// sentence: from then on every new folder that goes before its check counts toward
/// `git_watch::GONE_LIMIT`.
pub(crate) const PAIRING_LOST: &str = "Clauseo can't follow folders the AI tool moves in this \
     session, so it will stop the session if the AI tool keeps moving or removing new folders in \
     the matter before Clauseo can check them.";

/// What every line names after its tag.
const PROGRAM: &str = "clauseo-sandbox-run";
/// The tag of a line [`refuse`] writes.
const REFUSAL: &str = "ERR";
/// The tag of a line [`notice`] writes.
const NOTICE: &str = "NOTE";
/// The tag of each detail line, in a `vm-harness-detail` build alone.
#[cfg(feature = "vm-harness-detail")]
const DETAIL: &str = "DETAIL";

/// What failed, in detail, for a `vm-harness-detail` build's `DETAIL` lines: an error's text, a
/// call and its code, a pid. Made from a closure that only such a build (and this crate's unit
/// tests, which never print it) calls; every other build drops it uncalled, so nothing in it is
/// ever formatted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Detail {
    #[cfg(any(test, feature = "vm-harness-detail"))]
    text: Option<String>,
}

impl Detail {
    /// The detail `text` gives, called only in a `vm-harness-detail` build (or a unit test).
    #[cfg(any(test, feature = "vm-harness-detail"))]
    pub fn new(text: impl FnOnce() -> String) -> Detail {
        Detail { text: Some(text()) }
    }

    /// No detail: `text` is dropped uncalled.
    #[cfg(not(any(test, feature = "vm-harness-detail")))]
    pub fn new(text: impl FnOnce() -> String) -> Detail {
        drop(text);
        Detail {}
    }

    /// No detail at all.
    pub fn none() -> Detail {
        Detail::default()
    }

    /// The detail the inner runner sent over `ctl` ([`Detail::to_send`]).
    pub fn sent(text: Option<String>) -> Detail {
        #[cfg(any(test, feature = "vm-harness-detail"))]
        {
            Detail { text }
        }
        #[cfg(not(any(test, feature = "vm-harness-detail")))]
        {
            drop(text);
            Detail {}
        }
    }

    /// What the inner runner sends over `ctl` for the outer to write: the text in a
    /// `vm-harness-detail` build, else nothing.
    pub fn to_send(&self) -> Option<String> {
        #[cfg(feature = "vm-harness-detail")]
        {
            self.text.clone()
        }
        #[cfg(not(feature = "vm-harness-detail"))]
        {
            let Detail { .. } = self;
            None
        }
    }

    /// This detail, `more` added after it (only in a build that keeps details).
    #[must_use]
    pub fn then(self, more: impl FnOnce() -> String) -> Detail {
        #[cfg(any(test, feature = "vm-harness-detail"))]
        {
            let more = more();
            Detail {
                text: Some(match self.text {
                    Some(text) => format!("{text}; {more}"),
                    None => more,
                }),
            }
        }
        #[cfg(not(any(test, feature = "vm-harness-detail")))]
        {
            drop(more);
            self
        }
    }

    /// This detail's text put in a sentence by `with` (only in a build that keeps details).
    #[must_use]
    pub fn wrap(self, with: impl FnOnce(&str) -> String) -> Detail {
        #[cfg(any(test, feature = "vm-harness-detail"))]
        {
            Detail {
                text: Some(with(self.text.as_deref().unwrap_or_default())),
            }
        }
        #[cfg(not(any(test, feature = "vm-harness-detail")))]
        {
            drop(with);
            self
        }
    }

    /// Its text, for this crate's unit tests ("" for none).
    #[cfg(test)]
    pub(crate) fn text(&self) -> &str {
        self.text.as_deref().unwrap_or_default()
    }
}

impl From<&'static str> for Detail {
    /// A fixed detail: the code's own words.
    fn from(text: &'static str) -> Detail {
        Detail::new(|| text.to_owned())
    }
}

/// Writes a refusal of `kind` on this process's stderr: one line,
/// `ERR clauseo-sandbox-run kind=<kind>: <sentence>`, then, in a `vm-harness-detail` build alone,
/// `detail`'s lines.
pub fn refuse(kind: RunnerKind, detail: Detail) {
    debug_assert!(!kind.is_notice(), "{} is a notice's kind", kind.name());
    drop(write_line(&mut io::stderr().lock(), REFUSAL, kind, detail));
}

/// Writes a notice of `kind` on this process's stderr: one line,
/// `NOTE clauseo-sandbox-run kind=<kind>: <sentence>`, then, in a `vm-harness-detail` build
/// alone, `detail`'s lines.
pub fn notice(kind: RunnerKind, detail: Detail) {
    debug_assert!(kind.is_notice(), "{} is a refusal's kind", kind.name());
    drop(write_line(&mut io::stderr().lock(), NOTICE, kind, detail));
}

/// This process's stderr, for the agent's own stderr the outer relays byte for byte (its
/// `copy` takes the unlocked handle, so no write waits on a copy blocked in a read).
#[cfg(windows)]
pub(super) fn agent_stream() -> io::Stderr {
    io::stderr()
}

/// The line `tag` (`ERR` or `NOTE`) of `kind`, then `detail`'s, flushed: what [`refuse`] and
/// [`notice`] write. It takes the detail whole: only a `vm-harness-detail` build writes it.
#[cfg_attr(
    not(feature = "vm-harness-detail"),
    expect(
        unused_variables,
        clippy::needless_pass_by_value,
        reason = "the detail is dropped unwritten outside a vm-harness-detail build"
    )
)]
fn write_line(out: &mut impl Write, tag: &str, kind: RunnerKind, detail: Detail) -> io::Result<()> {
    writeln!(
        out,
        "{tag} {PROGRAM} kind={}: {}",
        kind.name(),
        kind.sentence()
    )?;
    #[cfg(feature = "vm-harness-detail")]
    if let Some(text) = detail.text {
        for line in text.lines() {
            writeln!(out, "{DETAIL} {line}")?;
        }
    }
    out.flush()
}

#[cfg(test)]
mod tests;
