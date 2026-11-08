//! The paths the log line names: only tasks that keep the run at 56, at most five, each with
//! its SIDs redacted, its control and formatting characters replaced, and cut at 260.

use super::*;
use crate::windows::setup::sweep::log_path::{LOGGED_CAP, PATH_CAP, clean, listed};

#[test]
fn a_sid_in_a_name_is_redacted() {
    assert_eq!(
        clean(r"\OneDrive Reporting Task-S-1-5-21-1-2-3-1001"),
        r"\OneDrive Reporting Task-<sid>"
    );
    // Either case, and only SID-shaped runs: S-1-5 alone, or S-1- with no digits, stay.
    assert_eq!(clean(r"\A\s-1-5-18x"), r"\A\<sid>x");
    assert_eq!(clean(r"\A\S-1-5 and S-1-x"), r"\A\S-1-5 and S-1-x");
}

#[test]
fn two_sids_are_each_redacted() {
    assert_eq!(
        clean(r"\S-1-5-21-9-9-9-500\Task S-1-12-1-2-3-4-5 done"),
        r"\<sid>\Task <sid> done"
    );
}

#[test]
fn control_bidi_and_invisible_characters_are_replaced() {
    let path = "\\A\\a\u{0}b\u{7}c\u{7F}d\u{85}e\u{200B}f\u{202E}g\u{2066}h\u{61C}i\u{FEFF}j";
    assert_eq!(clean(path), r"\A\a?b?c?d?e?f?g?h?i?j");
}

#[test]
fn a_long_path_is_cut_at_260_characters() {
    let path = format!("\\{}", "é".repeat(299));
    let cleaned = clean(&path);
    assert_eq!(cleaned.chars().count(), PATH_CAP + 1);
    assert!(cleaned.ends_with("é…"), "{cleaned}");
    assert_eq!(clean(&"a".repeat(PATH_CAP)), "a".repeat(PATH_CAP));
}

#[test]
fn at_most_five_paths_are_named_then_how_many_more() {
    let paths: Vec<String> = (0..8).map(|n| format!(r"\A\t{n}")).collect();
    let line = listed(&paths);
    assert_eq!(
        line,
        r"; tasks that keep the sweep at 56: \A\t0, \A\t1, \A\t2, \A\t3, \A\t4 (+3 more)"
    );
    assert_eq!(LOGGED_CAP, 5);
    let twice = [r"\A\t".to_owned(), r"\A\t".to_owned()];
    assert_eq!(listed(&twice), r"; tasks that keep the sweep at 56: \A\t");
    assert_eq!(listed(&[]), "");
}

#[test]
fn only_a_task_that_keeps_the_run_at_56_is_logged() {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\A", "fine", user("S-1-5-18"));
    fake.task("\\A", "gone", user("S-1-5-21-1-2-3-1010"));
    fake.task("\\B", "stuck", user("S-1-5-21-1-2-3-1010"));
    fake.delete_refuse = vec!["stuck".to_owned()];
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    let line = &fake.logged[0].1;
    assert!(
        line.ends_with(r"; tasks that keep the sweep at 56: \B\stuck"),
        "{line}"
    );
    assert!(!line.contains("fine") && !line.contains("gone"), "{line}");
    // A run that ends 0 names none.
    let mut clean_run = Fake::new(&[1010]);
    clean_run.task("\\A", "fine", user("S-1-5-18"));
    assert_eq!(sweep(&mut clean_run), SweepExit::Done);
    assert!(!clean_run.logged[0].1.contains("keep the sweep"));
}
