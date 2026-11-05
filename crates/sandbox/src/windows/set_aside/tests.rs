use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use super::*;
use crate::windows::test_world::{STATE, World};

#[test]
fn only_names_the_pool_user_owns_are_set_aside() {
    let w = World::new("set-aside");
    w.dir.file("matter/.mcp.json", "{}");
    w.dir.file("matter/CLAUDE.md", "# the lawyer's\n");
    w.dir.file("matter/.claude/settings.json", "{}");
    w.dir.file("matter/notes.txt", "kept");
    let policy = w.builder().build().unwrap();
    let agent_made = |path: &Path| {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        Ok(name == ".mcp.json" || name == ".claude" || name == "notes.txt")
    };
    let now = UNIX_EPOCH + Duration::from_millis(1_762_337_700_123);
    let moved = set_aside_with(&policy, agent_made, now).unwrap();

    let folder = w.canonical(STATE).join("set-aside/20251105T101500-123Z");
    let names: Vec<_> = moved.iter().map(|m| m.to.clone()).collect();
    assert_eq!(
        names,
        vec![folder.join(".claude"), folder.join(".mcp.json")]
    );
    assert!(folder.join(".claude/settings.json").is_file());
    assert!(!w.at("matter/.mcp.json").exists());
    assert!(w.at("matter/CLAUDE.md").exists(), "the lawyer's name stays");
    assert!(w.at("matter/notes.txt").exists(), "not a protected name");
}

#[test]
fn a_set_aside_that_would_overwrite_refuses() {
    let w = World::new("set-aside-clash");
    w.dir.file("matter/AGENTS.md", "x");
    let policy = w.builder().build().unwrap();
    let now = UNIX_EPOCH;
    w.dir.file(
        &format!("{STATE}/set-aside/19700101T000000-000Z/AGENTS.md"),
        "y",
    );
    let error = set_aside_with(&policy, |_| Ok(true), now).unwrap_err();
    assert!(matches!(
        error,
        Error::AclApply {
            failure: ApplyFailure::Io(io::ErrorKind::AlreadyExists),
            ..
        }
    ));
    assert!(w.at("matter/AGENTS.md").exists());
}

#[test]
fn timestamps_are_utc() {
    assert_eq!(timestamp(UNIX_EPOCH), "19700101T000000-000Z");
    let leap_day_2000: u64 = 951_782_400;
    let leap = UNIX_EPOCH + Duration::from_secs(leap_day_2000);
    assert_eq!(timestamp(leap), "20000229T000000-000Z");
}
