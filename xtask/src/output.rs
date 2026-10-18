//! The only place xtask writes to stdout. Every line the xtask shows goes through a `Printer`, so
//! the output format and the live-or-captured rule live in one place.

use crate::steps::{StepResult, StepStatus};
use crate::violation::Violation;
use std::fmt::Display;
use std::io::{self, IsTerminal, Write};

/// Writes the xtask's lines to `out`. When `live`, tool output streams as it arrives; otherwise it
/// is held back and only a failure shows it.
pub(crate) struct Printer<W: Write> {
    out: W,
    live: bool,
}

impl Printer<io::Stdout> {
    /// The printer for this process's stdout. Live when stdout is a terminal.
    pub(crate) fn stdout() -> Self {
        Printer::new(io::stdout(), io::stdout().is_terminal())
    }
}

impl<W: Write> Printer<W> {
    pub(crate) fn new(out: W, live: bool) -> Self {
        Printer { out, live }
    }

    /// The writer back, for tests that read what was printed.
    #[cfg(test)]
    pub(crate) fn into_inner(self) -> W {
        self.out
    }

    /// Every write goes through here. It writes and flushes `bytes` and ignores the error on
    /// purpose: a closed stdout must not stop the check.
    #[expect(
        clippy::let_underscore_must_use,
        reason = "a closed stdout must not stop the check"
    )]
    fn put(&mut self, bytes: &[u8]) {
        let _ = self.out.write_all(bytes).and_then(|()| self.out.flush());
    }

    /// `text` and a newline.
    fn line(&mut self, text: impl Display) {
        self.put(format!("{text}\n").as_bytes());
    }

    /// `==> name: command`, which starts a step.
    pub(crate) fn header(&mut self, name: &str, command: impl Display) {
        self.line(format_args!("==> {name}: {command}"));
    }

    /// One line of plain text.
    pub(crate) fn note(&mut self, text: impl Display) {
        self.line(text);
    }

    /// One finding: `path:line: what`, then its `    fix: ...` line.
    pub(crate) fn violation(&mut self, found: &Violation) {
        self.line(found);
    }

    /// A failure with no file and line: the message on one line, then its fix.
    pub(crate) fn failure(&mut self, message: &str, fix: &str) {
        self.line(format_args!("{message}\n    fix: {fix}"));
    }

    /// One chunk of raw tool output. Written only when live.
    pub(crate) fn tool_output(&mut self, chunk: &[u8]) {
        if self.live {
            self.put(chunk);
        }
    }

    /// The whole captured output of a failed tool. Written only when not live, since a live run
    /// has already streamed it. A missing final newline is added.
    pub(crate) fn captured_output(&mut self, text: &str) {
        if !self.live {
            self.held_output(text);
        }
    }

    /// Output that was never streamed (a tool whose raw output is for machines), written live or
    /// not. A missing final newline is added.
    pub(crate) fn held_output(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if text.ends_with('\n') {
            self.put(text.as_bytes());
        } else {
            self.line(text);
        }
    }

    /// The line that ends a static check's step: its summary line, with the count.
    pub(crate) fn result(&mut self, result: &StepResult) {
        self.line(summary_line(result));
    }

    /// The closing `summary:` block, one line per step.
    pub(crate) fn summary(&mut self, results: &[StepResult]) {
        self.line("summary:");
        for result in results {
            self.line(summary_line(result));
        }
    }
}

/// `fmt ok (0.0 s)`, `clippy ok (1 warning, 0.1 s)`, `clippy FAILED (0.2 s)`,
/// `capabilities ok (1 capability file inspected, 0.0 s)`,
/// `nextest ok (skipped: no changed packages)`.
pub(crate) fn summary_line(result: &StepResult) -> String {
    match &result.status {
        StepStatus::Skipped(reason) => format!("{} ok (skipped: {reason})", result.name),
        StepStatus::Ran {
            failed,
            warnings,
            secs,
            inspected,
        } => {
            let status = if *failed { "FAILED" } else { "ok" };
            let mut parts: Vec<String> = inspected.iter().cloned().collect();
            if *warnings > 0 {
                let noun = if *warnings == 1 {
                    "warning"
                } else {
                    "warnings"
                };
                parts.push(format!("{warnings} {noun}"));
            }
            parts.push(format!("{secs:.1} s"));
            format!("{} {status} ({})", result.name, parts.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ran(name: &str, failed: bool, warnings: usize, secs: f64) -> StepResult {
        StepResult {
            name: name.into(),
            status: StepStatus::Ran {
                failed,
                warnings,
                secs,
                inspected: None,
            },
        }
    }

    #[test]
    fn summary_lines_for_ran_steps() {
        assert_eq!(summary_line(&ran("fmt", false, 0, 0.0)), "fmt ok (0.0 s)");
        assert_eq!(
            summary_line(&ran("clippy", false, 1, 0.1)),
            "clippy ok (1 warning, 0.1 s)"
        );
        assert_eq!(
            summary_line(&ran("clippy", false, 2, 3.1)),
            "clippy ok (2 warnings, 3.1 s)"
        );
        assert_eq!(
            summary_line(&ran("clippy", true, 0, 0.2)),
            "clippy FAILED (0.2 s)"
        );
    }

    #[test]
    fn a_static_check_shows_its_count_and_fails_on_zero() {
        let counted = |inspected: &str, failed| StepResult {
            name: "capabilities".into(),
            status: StepStatus::Ran {
                failed,
                warnings: 0,
                secs: 0.0,
                inspected: Some(inspected.into()),
            },
        };
        assert_eq!(
            summary_line(&counted("1 capability file inspected", false)),
            "capabilities ok (1 capability file inspected, 0.0 s)"
        );
        assert_eq!(
            summary_line(&counted("0 capability files inspected", true)),
            "capabilities FAILED (0 capability files inspected, 0.0 s)"
        );
    }

    #[test]
    fn summary_line_for_a_skipped_step() {
        let skipped = StepResult {
            name: "nextest".into(),
            status: StepStatus::Skipped("no changed packages"),
        };
        assert_eq!(
            summary_line(&skipped),
            "nextest ok (skipped: no changed packages)"
        );
    }

    #[test]
    fn captured_output_is_written_only_when_not_live_and_ends_with_a_newline() {
        let mut quiet = Printer::new(Vec::new(), false);
        quiet.captured_output("no newline");
        assert_eq!(quiet.into_inner(), b"no newline\n");

        let mut live = Printer::new(Vec::new(), true);
        live.captured_output("already streamed\n");
        assert_eq!(live.into_inner(), b"");
    }
}
