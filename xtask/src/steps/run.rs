//! Runs the steps in order. Every header, finding and summary line goes through the `Printer`, and
//! so does tool output: it streams when stdout is a terminal and is held back otherwise.

use crate::checks::StaticCheck;
use crate::clock::timed;
use crate::output::Printer;
use crate::process::{Captured, Cmd};
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

/// Runs one static check through `StaticCheck::report`: prints its violations, its failure for
/// inspecting nothing, its notes, then a result line with the count, e.g.
/// `capabilities ok (1 capability file inspected, 0.0 s)`.
fn run_static<W: Write>(check: &StaticCheck, ctx: &Context, out: &mut Printer<W>) -> StepResult {
    out.header(check.name, "built-in static check");
    let (report, secs) = timed(|| check.report(ctx));
    for found in &report.inspection.violations {
        out.violation(found);
    }
    if let Some(nothing) = &report.nothing_inspected {
        out.failure(&nothing.message, &nothing.fix);
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
            inspected: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::{Inspection, Noun};
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

    fn one_violation(_ctx: &Context) -> Inspection {
        Inspection::new(
            1,
            vec![Violation {
                path: "docs/a.md".into(),
                line: 2,
                message: "bad link".into(),
                fix: "fix the link".into(),
            }],
        )
    }

    fn one_clean_doc_with_a_note(_ctx: &Context) -> Inspection {
        Inspection {
            inspected: 1,
            violations: Vec::new(),
            notes: vec!["docs/a.md has 550 of 600 lines; split it now".into()],
        }
    }

    fn nothing(_ctx: &Context) -> Inspection {
        Inspection::default()
    }

    const DOCS: Noun = Noun {
        one: "doc",
        many: "docs",
    };

    static FAILING_CHECK: StaticCheck = StaticCheck {
        name: "fake-static",
        counts: DOCS,
        place: "in docs/",
        source: "xtask/src/checks/fake_static.rs",
        run: one_violation,
    };

    static NOTING_CHECK: StaticCheck = StaticCheck {
        name: "fake-static",
        counts: DOCS,
        place: "in docs/",
        source: "xtask/src/checks/fake_static.rs",
        run: one_clean_doc_with_a_note,
    };

    static EMPTY_CHECK: StaticCheck = StaticCheck {
        name: "fake-static",
        counts: DOCS,
        place: "in docs/",
        source: "xtask/src/checks/fake_static.rs",
        run: nothing,
    };

    #[test]
    fn a_static_check_prints_its_findings_notes_and_count() {
        let mut out = Printer::new(Vec::new(), false);
        let results = run_all(
            vec![Step::Static(&FAILING_CHECK), Step::Static(&NOTING_CHECK)],
            &context(),
            &mut out,
            &mut noisy_pass,
        );
        assert!(matches!(
            results[1].status,
            StepStatus::Ran { failed: false, .. }
        ));
        assert_eq!(
            without_secs(&text(out)),
            "==> fake-static: built-in static check\n\
             docs/a.md:2: bad link\n\
             \x20   fix: fix the link\n\
             fake-static FAILED (1 doc inspected, S)\n\
             ==> fake-static: built-in static check\n\
             note: docs/a.md has 550 of 600 lines; split it now\n\
             fake-static ok (1 doc inspected, S)\n"
        );
    }

    #[test]
    fn a_static_check_that_inspects_nothing_fails_without_a_location() {
        let mut out = Printer::new(Vec::new(), false);
        let results = run_all(
            vec![Step::Static(&EMPTY_CHECK)],
            &context(),
            &mut out,
            &mut noisy_pass,
        );
        assert!(matches!(
            results[0].status,
            StepStatus::Ran { failed: true, .. }
        ));
        assert_eq!(
            without_secs(&text(out)),
            "==> fake-static: built-in static check\n\
             0 docs inspected in docs/\n\
             \x20   fix: fake-static looks for docs in docs/; restore them, or update xtask/src/checks/fake_static.rs if they moved\n\
             fake-static FAILED (0 docs inspected, S)\n"
        );
    }

    /// `printed` with each `<n>.<n> s)` timing replaced by `S)`, since the clock is real.
    fn without_secs(printed: &str) -> String {
        printed
            .lines()
            .map(|line| match line.rsplit_once(", ") {
                Some((head, tail)) if tail.ends_with(" s)") => format!("{head}, S)"),
                _ => line.to_owned(),
            })
            .map(|line| line + "\n")
            .collect()
    }

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
