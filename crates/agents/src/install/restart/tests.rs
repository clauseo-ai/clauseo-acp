use super::*;
use crate::platform::{Arch, Os};
use crate::write::TestDir;

fn agent() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

fn archive(build: u32) -> PythonArchive {
    let name = format!("clauseo-python-3.13.18-c{build}-linux-x64.tar.gz");
    PythonArchive::new(
        Os::Linux,
        Arch::X64,
        &name,
        "https://p.test/a",
        3,
        9,
        &"ab".repeat(32),
    )
    .unwrap()
}

#[test]
fn an_agent_s_marker_is_written_read_and_removed() {
    let data = TestDir::new("restart-wanted");
    assert!(!is_wanted(&data.0, &agent()));
    mark_wanted(&data.0, &agent());
    assert!(is_wanted(&data.0, &agent()));
    clear_wanted(&data.0, &agent());
    assert!(!is_wanted(&data.0, &agent()));
    // Removing a marker that is not there is fine.
    clear_wanted(&data.0, &agent());
}

#[cfg(unix)]
#[test]
fn a_link_at_the_agent_s_folder_is_never_read_or_written_through() {
    let data = TestDir::new("restart-link");
    let outside = TestDir::new("restart-outside");
    outside.file(WANTED, b"1\n");
    crate::write::create_dir_all(&data.0.join("agents")).unwrap();
    crate::write::symlink(&outside.0, &data.0.join("agents").join("claude-acp")).unwrap();
    assert!(!is_wanted(&data.0, &agent()));
    clear_wanted(&data.0, &agent());
    assert!(
        outside.0.join(WANTED).exists(),
        "nothing removed through the link"
    );
}

#[test]
fn python_s_marker_keeps_what_refused_it() {
    let data = TestDir::new("restart-blocked");
    crate::write::create_dir_all(&data.0.join("agents")).unwrap();
    let (one, two) = (archive(1), archive(2));
    assert_eq!(blocked(&data.0, Some(&one)), None);
    mark_blocked(&data.0, &one, Some(PythonBlocker::AppLocker));
    assert_eq!(blocked(&data.0, Some(&one)), Some(PythonBlocker::AppLocker));
    // An app update's new archive is tried again; no Python here, nothing blocked.
    assert_eq!(blocked(&data.0, Some(&two)), None);
    assert_eq!(blocked(&data.0, None), None);
    mark_blocked(&data.0, &one, None);
    assert_eq!(blocked(&data.0, Some(&one)), Some(PythonBlocker::Unknown));
    let PythonState::Failed(failure) = blocked_state(PythonBlocker::AppControl, &agent()) else {
        panic!("not Failed");
    };
    assert_eq!(failure.kind, PythonFailureKind::RunBlocked);
    assert_eq!(failure.blocked_by, Some(PythonBlocker::AppControl));
    clear_blocked(&data.0);
    assert_eq!(blocked(&data.0, Some(&one)), None);
}

#[test]
fn an_older_pin_s_archives_go_and_the_current_one_s_stay() {
    let data = TestDir::new("restart-archives");
    let old = data.file(
        "agents/downloads/clauseo-python-3.13.18-c1-linux-x64.tar.gz",
        b"o",
    );
    let old_part = data.file(
        "agents/downloads/clauseo-python-3.13.18-c1-linux-x64.tar.gz.part",
        b"o",
    );
    let current = archive(2);
    let new = data.file(&format!("agents/downloads/{}", current.name()), b"n");
    let new_part = data.file(&format!("agents/downloads/{}.part", current.name()), b"n");
    let node = data.file("agents/downloads/node-v24.11.1-linux-x64.tar.gz", b"x");
    let other = data.file("agents/downloads/clauseo-python-notes.txt", b"x");
    crate::write::create_dir_all(&data.0.join("agents/downloads/clauseo-python-x.tar.gz")).unwrap();
    remove_stale_archives(&data.0, Some(&current));
    assert!(
        !old.exists() && !old_part.exists(),
        "the older pin's archive goes"
    );
    assert!(new.exists() && new_part.exists(), "the current one stays");
    assert!(node.exists() && other.exists(), "nothing else is touched");
    assert!(
        data.0
            .join("agents/downloads/clauseo-python-x.tar.gz")
            .is_dir()
    );
    // No Python for this platform: every archive goes.
    remove_stale_archives(&data.0, None);
    assert!(!new.exists() && !new_part.exists());
}
