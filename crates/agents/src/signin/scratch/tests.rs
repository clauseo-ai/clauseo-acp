//! Detection's config folder: fresh per run, and removed on drop with what the CLI wrote.

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
    crate::write::write_atomic(&paths.0.join("written-by-the-cli"), b"{}").unwrap();
    crate::write::create_dir_all(&paths.0.join("backups")).unwrap();
    drop(one);
    assert!(!paths.0.exists());
    assert!(paths.1.is_dir());
    drop(two);
    assert!(!paths.1.exists());
}
