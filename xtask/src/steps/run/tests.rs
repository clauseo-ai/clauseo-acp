//! Tests of `run.rs`: the step runner prints each step, its findings and its summary line, and
//! keeps running after a step fails.

use super::*;
use crate::checks::{Inspection, Noun};
use crate::process::{Program, TEST_TIMEOUT};
use crate::violation::{Finding, Violation};
use std::path::PathBuf;

fn context() -> Context {
    Context {
        root: PathBuf::from("/ws"),
        members: Ok(Vec::new()),
        load_resolved_graph: false,
    }
}

fn text(out: Printer<Vec<u8>>) -> String {
    String::from_utf8(out.into_inner()).expect("output is utf-8")
}

fn ok_output(cmd: &Cmd) -> Ran {
    Ran::exited(true, cmd.to_string(), "")
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
        cmd: Cmd::new(Program::Named(program), args, TEST_TIMEOUT),
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
            fix: crate::fix::Fix::Fake("fix the link"),
        }],
    )
}

fn one_clean_doc_with_a_note(_ctx: &Context) -> Inspection {
    Inspection {
        inspected: 1,
        notes: vec!["docs/a.md has 550 of 600 lines; split it now".into()],
        ..Inspection::default()
    }
}

fn nothing(_ctx: &Context) -> Inspection {
    Inspection::default()
}

const DOCS: Noun = Noun {
    one: "doc",
    many: "docs",
};

static FAILING_CHECK: StaticCheck = StaticCheck::fake(
    "fake-static",
    DOCS,
    || "in docs/".to_owned(),
    "xtask/src/checks/fake_static.rs",
    one_violation,
);

static NOTING_CHECK: StaticCheck = StaticCheck::fake(
    "fake-static",
    DOCS,
    || "in docs/".to_owned(),
    "xtask/src/checks/fake_static.rs",
    one_clean_doc_with_a_note,
);

static EMPTY_CHECK: StaticCheck = StaticCheck::fake(
    "fake-static",
    DOCS,
    || "in docs/".to_owned(),
    "xtask/src/checks/fake_static.rs",
    nothing,
);

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
         \x20   fix: fake-static looks for docs in docs/; restore them, or, if they moved, update the consts xtask/src/checks/fake_static.rs builds that place from\n\
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
fn noisy_pass(_cmd: &Cmd, sink: &mut dyn FnMut(&[u8])) -> io::Result<Ran> {
    sink(b"   Compiling foo v0.1.0\n");
    sink(b"running 3 tests\n");
    Ok(Ran::exited(
        true,
        "   Compiling foo v0.1.0\nrunning 3 tests\n",
        "",
    ))
}

/// A failing run whose only output is `boom`, with no file and line.
#[expect(
    clippy::unnecessary_wraps,
    reason = "must match the Exec type, which returns io::Result"
)]
fn boom_fail(_cmd: &Cmd, sink: &mut dyn FnMut(&[u8])) -> io::Result<Ran> {
    sink(b"boom\n");
    Ok(Ran::exited(false, "boom\n", ""))
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
        Ok(Ran::exited(
            !cmd.args.iter().any(|arg| arg == "fail"),
            "boom\n",
            "",
        ))
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
fn a_tool_past_its_limit_fails_its_step_and_says_it_was_stopped() {
    let steps = vec![fake_tool("slow", "fake", &["hang"])];
    let mut exec = |_: &Cmd, _sink: &mut dyn FnMut(&[u8])| Ok(Ran::timed_out());
    let mut out = Printer::new(Vec::new(), false);
    let results = run_all(steps, &context(), &mut out, &mut exec);
    assert!(matches!(
        results[0].status,
        StepStatus::Ran { failed: true, .. }
    ));
    assert_eq!(
        text(out),
        "==> slow: fake hang\n\
         `fake hang` did not finish within 1 min, so it was stopped\n\
         \x20   fix: run `fake hang` by hand to see why it does not finish within 1 min\n"
    );
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
        cmd: Cmd::new(Program::Named("fake"), args, TEST_TIMEOUT),
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

/// After a failed `cargo metadata`, a cargo step is not run and prints one failure that points
/// at the note, and counts as failed. A step that is not cargo still runs.
#[test]
fn after_a_failed_metadata_load_a_cargo_step_is_not_run() {
    let ctx = Context {
        members: Err("`cargo metadata` failed:\nerror: bad manifest".to_owned()),
        ..context()
    };
    let steps = vec![
        Step::Tool(ToolStep {
            name: "shear",
            cmd: Cmd::new(Program::Cargo, ["shear"], TEST_TIMEOUT),
            parse: no_findings,
            fix: reproduce,
            render: None,
        }),
        fake_tool("other", "fake", &["pass"]),
    ];
    let mut calls = Vec::new();
    let mut exec = |cmd: &Cmd, _sink: &mut dyn FnMut(&[u8])| {
        calls.push(cmd.to_string());
        Ok(ok_output(cmd))
    };
    let mut out = Printer::new(Vec::new(), false);
    let results = run_all(steps, &ctx, &mut out, &mut exec);
    assert_eq!(calls, ["fake pass"]);
    assert_eq!(
        results[0].status,
        StepStatus::Ran {
            failed: true,
            warnings: 0,
            secs: 0.0,
            inspected: None
        }
    );
    assert_eq!(
        text(out),
        format!(
            "==> shear: cargo shear\n\
             `cargo shear` was not run, since `cargo metadata` failed\n\
             \x20   fix: {METADATA_STEP_FIX}\n\
             ==> other: fake pass\n"
        )
    );
}
