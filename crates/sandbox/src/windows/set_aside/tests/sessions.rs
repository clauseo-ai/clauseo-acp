//! N6: two sessions of one matter at once. The record keeps the set of open sessions, so the
//! first session's end leaves the record open and the second's end still checks.

use std::collections::BTreeSet;
use std::time::Duration;

use super::{exit, exit_as, launch, launch_as, none, record, volume, world};
use crate::windows::set_aside::Reason;

fn sessions(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// N6: session 1 ends while session 2 runs; what session 2's agent makes after that is still
/// set aside when session 2 ends.
#[test]
fn a_second_session_in_the_same_matter_is_still_checked() {
    let (w, root, data) = world("aside-two-sessions");
    let v = volume();
    launch_as(&v, &root, &data, "s1", &[], launch()).unwrap();
    let second = launch() + Duration::from_secs(60);
    let checked = launch_as(&v, &root, &data, "s2", &[], second).unwrap();
    assert_eq!(checked, none());
    assert_eq!(record(&data).sessions, sessions(&["s1", "s2"]));

    let first_end = exit() - Duration::from_secs(60);
    let checked = exit_as(&v, &root, &data, "s1", &[], first_end).unwrap();
    assert_eq!(checked, none());
    assert_eq!(record(&data).sessions, sessions(&["s2"]), "still open");

    w.dir
        .file("matter/.claude/settings.json", "{\"hooks\": {}}");
    let moved = exit_as(&v, &root, &data, "s2", &[], exit()).unwrap();

    let seen: Vec<_> = moved.iter().map(|e| (e.name.as_str(), e.reason)).collect();
    assert_eq!(seen, [(".claude", Reason::New)]);
    assert!(!root.join(".claude").exists());
    assert_eq!(record(&data).sessions, sessions(&[]), "clean only now");
}

/// N6: a session that crashed stays open in the record, so every later launch keeps checking
/// (fail-safe): here a name the lawyer adds between sessions is set aside too.
#[test]
fn a_crashed_session_keeps_every_later_launch_checking() {
    let (w, root, data) = world("aside-crashed-session");
    let v = volume();
    launch_as(&v, &root, &data, "crashed", &[], launch()).unwrap();
    let later = exit() + Duration::from_secs(60);
    launch_as(&v, &root, &data, "s2", &[], exit()).unwrap();
    exit_as(&v, &root, &data, "s2", &[], later).unwrap();
    assert_eq!(record(&data).sessions, sessions(&["crashed"]));

    w.dir
        .file("matter/CLAUDE.md", "# the lawyer's, between sessions\n");
    let third = later + Duration::from_secs(60);
    let moved = launch_as(&v, &root, &data, "s3", &[], third).unwrap();

    let names: Vec<_> = moved.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["CLAUDE.md"]);
    assert_eq!(record(&data).sessions, sessions(&["crashed", "s3"]));
}
