//! A Chrome record left saying `starting`: the guard was killed between that record and the
//! next, maybe after Chrome's spawn, so no pid reached its reaper (decision record 0029, "Chrome
//! records"). The reaper and the next run's sweep end whatever runs with the run's profile, then
//! delete the record, so `chrome_groups` never reads `Starting` for that run forever. The
//! Chrome stand-in plays the Chrome nobody was told about. Waits poll as `lifecycle.rs`'s do.

#![cfg(unix)]

use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::Command;

use clauseo_mcp::browser_guard::{ChromeState, chrome_groups};
use serde_json::json;

use crate::support::{CHROME, GUARD, Guard, Scratch, example, guard_command, run_files};
use crate::write;

/// A dead run's id under `--cwd`.
const STALE: &str = "4000000-0badf00d";

/// The files a guard killed right after its `starting` record leaves: a free lock file, the
/// profile and the record.
fn stale_run(cwd: &Path) {
    let name = |suffix: &str| cwd.join(format!("chrome-profile-{STALE}{suffix}"));
    write::write(name(".lock"), "").unwrap();
    write::create_dir_all(name("")).unwrap();
    write::write(name(".chrome"), "starting").unwrap();
}

/// The Chrome stand-in running with the stale run's profile, as the killed guard spawned it.
/// Started as a piped child (in the test's process group, so only it is signalled).
fn orphan_chrome(scratch: &Scratch) -> Guard {
    let mut flag = String::from("--user-data-dir=");
    flag.push_str(
        scratch
            .cwd()
            .join(format!("chrome-profile-{STALE}"))
            .to_str()
            .unwrap(),
    );
    let mut command = Command::new(scratch.path().join(CHROME));
    command.args([flag.as_str(), "--remote-debugging-port=61198"]);
    let chrome = Guard::start(command);
    // Its pid file says it runs.
    crate::lifecycle::pid_in(&scratch.path().join("chrome.pid"));
    chrome
}

/// With nothing running with its profile, the next run's sweep deletes the stale run's record
/// with the rest of it: only the new run is listed.
#[test]
fn the_sweep_clears_a_starting_record_with_no_chrome() {
    let scratch = Scratch::new("record_stale_none");
    let cwd = scratch.cwd();
    stale_run(&cwd);
    assert_eq!(chrome_groups(&cwd), [ChromeState::Starting]);
    let mut guard = Guard::start(guard_command(&scratch, &example("daemon_stub"), &[]));
    guard.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}));
    guard.read();
    assert_eq!(
        chrome_groups(&cwd),
        [ChromeState::None],
        "the stale run is listed"
    );
    let (status, _) = guard.finish();
    assert_eq!(status.code(), Some(0));
    assert_eq!(chrome_groups(&cwd), []);
}

/// A Chrome running with the stale run's profile: the next run's sweep kills it, then deletes
/// the record.
#[test]
fn the_sweep_ends_a_starting_runs_chrome_then_clears_the_record() {
    let scratch = Scratch::new("record_stale_live");
    let cwd = scratch.cwd();
    stale_run(&cwd);
    let mut chrome = orphan_chrome(&scratch);
    let mut guard = Guard::start(guard_command(&scratch, &example("daemon_stub"), &[]));
    guard.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}));
    guard.read();
    assert_eq!(
        chrome.wait().signal(),
        Some(9),
        "the orphan Chrome was not killed"
    );
    assert_eq!(
        chrome_groups(&cwd),
        [ChromeState::None],
        "the stale run is listed"
    );
    let (status, _) = guard.finish();
    assert_eq!(status.code(), Some(0));
    assert_eq!(chrome_groups(&cwd), []);
}

/// The reaper of a guard killed after its `starting` record, and told no Chrome pid: it kills
/// the Chrome running with the run's profile, then deletes the record, the profile and the lock.
#[test]
fn the_reaper_ends_a_starting_runs_chrome_then_clears_the_record() {
    let scratch = Scratch::new("record_stale_reaper");
    let cwd = scratch.cwd();
    stale_run(&cwd);
    let mut chrome = orphan_chrome(&scratch);
    let mut reaper = Command::new(GUARD);
    reaper
        .arg("browser-reaper")
        .arg("--socket-dir")
        .arg(scratch.socket_dir())
        .args(["--session", &format!("run-{STALE}")])
        .arg("--cwd")
        .arg(&cwd)
        .args(["--run", STALE]);
    // The guard's end: the reaper's stdin ends with no `done`.
    let (status, said) = Guard::start(reaper).finish();
    assert_eq!(said, "ready\n");
    assert_eq!(status.code(), Some(0));
    assert_eq!(
        chrome.wait().signal(),
        Some(9),
        "the orphan Chrome was not killed"
    );
    assert_eq!(chrome_groups(&cwd), [], "the record was kept");
    assert_eq!(run_files(&cwd), Vec::<std::path::PathBuf>::new());
}
