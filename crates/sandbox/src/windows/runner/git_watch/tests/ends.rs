//! What else ends the session since PR #789's review: items gone on different paths count
//! together, a watch that breaks ends the session (rule 4: nothing stands in for it), and an end
//! that can't confirm the agent's job is empty moves nothing.

use std::io;
use std::path::PathBuf;

use super::super::Next;
use super::{Log, added, guard, script, session};
use crate::windows::runner::git_watch::watch;

/// Review item 1: new names each time (`a1/.git`, `a2/.git`, ...), each gone by its check,
/// count together: the agent can't keep short-lived repositories coming and going.
#[test]
fn items_gone_on_different_paths_count_together_toward_the_limit() {
    let log = Log::default();
    let mut chased = guard(&log);
    chased.gone = ["a1/.git", "a2/.git", "a3/.git"]
        .map(PathBuf::from)
        .to_vec();
    let reports = vec![
        added(&["a1/.git"]),
        added(&["a2/.git"]),
        added(&["a3/.git"]),
    ];
    watch(script(reports), &chased, &session(&log, 1));
    assert_eq!(
        log.lines(),
        ["changed", "changed", "changed", "said", "end", "rescan"]
    );
}

/// Rule 4: a watch that breaks while the agent runs ends the session, with its own notice, and
/// nothing stands in for it: no rescans every few seconds, no arming again. It waits for its
/// stop, then makes the last rescan once the job is empty.
#[test]
fn a_broken_watch_ends_the_session_and_waits_for_its_stop() {
    let log = Log::default();
    let first = script(vec![
        Next::Broken(io::Error::from_raw_os_error(6)),
        added(&["never/.git"]),
    ]);
    watch(first, &guard(&log), &session(&log, 3));
    assert_eq!(
        log.lines(),
        ["said", "said", "end", "pause", "pause", "pause", "rescan"]
    );
}

/// The end can't confirm the agent's job is empty: nothing is moved while it may run, and the
/// lawyer's log says once that the check after the session sets the items aside.
#[test]
fn with_the_end_unconfirmed_nothing_moves_and_the_items_left_are_said_once() {
    let log = Log::default();
    let mut unsure = session(&log, 1);
    unsure.empty = false;
    let first = script(vec![added(&["a/.git", "b/.git"]), added(&["c/.git"])]);
    watch(first, &guard(&log), &unsure);
    let lines = log.lines();
    assert!(
        !lines.iter().any(|line| line.starts_with("moved")),
        "{lines:?}"
    );
    assert_eq!(lines.iter().filter(|line| *line == "end").count(), 1);
    // The notice of the stop, then the one line for what was left; the last rescan, with the
    // job empty, moves again.
    assert_eq!(lines[..5], ["changed", "said", "end", "said", "changed"]);
    assert_eq!(lines.iter().filter(|line| *line == "said").count(), 2);
}
