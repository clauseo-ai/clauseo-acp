//! Lazy start and the idle stop with the real agent-browser v0.41.1 and a real Chrome, on
//! `real_chrome.rs`'s helpers and inputs: no Chrome until the first call, and after the idle stop
//! (shortened through the `test-support` knob) no Chrome or daemon until the next call, which
//! opens the page again.

use std::time::{Duration, Instant};

use serde_json::json;

use super::{Session, binaries, call, command, handshake, processes};
use crate::lifecycle::sigkill;
use crate::page_proxy::{HEADING, PageProxy};
use crate::support::{Guard, Scratch};

/// Whether any process runs with the scratch folder's profile flag: the session's Chrome.
fn chrome_runs(scratch: &Scratch) -> bool {
    !chrome_pids(scratch).is_empty()
}

/// The processes running with the scratch folder's profile flag (zombies have no command line).
fn chrome_pids(scratch: &Scratch) -> Vec<i32> {
    let flag = format!(
        "--user-data-dir={}",
        scratch.cwd().join("chrome-profile-").display()
    );
    processes()
        .into_iter()
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|cmd| {
                cmd.split(|b| *b == 0)
                    .any(|arg| arg.starts_with(flag.as_bytes()))
            })
        })
        .collect()
}

/// Whether `ready` holds within 10 s, looking every 50 ms.
fn within(mut ready: impl FnMut() -> bool) -> bool {
    (0..200).any(|_| {
        let now = ready();
        if !now {
            std::thread::sleep(Duration::from_millis(50));
        }
        now
    })
}

/// The handshake starts no Chrome and makes no profile; the first call opens the page; the idle
/// stop (3 s here) ends Chrome's group and the daemon and keeps the profile, and the run's Chrome
/// record says so; the next call starts both again and opens the page; the guard's exit leaves
/// nothing and the run settles.
#[test]
fn chrome_starts_at_the_first_call_and_stops_when_idle() {
    use clauseo_mcp::browser_guard::{ChromeState, chrome_groups, runs_settled};
    let Some(binaries) = binaries() else { return };
    let scratch = Scratch::new("real_lazy");
    let proxy = PageProxy::start();
    let mut command = command(&scratch, &binaries, &proxy);
    command.env("CLAUSEO_TEST_GUARD_IDLE_MS", "3000");
    let started = Instant::now();
    let mut guard = Guard::start(command);
    handshake(&mut guard);
    eprintln!(
        "handshake, no Chrome: {:.2} s",
        started.elapsed().as_secs_f64()
    );
    assert!(!chrome_runs(&scratch), "Chrome started at the handshake");
    assert!(
        crate::support::profiles(&scratch.cwd()).is_empty(),
        "a profile at the handshake"
    );
    assert_eq!(chrome_groups(&scratch.cwd()), [ChromeState::None]);
    let open = json!({"url": "http://page.test/"});
    call(&mut guard, 3, "agent_browser_open", &open);
    let first = Session::find(&scratch);
    let [ChromeState::Running(process)] = chrome_groups(&scratch.cwd())[..] else {
        panic!("{:?}", chrome_groups(&scratch.cwd()));
    };
    assert_eq!(i32::try_from(process.id).unwrap(), first.chrome_group);
    guard.stderr_until("stopped Chrome and agent-browser, keeping the profile");
    // The guard and its reaper still run; nothing else of the session may.
    let browser_left = || -> Vec<String> {
        first
            .left()
            .into_iter()
            .filter(|process| {
                !process.contains("clauseo-mcp browser-guard")
                    && !process.contains("clauseo-mcp browser-reaper")
            })
            .collect()
    };
    let stopped = within(|| browser_left().is_empty());
    assert!(stopped, "left after the idle stop: {:?}", browser_left());
    assert!(process.gone(), "the stopped Chrome's group read as alive");
    assert_eq!(chrome_groups(&scratch.cwd()), [ChromeState::None]);
    assert_eq!(
        crate::support::profiles(&scratch.cwd()).len(),
        1,
        "the profile went"
    );
    assert!(!runs_settled(&scratch.cwd()));
    call(&mut guard, 4, "agent_browser_open", &open);
    let snapshot = call(&mut guard, 5, "agent_browser_snapshot", &json!({}));
    assert!(
        snapshot.to_string().contains(HEADING),
        "snapshot: {snapshot}"
    );
    let second = Session::find(&scratch);
    assert_ne!(second.chrome_group, first.chrome_group);
    let (status, _) = guard.finish();
    assert_eq!(status.code(), Some(0));
    second.assert_gone(&scratch);
    assert!(runs_settled(&scratch.cwd()));
    assert_eq!(chrome_groups(&scratch.cwd()), []);
}

/// Chrome's second death with the real binaries while the agent floods tool calls: once the
/// guard's final stop has run, no call starts Chrome again, so once the guard has exited and its
/// reaper is done, no process of the session runs. Before the fix a full Chrome tree was left
/// on the claimed port (the review's 4 of 4 rounds).
#[test]
fn calls_after_the_final_stop_start_no_chrome() {
    let Some(binaries) = binaries() else { return };
    let scratch = Scratch::new("real_flood");
    let proxy = PageProxy::start();
    let mut guard = Guard::start(command(&scratch, &binaries, &proxy));
    handshake(&mut guard);
    let open = json!({"url": "http://page.test/"});
    call(&mut guard, 3, "agent_browser_open", &open);
    let first = Session::find(&scratch);
    sigkill(first.chrome_group);
    guard.stderr_until("starts both again at the next browser call");
    assert!(
        within(|| !chrome_runs(&scratch)),
        "the first Chrome is left"
    );
    call(&mut guard, 4, "agent_browser_open", &open);
    let second = Session::find(&scratch);
    sigkill(second.chrome_group);
    let status = guard.flood(|n| {
        json!({"jsonrpc": "2.0", "id": 100 + n, "method": "tools/call",
            "params": {"name": "agent_browser_snapshot", "arguments": {}}})
    });
    assert_eq!(status.code(), Some(1));
    let left = || {
        let mut left = second.left();
        left.extend(chrome_pids(&scratch).iter().map(i32::to_string));
        left
    };
    if !within(|| left().is_empty()) {
        let left = left();
        for pid in chrome_pids(&scratch) {
            sigkill(pid);
        }
        panic!("left behind: {left:?}");
    }
    assert_eq!(
        crate::support::run_files(&scratch.cwd()),
        Vec::<std::path::PathBuf>::new()
    );
}
