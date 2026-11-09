//! Detection's config folder: fresh per run, removed on drop, seeded by copies that leave the
//! lawyer's files as they were.

use std::time::UNIX_EPOCH;

use clauseo_protocol::AgentId;

use super::Scratch;
use crate::write::TestDir;

fn claude() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

#[test]
fn each_run_gets_a_new_folder_that_goes_on_drop() {
    let dir = TestDir::new("scratch-fresh");
    let data = dir.0.join("data");
    crate::write::create_dir_all(&data).unwrap();
    let one = Scratch::create(&data, &claude(), UNIX_EPOCH).unwrap();
    let two = Scratch::create(&data, &claude(), UNIX_EPOCH).unwrap();
    assert_ne!(one.path(), two.path());
    assert!(one.path().is_dir() && two.path().is_dir());
    assert!(
        one.path()
            .starts_with(data.join("agents/claude-acp/detect-scratch"))
    );
    assert_eq!(std::fs::read_dir(one.path()).unwrap().count(), 0);
    let paths = (one.path().to_owned(), two.path().to_owned());
    crate::write::write_atomic(&paths.0.join(".claude.json"), b"{}").unwrap();
    crate::write::create_dir_all(&paths.0.join("backups")).unwrap();
    drop(one);
    assert!(!paths.0.exists());
    assert!(paths.1.is_dir());
    drop(two);
    assert!(!paths.1.exists());
}

#[test]
fn the_seed_copies_the_config_and_settings_and_changes_neither() {
    let dir = TestDir::new("scratch-seed");
    let data = dir.0.join("data");
    crate::write::create_dir_all(&data).unwrap();
    let lawyer = dir.0.join("home");
    let config = dir.file("home/.claude.json", br#"{"oauthAccount":{}}"#);
    let settings = dir.file("home/.claude/settings.json", br#"{"theme":"dark"}"#);
    let before = [&config, &settings].map(|path| std::fs::metadata(path).unwrap().modified().ok());
    let scratch = Scratch::create(&data, &claude(), UNIX_EPOCH).unwrap();
    scratch.seed(&lawyer);
    assert_eq!(
        std::fs::read(scratch.path().join(".claude.json")).unwrap(),
        br#"{"oauthAccount":{}}"#
    );
    assert_eq!(
        std::fs::read(scratch.path().join("settings.json")).unwrap(),
        br#"{"theme":"dark"}"#
    );
    assert!(!scratch.path().join(".config.json").exists());
    let after = [&config, &settings].map(|path| std::fs::metadata(path).unwrap().modified().ok());
    assert_eq!(before, after);
    // An empty home seeds nothing and makes nothing there.
    let empty = dir.0.join("empty");
    crate::write::create_dir_all(&empty).unwrap();
    let bare = Scratch::create(&data, &claude(), UNIX_EPOCH).unwrap();
    bare.seed(&empty);
    assert_eq!(std::fs::read_dir(bare.path()).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(&empty).unwrap().count(), 0);
}
