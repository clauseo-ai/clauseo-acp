//! Runs the steps in order. Every header, finding and summary line goes through the `Printer`, and
//! so does tool output: it streams when stdout is a terminal and is held back otherwise.

use crate::checks::StaticCheck;
use crate::clock::timed;
use crate::output::Printer;
use crate::process::{Captured, Cmd, Program};
use crate::steps::{Step, ToolStep};
use crate::workspace::Context;
use std::io::{self, Write};

/// Runs a tool command and passes each chunk of its output to the sink as it arrives.
pub(crate) type Exec<'a> = dyn FnMut(&Cmd, &mut dyn FnMut(&[u8])) -> io::Result<Captured> + 'a;

/// The summary entry for one step.
pub(crate) struct StepResult {
    pub name: String,
    pub status: StepStatus,
}

#[derive(Debug, PartialEq)]
pub(crate) enum StepStatus {
    Ran {
        failed: bool,
        warnings: usize,
        secs: f64,
        /// `1 capability file inspected` for a static check; `None` for a tool.
        inspected: Option<String>,
    },
    Skipped(&'static str),
}

/// Runs every step in order, even after one fails. Prints each step as it goes and returns one
/// result per step. `exec` runs a tool command and passes each chunk of its output to the sink as
/// it arrives; production passes `process::capture`.
pub(crate) fn run_all<W: Write>(
    steps: Vec<Step>,
    ctx: &Context,
    out: &mut Printer<W>,
    exec: &mut Exec<'_>,
) -> Vec<StepResult> {
    let mut results = Vec::new();
    for step in steps {
        let result = match step {
            Step::Skipped { name, reason } => {
                out.header(name, format!("skipped ({reason})"));
                StepResult {
                    name: name.into(),
                    status: StepStatus::Skipped(reason),
                }
            }
            Step::Static(check) => run_static(check, ctx, out),
            Step::Tool(tool) => run_tool(&tool, ctx, out, exec),
        };
        results.push(result);
    }
    results
}

/// Runs one static check through `StaticCheck::report`: prints its violations, its failures
/// without a location (the one for inspecting nothing last), its notes, then a result line with
/// the count, e.g. `capabilities ok (1 capability file inspected, 0.0 s)`.
fn run_static<W: Write>(check: &StaticCheck, ctx: &Context, out: &mut Printer<W>) -> StepResult {
    out.header(check.name, "built-in static check");
    let (report, secs) = timed(|| check.report(ctx));
    for found in &report.inspection.violations {
        out.violation(found);
    }
    for failure in report
        .inspection
        .failures
        .iter()
        .chain(&report.nothing_inspected)
    {
        out.failure(&failure.message, &failure.fix);
    }
    for note in &report.inspection.notes {
        out.note(format_args!("note: {note}"));
    }
    let result = StepResult {
        name: check.name.into(),
        status: StepStatus::Ran {
            failed: report.failed(),
            warnings: 0,
            secs,
            inspected: Some(report.counted),
        },
    };
    out.result(&result);
    result
}

fn run_tool<W: Write>(
    tool: &ToolStep,
    ctx: &Context,
    out: &mut Printer<W>,
    exec: &mut Exec<'_>,
) -> StepResult {
    out.header(tool.name, &tool.cmd);
    let stream = tool.render.is_none();
    let (captured, secs) = timed(|| {
        exec(&tool.cmd, &mut |chunk| {
            if stream {
                out.tool_output(chunk);
            }
        })
    });
    let (failed, warnings) = match captured {
        Ok(captured) => {
            let findings = (tool.parse)(&captured.output, ctx, &tool.cmd);
            for finding in &findings {
                out.violation(&finding.violation);
            }
            let has_error = findings.iter().any(|f| f.is_error);
            let warnings = findings.iter().filter(|f| !f.is_error).count();
            let failed = !captured.success;
            if failed && !has_error {
                match tool.render {
                    None => out.captured_output(&captured.output),
                    Some(render) => out.held_output(&render(&captured.output)),
                }
                out.failure(
                    &format!(
                        "`{}` failed without a file:line; its output is above",
                        tool.cmd
                    ),
                    &tool_fix(tool, ctx, Some(&captured.output)),
                );
            }
            (failed, warnings)
        }
        Err(error) => {
            let message = format!("could not run `{}`: {error}", tool.cmd.program.name());
            out.failure(&message, &tool_fix(tool, ctx, None));
            (true, 0)
        }
    };
    StepResult {
        name: tool.name.into(),
        status: StepStatus::Ran {
            failed,
            warnings,
            secs,
            inspected: None,
        },
    }
}

/// The fix of a failed tool step. When `cargo metadata` failed, a cargo step gets the metadata
/// fix instead of its own, since its own fix names a cause that this failure need not have.
fn tool_fix(tool: &ToolStep, ctx: &Context, output: Option<&str>) -> String {
    if ctx.members.is_err() && tool.cmd.program == Program::Cargo {
        METADATA_STEP_FIX.to_owned()
    } else {
        (tool.fix)(&tool.cmd, output)
    }
}

/// The fix of a cargo step that failed after `cargo metadata` failed.
pub(crate) const METADATA_STEP_FIX: &str = "fix the `cargo metadata` error in the note above first, since this step runs cargo on the same workspace manifests";

#[cfg(test)]
mod tests;
