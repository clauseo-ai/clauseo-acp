//! The only module that prints. Every line the xtask shows goes through one of these functions,
//! so the output format lives in one place.

use crate::steps::{StepResult, StepStatus};
use crate::violation::Violation;
use std::fmt::Display;
use std::io::{self, Write};

/// `==> name: command`, which starts a step.
pub fn header(name: &str, command: impl Display) {
    println!("==> {name}: {command}");
}

/// One line of plain text.
pub fn note(text: impl Display) {
    println!("{text}");
}

/// One finding: `path:line: what`, then its `    fix: ...` line.
pub fn violation(found: &Violation) {
    println!("{found}");
}

/// A failure with no file and line: the message on one line, then its fix.
pub fn failure(message: &str, fix: &str) {
    println!("{message}\n    fix: {fix}");
}

/// Raw tool output, written as it arrives, so tool output and our lines share one ordered stream.
pub fn tool_output(chunk: &[u8]) {
    let mut stdout = io::stdout().lock();
    // A closed stdout is not a reason to stop checking, so write errors are ignored.
    let _ = stdout.write_all(chunk);
    let _ = stdout.flush();
}

/// The closing `summary:` block, one line per step.
pub fn summary(results: &[StepResult]) {
    println!("summary:");
    for result in results {
        println!("{}", summary_line(result));
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
}
