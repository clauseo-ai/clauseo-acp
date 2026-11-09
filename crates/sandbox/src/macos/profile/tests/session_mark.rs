//! The session mark's rule, and the writable paths the launcher checks its marks folder against.

use std::path::PathBuf;

use super::{TAG, World};
use crate::macos::profile::{build, build_marked};

/// L7: the paths the launcher keeps its marks folder out of are read from the allow rules with a
/// `file-write` operation, never listed apart: the matter, the state, notes and temp folders;
/// not a read-only path, and not the session mark.
#[test]
fn the_writable_paths_are_read_from_the_allow_rules() {
    let w = World::new("profile-writable");
    let policy = w
        .builder()
        .notes(w.at("data/agent-state/m1/notes"))
        .build()
        .unwrap();
    let mark = dunce::canonicalize(w.dir.dir("data/marks/clauseo-4242-7"))
        .unwrap()
        .join("mark");
    let profile = build_marked(&policy, TAG, Some(&mark));
    let writable = profile.writable();
    let mut want = vec![
        policy.matter().to_owned(),
        policy.state().to_owned(),
        policy.notes().unwrap().to_owned(),
    ];
    if profile.params().iter().any(|(name, _)| name == "TMPDIR") {
        want.push(policy.tmpdir());
    }
    for path in &want {
        assert!(
            writable.contains(path),
            "{} missing: {writable:?}",
            path.display()
        );
    }
    assert!(
        !writable.contains(&mark),
        "the mark is not a writable folder"
    );
    // Each is a parameter's path, and only those the allow-write rules name.
    for path in writable {
        assert!(
            profile.params().iter().any(|(_, p)| p == path),
            "{}",
            path.display()
        );
        assert!(
            want.contains(path),
            "{} is not written by the agent",
            path.display()
        );
    }
    // The same paths whether or not the mark's rule is in.
    assert_eq!(build(&policy, TAG).writable(), writable);
}

/// The session mark (`macos/sweep.rs`): one rule, `file-write-data` on the mark alone, after
/// the hidden data dir's deny that covers it, and no rule naming its canary or its folder.
#[test]
fn with_a_session_mark() {
    let w = World::new("profile-session-mark");
    let policy = w.builder().build().unwrap();
    let marks = dunce::canonicalize(w.dir.dir("data/marks/clauseo-4242-7")).unwrap();
    let mark = marks.join("mark");
    let profile = build_marked(&policy, TAG, Some(&mark));
    let plain = build(&policy, TAG);
    let added: Vec<&str> = profile
        .text()
        .lines()
        .filter(|line| !plain.text().lines().any(|p| p == *line))
        .collect();
    assert_eq!(
        added,
        [
            "; The session mark: data writes to this one file, so the host can find every",
            "; process of this launch, whatever its parent, session or group (0038).",
            "(allow file-write-data",
            "  (literal (param \"SESSION_MARK\")))",
        ]
    );
    let text = profile.text();
    let hidden = text.find("(subpath (param \"HIDDEN_0\"))").unwrap();
    let rule = text.find("(literal (param \"SESSION_MARK\"))").unwrap();
    assert!(
        hidden < rule,
        "the mark's rule must come after the data dir's deny"
    );
    assert_eq!(text.matches("SESSION_MARK").count(), 1);
    let named: Vec<&(String, PathBuf)> = profile
        .params()
        .iter()
        .filter(|(name, _)| name == "SESSION_MARK")
        .collect();
    assert_eq!(named, [&("SESSION_MARK".to_owned(), mark)]);
    assert!(
        !profile
            .params()
            .iter()
            .any(|(_, path)| path.ends_with("canary"))
    );
}
