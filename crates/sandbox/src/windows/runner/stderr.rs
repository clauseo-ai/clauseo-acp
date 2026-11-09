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
//! byte, goes through [`agent_stream`]. It shares the stream, so the runner's own line always
//! starts on a fresh line: when the agent's last relayed byte isn't a newline, [`refuse`] and
//! [`notice`] write one first.
//!
//! A panic says nothing but its kind either: [`install_panic_hook`], which each of the binary's
//! modes calls first, replaces std's hook (its `panicked at <source path>:<line>`, its message and
//! any backtrace) with one [`RunnerKind::Internal`] refusal.
//!
//! What failed in detail (a call and its code, an error's text, a pid) is a [`Detail`]: a closure
//! the caller hands over. A build with the `vm-harness-detail` feature, which only the s15 VM
//! harness enables, calls it and writes its text on the lines after, each prefixed `DETAIL `. Any
//! other build never calls it, so the text is never even formatted; no environment variable
//! changes that. The `runner-stderr` xtask check fails any other write to stderr in the runner's
//! files, and any formatting of an error outside a [`Detail::new`] closure.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

// The detail lines are the VM harness's, in its debug builds: never a release's. The build script
// sets `clauseo_release_profile` when cargo's `PROFILE` is `release` (the release profile, or one
// that inherits it), whatever its `debug-assertions`; a build without debug assertions fails too.
#[cfg(all(
    feature = "vm-harness-detail",
    any(clauseo_release_profile, not(debug_assertions))
))]
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

/// True while the last byte written to this process's stderr is the agent's and not a newline:
/// the runner's next line then starts with one. Read and set only with stderr's lock held.
static MID_LINE: AtomicBool = AtomicBool::new(false);

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
    drop(write_line(
        &mut io::stderr().lock(),
        &MID_LINE,
        REFUSAL,
        kind,
        detail,
    ));
}

/// Writes a notice of `kind` on this process's stderr: one line,
/// `NOTE clauseo-sandbox-run kind=<kind>: <sentence>`, then, in a `vm-harness-detail` build
/// alone, `detail`'s lines.
pub fn notice(kind: RunnerKind, detail: Detail) {
    debug_assert!(kind.is_notice(), "{} is a refusal's kind", kind.name());
    drop(write_line(
        &mut io::stderr().lock(),
        &MID_LINE,
        NOTICE,
        kind,
        detail,
    ));
}

/// Replaces std's panic hook with one that writes a [`RunnerKind::Internal`] refusal and
/// nothing else: no `panicked at`, no source path, no message, no backtrace, whatever
/// `RUST_BACKTRACE` says. (A `vm-harness-detail` build adds the panic's text as the detail.) In a
/// release build the process then aborts. Each of the binary's modes calls it first. What std
/// writes itself on a failed allocation (`memory allocation of <n> bytes failed`) is not a panic
/// and stays.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        refuse(RunnerKind::Internal, Detail::new(|| info.to_string()));
    }));
}

/// This process's stderr, for the agent's own stderr the outer relays byte for byte. Each write
/// takes stderr's lock alone (its `copy` holds none, so no write waits on a copy blocked in a
/// read), and notes whether the agent's output ended mid-line.
#[cfg(windows)]
pub(super) fn agent_stream() -> AgentStream {
    AgentStream
}

/// See [`agent_stream`].
#[cfg(windows)]
pub(super) struct AgentStream;

#[cfg(windows)]
impl Write for AgentStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        relay(&mut io::stderr().lock(), &MID_LINE, buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stderr().lock().flush()
    }
}

/// Writes what it can of the agent's `buf` to `out`, and sets `mid_line` when the last byte
/// written isn't a newline (clears it when it is).
#[cfg(any(windows, test))]
fn relay(out: &mut impl Write, mid_line: &AtomicBool, buf: &[u8]) -> io::Result<usize> {
    let written = out.write(buf)?;
    if let Some(&last) = buf.get(..written).and_then(<[u8]>::last) {
        mid_line.store(last != b'\n', Ordering::Relaxed);
    }
    Ok(written)
}

/// The line `tag` (`ERR` or `NOTE`) of `kind`, then `detail`'s, flushed: what [`refuse`] and
/// [`notice`] write. A newline goes first when `mid_line` says the agent's output ended mid-line,
/// so the kind line starts a line. It takes the detail whole: only a `vm-harness-detail` build
/// writes it.
#[cfg_attr(
    not(feature = "vm-harness-detail"),
    expect(
        unused_variables,
        clippy::needless_pass_by_value,
        reason = "the detail is dropped unwritten outside a vm-harness-detail build"
    )
)]
fn write_line(
    out: &mut impl Write,
    mid_line: &AtomicBool,
    tag: &str,
    kind: RunnerKind,
    detail: Detail,
) -> io::Result<()> {
    if mid_line.swap(false, Ordering::Relaxed) {
        writeln!(out)?;
    }
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
