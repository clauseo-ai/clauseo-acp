//! Runs the steps in order. Every header, finding and summary line goes through the `Printer`, and
//! so does tool output: it streams when stdout is a terminal and is held back otherwise.

use crate::checks::StaticCheck;
use crate::output::Printer;
use crate::process::{Captured, Cmd};
use crate::steps::{Step, ToolStep};
use crate::workspace::Context;
use std::io::{self, Write};
use std::time::Instant;

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

fn run_static<W: Write>(check: &StaticCheck, ctx: &Context, out: &mut Printer<W>) -> StepResult {
    out.header(check.name, "built-in static check");
    let (violations, secs) = timed(|| (check.run)(ctx));
    for found in &violations {
        out.violation(found);
    }
    StepResult {
        name: check.name.into(),
        status: StepStatus::Ran {
            failed: !violations.is_empty(),
            warnings: 0,
            secs,
        },
    }
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
                    &(tool.fix)(&tool.cmd, Some(&captured.output)),
                );
            }
            (failed, warnings)
        }
        Err(error) => {
            let message = format!("could not run `{}`: {error}", tool.cmd.program.name());
            out.failure(&message, &(tool.fix)(&tool.cmd, None));
            (true, 0)
        }
    };
    StepResult {
        name: tool.name.into(),
        status: StepStatus::Ran {
            failed,
            warnings,
            secs,
        },
    }
}

/// Runs `work` and returns its value with the seconds it took. The clock read for the check steps;
/// `buildtime` has its own, for the build timings.
#[expect(clippy::disallowed_methods, reason = "xtask runs tools and times them")]
fn timed<T>(work: impl FnOnce() -> T) -> (T, f64) {
    let started = Instant::now();
    let value = work();
    (value, started.elapsed().as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::Program;
    use crate::violation::{Finding, Violation};
    use std::path::PathBuf;

    fn context() -> Context {
        Context {
            root: PathBuf::from("/ws"),
            members: Vec::new(),
        }
    }

    fn text(out: Printer<Vec<u8>>) -> String {
        String::from_utf8(out.into_inner()).expect("output is utf-8")
    }

    fn ok_output(cmd: &Cmd) -> Captured {
        Captured {
            success: true,
            output: cmd.to_string(),
        }
    }

    fn no_findings(_output: &str, _ctx: &Context, _cmd: &Cmd) -> Vec<Finding> {
        Vec::new()
    }

    fn reproduce(cmd: &Cmd, _output: Option<&str>) -> String {
        format!("run `{cmd}` to reproduce")
    }

    fn fake_tool(name: &'static str, program: &'static str, args: &[&str]) -> Step {
        Step::Tool(ToolStep {
            name,
            cmd: Cmd::new(Program::Named(program), args),
            parse: no_findings,
            fix: reproduce,
            render: None,
        })
    }

    fn one_violation(_ctx: &Context) -> Vec<Violation> {
        vec![Violation {
            path: "docs/a.md".into(),
            line: 2,
            message: "bad link".into(),
            fix: "fix the link".into(),
        }]
    }

    static FAILING_CHECK: StaticCheck = StaticCheck {
        name: "fake-static",
        run: one_violation,
    };

    /// A passing cargo run: noisy output, streamed to the sink in two chunks.
    #[expect(
        clippy::unnecessary_wraps,
        reason = "must match the Exec type, which returns io::Result"
    )]
    fn noisy_pass(_cmd: &Cmd, sink: &mut dyn FnMut(&[u8])) -> io::Result<Captured> {
        sink(b"   Compiling foo v0.1.0\n");
        sink(b"running 3 tests\n");
        Ok(Captured {
            success: true,
            output: "   Compiling foo v0.1.0\nrunning 3 tests\n".into(),
        })
    }

    /// A failing run whose only output is `boom`, with no file and line.
    #[expect(
        clippy::unnecessary_wraps,
        reason = "must match the Exec type, which returns io::Result"
    )]
    fn boom_fail(_cmd: &Cmd, sink: &mut dyn FnMut(&[u8])) -> io::Result<Captured> {
        sink(b"boom\n");
        Ok(Captured {
            success: false,
            output: "boom\n".into(),
        })
    }

    #[test]
    fn a_failing_step_does_not_stop_the_later_ones() {
        let steps = vec![
            fake_tool("first", "fake", &["fail"]),
            Step::Static(&FAILING_CHECK),
            fake_tool("last", "fake", &["pass"]),
        ];
        let mut calls = Vec::new();
        let mut exec = |cmd: &Cmd, _sink: &mut dyn FnMut(&[u8])| {
            calls.push(cmd.to_string());
            Ok(Captured {
                success: !cmd.args.iter().any(|arg| arg == "fail"),
                output: "boom\n".into(),
            })
        };
        let mut out = Printer::new(Vec::new(), false);
        let results = run_all(steps, &context(), &mut out, &mut exec);
        assert_eq!(results.len(), 3);
        assert!(matches!(
            results[0].status,
            StepStatus::Ran { failed: true, .. }
        ));
        assert!(matches!(
            results[1].status,
            StepStatus::Ran { failed: true, .. }
        ));
        assert!(matches!(
            results[2].status,
            StepStatus::Ran { failed: false, .. }
        ));
        assert_eq!(calls, vec!["fake fail".to_owned(), "fake pass".to_owned()]);
    }

    #[test]
    fn a_program_that_cannot_start_fails_its_step_and_the_next_step_runs() {
        let steps = vec![
            fake_tool("missing", "missing", &["x"]),
            fake_tool("present", "fake", &["x"]),
        ];
        let mut exec = |cmd: &Cmd, _sink: &mut dyn FnMut(&[u8])| {
            if cmd.program.name() == "missing" {
                Err(io::Error::new(io::ErrorKind::NotFound, "no such file"))
            } else {
                Ok(ok_output(cmd))
            }
        };
        let mut out = Printer::new(Vec::new(), false);
        let results = run_all(steps, &context(), &mut out, &mut exec);
        assert_eq!(results.len(), 2);
        assert!(matches!(
            results[0].status,
            StepStatus::Ran { failed: true, .. }
        ));
        assert!(matches!(
            results[1].status,
            StepStatus::Ran { failed: false, .. }
        ));
        assert!(text(out).contains("could not run `missing`"));
    }

    #[test]
    fn a_clean_run_reports_no_failure() {
        let steps = vec![fake_tool("quiet", "fake", &["x"])];
        let mut exec = |cmd: &Cmd, _sink: &mut dyn FnMut(&[u8])| Ok(ok_output(cmd));
        let mut out = Printer::new(Vec::new(), false);
        let results = run_all(steps, &context(), &mut out, &mut exec);
        assert!(matches!(
            results[0].status,
            StepStatus::Ran {
                failed: false,
                warnings: 0,
                ..
            }
        ));
    }

    #[test]
    fn not_live_a_passing_tool_prints_only_its_header() {
        let steps = vec![fake_tool("noisy", "fake", &["pass"])];
        let mut out = Printer::new(Vec::new(), false);
        run_all(steps, &context(), &mut out, &mut noisy_pass);
        assert_eq!(text(out), "==> noisy: fake pass\n");
    }

    #[test]
    fn not_live_a_failure_with_no_location_prints_output_then_message_and_fix() {
        let steps = vec![fake_tool("broken", "fake", &["fail"])];
        let mut out = Printer::new(Vec::new(), false);
        run_all(steps, &context(), &mut out, &mut boom_fail);
        assert_eq!(
            text(out),
            "==> broken: fake fail\n\
             boom\n\
             `fake fail` failed without a file:line; its output is above\n\
             \x20   fix: run `fake fail` to reproduce\n"
        );
    }

    #[test]
    fn live_a_passing_tool_streams_its_output_after_the_header() {
        let steps = vec![fake_tool("noisy", "fake", &["pass"])];
        let mut out = Printer::new(Vec::new(), true);
        run_all(steps, &context(), &mut out, &mut noisy_pass);
        assert_eq!(
            text(out),
            "==> noisy: fake pass\n   Compiling foo v0.1.0\nrunning 3 tests\n"
        );
    }

    #[test]
    fn live_a_failure_shows_its_output_once() {
        let steps = vec![fake_tool("broken", "fake", &["fail"])];
        let mut out = Printer::new(Vec::new(), true);
        run_all(steps, &context(), &mut out, &mut boom_fail);
        let printed = text(out);
        assert_eq!(printed.matches("boom").count(), 1);
        assert_eq!(
            printed,
            "==> broken: fake fail\nboom\n\
             `fake fail` failed without a file:line; its output is above\n\
             \x20   fix: run `fake fail` to reproduce\n"
        );
    }

    fn shout(output: &str) -> String {
        output.to_uppercase()
    }

    fn rendered_tool(name: &'static str, args: &[&str]) -> Step {
        Step::Tool(ToolStep {
            name,
            cmd: Cmd::new(Program::Named("fake"), args),
            parse: no_findings,
            fix: reproduce,
            render: Some(shout),
        })
    }

    #[test]
    fn live_a_rendered_tool_never_streams_and_a_failure_prints_the_rendered_output() {
        let mut out = Printer::new(Vec::new(), true);
        run_all(
            vec![rendered_tool("json", &["pass"])],
            &context(),
            &mut out,
            &mut noisy_pass,
        );
        assert_eq!(text(out), "==> json: fake pass\n");

        let mut out = Printer::new(Vec::new(), true);
        run_all(
            vec![rendered_tool("json", &["fail"])],
            &context(),
            &mut out,
            &mut boom_fail,
        );
        assert_eq!(
            text(out),
            "==> json: fake fail\n\
             BOOM\n\
             `fake fail` failed without a file:line; its output is above\n\
             \x20   fix: run `fake fail` to reproduce\n"
        );
    }
}
