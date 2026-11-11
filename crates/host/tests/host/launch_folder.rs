//! A turn whose matter folder is gone, is there without its marker, or is one the agent may not
//! open: the turn fails with `matter.folder_missing`, `matter.folder_not_a_matter` or
//! `matter.folder_permission`, not `agent.launch_failed` or `agent.protocol_error`, and no path is
//! in the failure's detail.

use clauseo_protocol::{ErrorCode, StopReason, StreamId, TurnFailed};

use crate::codex::turn_failed;
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread};
use crate::write;

/// Prompts `thread` once and answers its `TurnFailed`, which it must end with.
fn failed_turn(host: &Running, thread: clauseo_protocol::ThreadId) -> TurnFailed {
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Summarise the brief.");
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::Failed, "{}", events.dump());
    turn_failed(&events, turn).unwrap_or_else(|| panic!("{}", events.dump()))
}

/// The failure names neither the temp root the matter's folder sits in nor the data folder.
fn assert_no_path(dir: &Dir, failed: &TurnFailed) {
    let detail = failed.detail.clone().unwrap_or_default();
    for root in [dir.work_path(""), dir.data().to_path_buf()] {
        assert!(!detail.contains(&*root.to_string_lossy()), "{detail}");
    }
}

#[test]
fn a_deleted_folder_fails_the_turn_as_folder_missing_with_no_path() {
    let dir = Dir::new(
        "launch_folder_gone",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (_, thread, folder) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    write::remove_dir_all(&folder).unwrap();
    let failed = failed_turn(&host, thread);
    assert_eq!(failed.code, ErrorCode::MatterFolderMissing, "{failed:?}");
    assert_no_path(&dir, &failed);
    host.shutdown();
}

#[test]
fn a_folder_without_its_marker_fails_the_turn_as_not_a_matter_with_no_path() {
    let dir = Dir::new(
        "launch_folder_no_marker",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (_, thread, folder) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    // Removed while the host runs: no relink comes (that is for the first open of a run).
    write::remove_file(&folder.join(".clauseo/matter.json"));
    let failed = failed_turn(&host, thread);
    assert_eq!(failed.code, ErrorCode::MatterFolderNotAMatter, "{failed:?}");
    assert_no_path(&dir, &failed);
    host.shutdown();
}

/// Claude's adapter answers `session/new` in a folder macOS keeps it out of with -32602 "`cwd`
/// does not exist" (a real 0.89.1 answer; its path is the agent's, not the test's): the turn fails
/// as `matter.folder_permission`, and neither that path nor the test's is in the detail.
#[test]
fn claudes_cwd_answer_fails_the_turn_as_folder_permission_with_no_path() {
    let dir = Dir::new(
        "launch_folder_cwd_refused",
        &[(FAKE, Scenario::Local("claude-new-refused-cwd"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let failed = failed_turn(&host, thread);
    assert_eq!(failed.code, ErrorCode::MatterFolderPermission, "{failed:?}");
    assert_no_path(&dir, &failed);
    let detail = failed.detail.unwrap_or_default();
    assert!(!detail.contains("Quarrington"), "{detail}");
    host.shutdown();
}

/// macOS: a matter folder the host may not open (here its mode is 000, which refuses the open as
/// TCC does) fails the turn as `matter.folder_permission` before the agent starts, with no path.
#[cfg(target_os = "macos")]
#[test]
fn a_folder_the_host_may_not_open_fails_the_turn_as_folder_permission() {
    let dir = Dir::new(
        "launch_folder_no_access",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (_, thread, folder) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    write::set_mode(&folder, 0o000);
    let failed = failed_turn(&host, thread);
    write::set_mode(&folder, 0o755);
    assert_eq!(failed.code, ErrorCode::MatterFolderPermission, "{failed:?}");
    assert_no_path(&dir, &failed);
    host.shutdown();
}
