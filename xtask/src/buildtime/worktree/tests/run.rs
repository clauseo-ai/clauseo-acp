//! Tests of a whole `cargo xtask buildtime worktree` run as `run` prints it, and of the edges of
//! a run: a worktree that cannot be removed, a folder git made but does not know, a command past
//! its limit, the touch time.

use super::super::super::Out;
use super::super::*;
use super::{Fake, Trick, app_only, find, git, left_behind, measure_with, repo, samples};
use crate::output::Printer;
use crate::process::Cmd;
use crate::scratch::Scratch;
use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;
use std::time::SystemTime;

/// A printer's output, kept for the test to read.
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

impl Shared {
    fn text(&self) -> String {
        String::from_utf8(self.0.borrow().clone()).expect("utf-8")
    }
}

/// The one row the worktree runs read, as `budgets.toml` writes it.
const BUDGETS: &str = "[[budget]]\nid = \"fresh_agent_worktree\"\nmetric = \"m\"\nlimit = 30.0\n\
                       unit = \"s\"\ngate = \"alert\"\nmeasured_by = \"cargo xtask buildtime \
                       worktree\"\nstatus = \"unmeasured\"\nsource = \"s\"\n";

/// Runs `run` on `root` with `fake`; returns its result, stdout and diagnostics.
fn run_with(
    root: &Path,
    json: bool,
    fake: &mut Fake,
) -> (Result<bool, Vec<String>>, String, String) {
    let (stdout, diagnostics) = (Shared::default(), Shared::default());
    let mut out: Out = Printer::new(Box::new(stdout.clone()), false);
    let mut diag: Out = Printer::new(Box::new(diagnostics.clone()), false);
    let ran = run(
        root,
        2,
        json,
        &mut out,
        &mut diag,
        &mut |cmd: &Cmd, dir: &Path| fake.run(cmd, dir),
    );
    (ran, stdout.text(), diagnostics.text())
}

#[test]
fn a_whole_run_prints_the_table_or_one_json_object_and_passes() {
    let scratch = Scratch::new("worktree-whole-run");
    let root = repo(&scratch);
    Scratch::write(root.join("budgets.toml"), BUDGETS).expect("the budget row");
    let mut fake = Fake {
        builds: vec![app_only()],
        ..Fake::default()
    };
    let (passed, table, diag) = run_with(&root, false, &mut fake);
    assert_eq!(passed, Ok(true), "{diag}");
    assert_eq!(
        diag,
        "run 1, first build: rebuilt clauseo-desktop (custom-build), clauseo-desktop (lib); \
         fresh: 1\nrun 2, first build: rebuilt clauseo-desktop (custom-build), clauseo-desktop \
         (lib); fresh: 1\n",
        "what each first build rebuilt, from cargo's JSON"
    );
    let lines: Vec<&str> = table.lines().collect();
    assert!(lines[0].starts_with("rustc "), "{table}");
    assert!(
        lines[1].starts_with("profile: dev, incremental expected "),
        "{table}"
    );
    assert!(
        lines[1].ends_with("; the first build's rustc line for clauseo-desktop decides"),
        "{table}"
    );
    let path = place(&root).expect("a place");
    assert_eq!(lines[2], format!("worktree: {}", path.display()));
    assert!(lines[3].starts_with("probe "), "{table}");
    for name in STEPS.iter().map(|step| step.name()).chain([TOTAL]) {
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with(&format!("{name} "))),
            "{name} has a row: {table}"
        );
    }
    assert!(!table.contains('{'), "no JSON in the table");

    let mut fake = Fake {
        builds: vec![app_only()],
        ..Fake::default()
    };
    let (passed, json, diag) = run_with(&root, true, &mut fake);
    assert_eq!(passed, Ok(true), "{diag}");
    assert_eq!(
        diag.lines().count(),
        2,
        "the two lines go to stderr: {diag}"
    );
    assert_eq!(json.lines().count(), 1, "one JSON object: {json}");
    let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    assert_eq!(value["runs"], 2);
    assert_eq!(value["passed"], true);
    assert_eq!(value["probes"].as_array().map(Vec::len), Some(7));
    assert!(value.get("errors").is_none());
}

#[test]
fn a_run_without_the_budget_row_stops_before_it_measures() {
    let scratch = Scratch::new("worktree-no-row");
    let root = repo(&scratch);
    let other = BUDGETS.replace("fresh_agent_worktree", "another_row");
    Scratch::write(root.join("budgets.toml"), other).expect("another row");
    let mut fake = Fake::default();
    let (stopped, out, diag) = run_with(&root, false, &mut fake);
    assert_eq!(
        stopped,
        Err(vec![
            "no budget row fresh_agent_worktree in budgets.toml".to_owned()
        ])
    );
    assert_eq!(
        diag,
        "no budget row fresh_agent_worktree in budgets.toml\n    fix: add a [[budget]] row with \
         id = \"fresh_agent_worktree\" to budgets.toml\n"
    );
    assert!(out.is_empty() && fake.seen.is_empty(), "nothing ran");
}

#[test]
fn a_worktree_that_cannot_be_removed_fails_the_run_and_no_further_run_starts() {
    let scratch = Scratch::new("worktree-stuck");
    let root = repo(&scratch);
    let mut fake = Fake {
        builds: vec![app_only()],
        tricks: vec![Trick::RefuseRemove],
        ..Fake::default()
    };
    let measured = measure_with(&root, false, 3, &mut fake).expect("measured");
    assert_eq!(
        fake.ran("git worktree add"),
        1,
        "no run after the failed removal"
    );
    let path = place(&root).expect("a place");
    let shown = path.display();
    assert_eq!(
        measured.errors,
        [Failure::new(
            format!(
                "could not remove the worktree {shown} that this run made: `git worktree remove \
                 --force {shown}` failed (fatal: refused by the test); `git worktree list` names \
                 it; its folder is on disk"
            ),
            leftover_fix(&path),
        )]
    );
    assert_eq!(
        samples(find(&measured, TOTAL)).len(),
        1,
        "the run itself was measured"
    );
    assert!(!measured.passed(false), "the failed removal fails the run");
    git(
        &root,
        &["worktree", "remove", "--force", &path.to_string_lossy()],
    );
    assert!(
        !left_behind(&root, &path),
        "the fix line's command removes it"
    );
}

#[test]
fn a_folder_git_made_but_does_not_know_fails_the_removal_and_is_left_as_it_is() {
    let scratch = Scratch::new("worktree-unknown-folder");
    let root = repo(&scratch);
    let mut fake = Fake {
        builds: vec![app_only()],
        tricks: vec![Trick::AddFailsAfterMakingIt],
        ..Fake::default()
    };
    let measured = measure_with(&root, false, 2, &mut fake).expect("measured");
    let path = place(&root).expect("a place");
    let shown = path.display();
    let [failure] = measured.errors.as_slice() else {
        panic!("one failure: {:?}", measured.errors);
    };
    assert_eq!(
        failure,
        &Failure::new(
            format!(
                "this run's `git worktree add` failed and something is left at {shown}: its \
                 folder is on disk. xtask cannot tell whether git or someone else made it, so it \
                 leaves it as it is"
            ),
            leftover_fix(&path)
        )
    );
    assert_eq!(fake.ran("git worktree remove"), 0, "xtask removes nothing");
    assert!(path.is_dir(), "left as it is");
    assert_eq!(fake.ran("git worktree add"), 1, "no second run");
    assert_eq!(
        find(&measured, "worktree_copy").outcome,
        Outcome::Skipped(NOT_REACHED.into())
    );
}

#[test]
fn a_command_past_its_limit_says_it_was_stopped() {
    let scratch = Scratch::new("worktree-timeout");
    let root = repo(&scratch);
    let mut fake = Fake {
        builds: vec![app_only()],
        tricks: vec![Trick::PnpmTimesOut],
        ..Fake::default()
    };
    let path = place(&root).expect("a place");
    let cmd = format!(
        "pnpm -C {} install --frozen-lockfile --prefer-offline",
        path.join("ui").display()
    );
    let stopped = Outcome::Failed(Failure::new(
        format!("`{cmd}` did not finish within 30 min, so it was stopped"),
        format!(
            "in a worktree made the same way at {}, run `{cmd}` by hand to see why it does not \
             finish within 30 min",
            path.display()
        ),
    ));
    let measured = measure_with(&root, false, 1, &mut fake).expect("measured");
    assert_eq!(find(&measured, "worktree_pnpm_install").outcome, stopped);
    assert!(!left_behind(&root, &path), "the worktree is gone");
    // The same fix holds when the worktree then cannot be removed.
    let mut stuck = Fake {
        builds: vec![app_only()],
        tricks: vec![Trick::PnpmTimesOut, Trick::RefuseRemove],
        ..Fake::default()
    };
    let measured = measure_with(&root, false, 1, &mut stuck).expect("measured");
    assert_eq!(find(&measured, "worktree_pnpm_install").outcome, stopped);
    assert_eq!(measured.errors.len(), 1, "the removal failed too");
    assert!(left_behind(&root, &path), "the worktree is still there");
    git(
        &root,
        &["worktree", "remove", "--force", &path.to_string_lossy()],
    );
}

#[test]
fn tracked_files_get_the_first_second_of_2000() {
    let since_epoch = touched_time()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("after the epoch");
    assert_eq!(since_epoch.as_secs(), 946_684_800, "2000-01-01T00:00:00Z");
}
