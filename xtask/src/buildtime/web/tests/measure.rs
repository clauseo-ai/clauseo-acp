//! Tests of `measure` in `web.rs` through an injected runner: a script that reports is measured,
//! and one that cannot start, runs past its limit or prints no report is named with its command
//! and the fix that says what to do.

use super::*;
use crate::process::Ran;
use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;

/// A writer that keeps what is written, so the test reads the printer's lines back.
#[derive(Clone, Default)]
struct Shared(Rc<RefCell<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A checkout with `ui/app/node_modules`, so `measure` gets as far as running the script.
fn checkout(name: &str) -> Scratch {
    let root = Scratch::new(name);
    Scratch::create_dir_all(root.join("ui/app/node_modules")).expect("create node_modules");
    root
}

/// `measure` for 2 runs with `runner`: what it returned, and what it printed.
fn measured(root: &Path, runner: &mut QuietRunner<'_>) -> (Result<WebRun, Vec<String>>, String) {
    let shared = Shared::default();
    let mut diag: Out = Printer::new(Box::new(shared.clone()), false);
    let result = measure(root, 2, false, &budgets(), &mut diag, runner);
    let printed = String::from_utf8(shared.0.borrow().clone()).expect("utf-8 output");
    (result, printed)
}

#[test]
fn a_script_past_its_limit_is_named_with_the_command_to_run_by_hand() {
    let root = checkout("web-measure-timeout");
    let mut runner = |_: &Cmd, _: &Path| -> io::Result<Ran> { Ok(Ran::timed_out()) };
    let (result, printed) = measured(&root, &mut runner);
    let message = concat!(
        "`node ui/app/scripts/buildtime.mjs --runs 2` did not finish within 1 h, ",
        "so it was stopped"
    );
    let fix = concat!(
        "run `node ui/app/scripts/buildtime.mjs --runs 2` by hand to see why it does not ",
        "finish within 1 h"
    );
    assert_eq!(
        result.expect_err("a script past its limit fails the half"),
        vec![message.to_owned()]
    );
    assert_eq!(printed, format!("{message}\n    fix: {fix}\n"));
}

#[test]
fn a_node_that_cannot_start_is_named_with_the_node_to_install() {
    let root = checkout("web-measure-no-node");
    let mut runner =
        |_: &Cmd, _: &Path| -> io::Result<Ran> { Err(io::Error::other("no such file")) };
    let (result, printed) = measured(&root, &mut runner);
    assert_eq!(
        result.expect_err("a node that cannot start fails the half"),
        vec!["could not run `node`: no such file".to_owned()]
    );
    assert_eq!(
        printed,
        "could not run `node`: no such file\n    fix: install Node 24.11 or later\n"
    );
}

#[test]
fn a_script_that_reports_is_measured() {
    let root = checkout("web-measure-reported");
    let mut runner = |_: &Cmd, _: &Path| -> io::Result<Ran> { Ok(Ran::exited(true, ALL_OK, "")) };
    let (result, printed) = measured(&root, &mut runner);
    let run = result.expect("a script that reports is measured");
    assert!(run.failures.is_empty(), "{printed}");
    assert_eq!(printed, "");
}

#[test]
fn a_script_that_exits_zero_and_prints_no_report_is_named_as_unreadable() {
    let root = checkout("web-measure-silent");
    let mut runner = |_: &Cmd, _: &Path| -> io::Result<Ran> { Ok(Ran::exited(true, "", "")) };
    let (result, printed) = measured(&root, &mut runner);
    let message = "ui/app/scripts/buildtime.mjs printed nothing on stdout";
    assert_eq!(
        result.expect_err("a script with no report fails the half"),
        vec![message.to_owned()]
    );
    assert_eq!(
        printed,
        format!(
            "{message}\n    fix: run `node ui/app/scripts/buildtime.mjs --runs 2` to see the \
             error\n"
        )
    );
}

#[test]
fn a_script_that_exits_non_zero_and_prints_no_report_shows_its_stderr_and_is_named() {
    let root = checkout("web-measure-crashed");
    let mut runner =
        |_: &Cmd, _: &Path| -> io::Result<Ran> { Ok(Ran::exited(false, "", "Error: boom\n")) };
    let (result, printed) = measured(&root, &mut runner);
    let message = "`node ui/app/scripts/buildtime.mjs --runs 2` failed";
    assert_eq!(
        result.expect_err("a script that failed fails the half"),
        vec![message.to_owned()]
    );
    assert_eq!(
        printed,
        format!(
            "Error: boom\n{message}\n    fix: run `node ui/app/scripts/buildtime.mjs --runs 2` to \
             see the error\n"
        )
    );
}
