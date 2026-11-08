//! N6: two sessions of one matter at once. The record keeps the set of open sessions, so the
//! first session's end leaves the record open and the second's end still checks. R3: a session
//! that crashed is checked at the next launch, then dropped.

use std::collections::BTreeSet;
use std::time::Duration;

use super::{exit, exit_as, launch, launch_as, none, record, volume, world};
use crate::windows::set_aside::Reason;

fn sessions(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// N6: session 1 ends while session 2 runs; what session 2's agent makes after that is still
/// set aside when session 2 ends. R3: session 2's launch is told session 1 still runs, so it
/// stays open.
#[test]
fn a_second_session_in_the_same_matter_is_still_checked() {
    let (w, root, data) = world("aside-two-sessions");
    let v = volume();
    launch_as(&v, &root, &data, "s1", &[], launch()).unwrap();
    let second = launch() + Duration::from_secs(60);
    let checked = launch_as(&v, &root, &data, "s2", &["s1"], second).unwrap();
    assert_eq!(checked, none());
    assert_eq!(record(&data).sessions, sessions(&["s1", "s2"]));

    let first_end = exit() - Duration::from_secs(60);
    let checked = exit_as(&v, &root, &data, "s1", first_end).unwrap();
    assert_eq!(checked, none());
    assert_eq!(record(&data).sessions, sessions(&["s2"]), "still open");

    w.dir
        .file("matter/.claude/settings.json", "{\"hooks\": {}}");
    let moved = exit_as(&v, &root, &data, "s2", exit()).unwrap();

    let seen: Vec<_> = moved
        .iter()
        .map(|e| (e.name.to_str().unwrap(), e.reason))
        .collect();
    assert_eq!(seen, [(".claude", Reason::New)]);
    assert!(!root.join(".claude").exists());
    assert_eq!(record(&data).sessions, sessions(&[]), "clean only now");
}

/// R3: a session that crashed (no after-session pass, and not among the running sessions the
/// next launch is given) is checked at that launch, then dropped; so a change the lawyer makes
/// between later sessions is never set aside.
#[test]
fn a_crashed_session_is_checked_at_the_next_launch_then_dropped() {
    let (w, root, data) = world("aside-crashed-session");
    let v = volume();
    launch_as(&v, &root, &data, "crashed", &[], launch()).unwrap();
    w.dir.file("matter/.mcp.json", "{\"the\": \"agent's\"}");

    let moved = launch_as(&v, &root, &data, "s2", &[], exit()).unwrap();
    let names: Vec<_> = moved.iter().map(|e| e.name.to_str().unwrap()).collect();
    assert_eq!(names, [".mcp.json"], "the crashed session's change");
    assert_eq!(
        record(&data).sessions,
        sessions(&["s2"]),
        "the crashed id is dropped"
    );
    let later = exit() + Duration::from_secs(60);
    assert_eq!(exit_as(&v, &root, &data, "s2", later).unwrap(), none());
    assert_eq!(record(&data).sessions, sessions(&[]));

    w.dir
        .file("matter/CLAUDE.md", "# the lawyer's, between sessions\n");
    let third = later + Duration::from_secs(60);
    let moved = launch_as(&v, &root, &data, "s3", &[], third).unwrap();

    assert_eq!(moved, none());
    assert!(root.join("CLAUDE.md").is_file());
    assert_eq!(record(&data).sessions, sessions(&["s3"]));
}
