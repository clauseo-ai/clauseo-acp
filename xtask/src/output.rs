//! The only place xtask writes to stdout. Every line the xtask shows goes through a `Printer`, so the
//! output format and the live-or-captured rule live in one place.

use crate::steps::{StepResult, StepStatus};
use crate::violation::Violation;
use std::fmt::Display;
use std::io::{self, IsTerminal, Write};

/// Writes the xtask's lines to `out`. When `live`, tool output streams as it arrives; otherwise it
/// is held back and only a failure shows it.
pub struct Printer<W: Write> {
    out: W,
    live: bool,
}

impl Printer<io::Stdout> {
    /// The printer for this process's stdout. Live when stdout is a terminal.
    pub fn stdout() -> Self {
        Printer::new(io::stdout(), io::stdout().is_terminal())
    }
}

impl<W: Write> Printer<W> {
    pub fn new(out: W, live: bool) -> Self {
        Printer { out, live }
    }

    /// The writer back, for tests that read what was printed.
    #[cfg(test)]
    pub fn into_inner(self) -> W {
        self.out
    }

    // Every write below ignores its error: a closed stdout must not stop the check.

    /// `==> name: command`, which starts a step.
    pub fn header(&mut self, name: &str, command: impl Display) {
        let _ = writeln!(self.out, "==> {name}: {command}");
    }

    /// One line of plain text.
    pub fn note(&mut self, text: impl Display) {
        let _ = writeln!(self.out, "{text}");
    }

    /// One finding: `path:line: what`, then its `    fix: ...` line.
    pub fn violation(&mut self, found: &Violation) {
        let _ = writeln!(self.out, "{found}");
    }

    /// A failure with no file and line: the message on one line, then its fix.
    pub fn failure(&mut self, message: &str, fix: &str) {
        let _ = writeln!(self.out, "{message}\n    fix: {fix}");
    }

    /// One chunk of raw tool output. Written and flushed only when live.
    pub fn tool_output(&mut self, chunk: &[u8]) {
        if self.live {
            let _ = self.out.write_all(chunk);
            let _ = self.out.flush();
        }
    }

    /// The whole captured output of a failed tool. Written only when not live, since a live run
    /// has already streamed it. A missing final newline is added.
    pub fn captured_output(&mut self, text: &str) {
        if self.live || text.is_empty() {
            return;
        }
        let _ = self.out.write_all(text.as_bytes());
        if !text.ends_with('\n') {
            let _ = self.out.write_all(b"\n");
        }
    }

    /// The closing `summary:` block, one line per step.
    pub fn summary(&mut self, results: &[StepResult]) {
        let _ = writeln!(self.out, "summary:");
        for result in results {
            let _ = writeln!(self.out, "{}", summary_line(result));
        }
    }
}

/// `fmt ok (0.0 s)`, `clippy ok (1 warning, 0.1 s)`, `clippy FAILED (0.2 s)`,
/// `nextest ok (skipped: no changed packages)`.
pub fn summary_line(result: &StepResult) -> String {
    match &result.status {
        StepStatus::Skipped(reason) => format!("{} ok (skipped: {reason})", result.name),
        StepStatus::Ran {
            failed,
            warnings,
            secs,
        } => {
            let status = if *failed { "FAILED" } else { "ok" };
            let mut parts = Vec::new();
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
        assert!(live.into_inner().is_empty());
    }
}
