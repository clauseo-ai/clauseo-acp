//! The runner's log file: `<data_dir>\sandbox\run\runner.log`, warn and error from Clauseo's own
//! crates (`clauseo_*` targets), one line each: the UTC RFC 3339 time, level, target, which runner
//! and its process id, the message and the fields kept. Until it, the runner set no tracing
//! subscriber, so its `tracing` lines (a pairing watch lost, a set-aside that failed, the denial
//! pipe's failures) went nowhere.
//!
//! - **Where.** The outer runner's, as the desktop user: the run folder the host makes, with its
//!   protected DACL, before it starts the outer (`write_launch`, `private_dir::create`): only the
//!   desktop user, `SYSTEM` and `Administrators` may open it, nothing inherited, so the agent (a
//!   pool user, never an administrator) can neither write nor read the file. The inner runner
//!   runs as the pool user and can't open the folder at all: it keeps no file. Its failures
//!   reach the outer as `InnerReport::Log` lines over `ctl`, as before, and no module on its
//!   path logs at warn or error through `tracing`.
//! - **Bound.** `runner.log` rotates into `runner.1.log` before it would pass [`CAP`] (1 MB), so
//!   the two hold 2 MB at most; a line is cut at [`LINE_MAX`]. Every outer runner of the
//!   install appends to the same file, each line written whole by one open-append-close
//!   (`write::append_capped`), synchronously: the runner often ends by `exit`, which a writer
//!   thread's queue wouldn't survive. A line that can't be written is dropped.
//! - **What it keeps.** Never matter content, a path in the matter, a name the agent chose or a
//!   secret: [`Kept`] writes the message and number and `bool` fields, and a string field only
//!   when its name is one whose values are the code's own words ([`WORDS`]). Any other field
//!   (`%error`, `?reason`, a string field by any other name) is written as `name=<dropped>`: an
//!   error's text can hold a path. A message is written as it is: `tracing` hands a literal and
//!   a formatted message over alike, so the `log-literals` xtask check keeps every `warn!` and
//!   `error!` under `windows/` (but `windows/setup/`) a literal with no format argument. One
//!   holding `\` or `/` is withheld too, as it might be a path.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use tracing::field::{Field, Visit};
use tracing::{Dispatch, Level, Metadata};
use tracing_subscriber::Layer;
use tracing_subscriber::field::RecordFields;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FormatFields, MakeWriter};
use tracing_subscriber::layer::SubscriberExt;

#[cfg(windows)]
use super::RUN_DIR;
#[cfg(windows)]
use crate::windows::matters::SANDBOX_DIR;
use crate::windows::write;

/// The file's name in the run folder.
pub(super) const LOG_FILE: &str = "runner.log";
/// The older file it rotates into.
pub(super) const OLD_LOG_FILE: &str = "runner.1.log";
/// The size `runner.log` rotates before passing: 1 MB.
pub(super) const CAP: u64 = 1024 * 1024;
/// The longest line kept; a longer one is cut there, ending in a newline.
pub(super) const LINE_MAX: usize = 8 * 1024;

/// The string fields kept: each call that sets one gives it the code's own words (a `Cause`'s
/// log text, a record's kind, `hand_over`'s "a set-aside item", the git watch's OS error code
/// or `ErrorKind` from `git_watch::code`).
const WORDS: [&str; 4] = ["cause", "code", "kind", "what"];

/// Starts the outer runner's log in `<data_dir>\sandbox\run\` as the process's global
/// subscriber. Never fails: a global subscriber already set leaves it as it was; a folder
/// that can't be written only drops the lines.
#[cfg(windows)]
pub(super) fn install(data_dir: &Path) {
    let run = data_dir.join(SANDBOX_DIR).join(RUN_DIR);
    let dispatch = build(LogFile::new(&run, "outer"));
    drop(tracing::dispatcher::set_global_default(dispatch));
}

/// The subscriber writing to `file`: [`kept`]'s lines, formatted by [`Redacted`].
pub(super) fn build<W>(file: W) -> Dispatch
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let layer = tracing_subscriber::fmt::layer()
        .with_writer(file)
        .fmt_fields(Redacted)
        .with_filter(filter_fn(kept));
    Dispatch::new(tracing_subscriber::registry().with(layer))
}

/// What the file keeps: warn and error from `clauseo_*` targets.
fn kept(meta: &Metadata<'_>) -> bool {
    meta.target().starts_with("clauseo_") && *meta.level() <= Level::WARN
}

/// The fields' formatter: see the module.
pub(super) struct Redacted;

impl<'writer> FormatFields<'writer> for Redacted {
    fn format_fields<R: RecordFields>(&self, writer: Writer<'writer>, fields: R) -> fmt::Result {
        let mut kept = Kept {
            writer,
            result: Ok(()),
            first: true,
        };
        fields.record(&mut kept);
        kept.result
    }
}

/// One event's fields, written as [`Redacted`] keeps them.
struct Kept<'writer> {
    writer: Writer<'writer>,
    result: fmt::Result,
    first: bool,
}

impl Kept<'_> {
    fn put(&mut self, name: &str, value: &dyn fmt::Display) {
        if self.result.is_err() {
            return;
        }
        let gap = if self.first { "" } else { " " };
        self.first = false;
        self.result = if name == "message" {
            write!(self.writer, "{gap}{value}")
        } else {
            write!(self.writer, "{gap}{name}={value}")
        };
    }

    fn dropped(&mut self, field: &Field) {
        self.put(field.name(), &"<dropped>");
    }
}

impl Visit for Kept<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() != "message" {
            self.dropped(field);
            return;
        }
        let message = format!("{value:?}");
        if message.contains(['\\', '/']) {
            self.put("message", &"<message withheld: it held a path separator>");
        } else {
            self.put("message", &message);
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if WORDS.contains(&field.name()) && !value.contains(['\\', '/']) {
            self.put(field.name(), &value);
        } else {
            self.dropped(field);
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field.name(), &value);
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field.name(), &value);
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        self.put(field.name(), &value);
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        self.put(field.name(), &value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field.name(), &value);
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.put(field.name(), &value);
    }

    fn record_error(&mut self, field: &Field, _value: &(dyn std::error::Error + 'static)) {
        self.dropped(field);
    }
}

/// The file and its older file, and what starts each line: the runner and its process id.
#[derive(Debug, Clone)]
pub(super) struct LogFile {
    path: PathBuf,
    old: PathBuf,
    who: String,
}

impl LogFile {
    /// The log in the folder `run`, for the runner `role`.
    pub(super) fn new(run: &Path, role: &str) -> LogFile {
        LogFile {
            path: run.join(LOG_FILE),
            old: run.join(OLD_LOG_FILE),
            who: format!("[{role} {}] ", std::process::id()),
        }
    }
}

impl<'a> MakeWriter<'a> for LogFile {
    type Writer = Line<'a>;

    fn make_writer(&'a self) -> Line<'a> {
        Line(self)
    }
}

/// One line: the formatter writes it whole with one `write_all`.
pub(super) struct Line<'a>(&'a LogFile);

impl io::Write for Line<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut line = self.0.who.clone().into_bytes();
        line.extend_from_slice(buf.get(..line_end(buf)).unwrap_or(buf));
        if !line.ends_with(b"\n") {
            line.push(b'\n');
        }
        // A line that can't be written is dropped: the runner never fails for its log.
        let _written = write::append_capped(&self.0.path, &self.0.old, &line, CAP);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Where `line` is cut: its end, or the last char boundary at or before [`LINE_MAX`].
fn line_end(line: &[u8]) -> usize {
    if line.len() <= LINE_MAX {
        return line.len();
    }
    let mut end = LINE_MAX;
    while end > 0
        && line
            .get(end)
            .is_some_and(|byte| byte & 0b1100_0000 == 0b1000_0000)
    {
        end -= 1;
    }
    end
}

#[cfg(test)]
mod tests;
