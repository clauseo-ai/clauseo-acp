//! The guard ended while Chrome is still starting, and Chrome dying just as the agent goes. The
//! Chrome stand-in's `silent` mode never prints its endpoint, so the guard stays in its wait for
//! it (up to 15 s) for as long as a test needs; the test acts once the guard says on stderr that
//! Chrome has started (after it told the reaper). Waits poll as `lifecycle.rs`'s do.

use std::path::PathBuf;
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use crate::lifecycle::{alive, no_args, pid_in, sigkill, throughout, within};
#[cfg(unix)]
use crate::support::Guard;
use crate::support::{Scratch, example, guard_command, run};

/// What the guard prints once Chrome runs and the reaper knows it.
#[cfg(unix)]
const STARTED: &str = "Chrome started; waiting for it to listen";

/// What the guard prints once Chrome listens on the claimed port.
#[cfg(unix)]
const LISTENING: &str = "Chrome is listening on the claimed debugging port";

/// Chrome is gone, its profile too, and on Linux no process names the scratch folder.
#[cfg(unix)]
fn assert_chrome_gone(scratch: &Scratch, chrome: i32) {
    assert!(within(|| !alive(chrome)), "Chrome {chrome} is left");
    let left = || crate::support::run_files(&scratch.cwd());
    assert!(within(|| left().is_empty()), "left: {:?}", left());
    #[cfg(target_os = "linux")]
    {
        let left = || crate::support::left_in(scratch.path());
        assert!(within(|| left().is_empty()), "left behind: {:?}", left());
    }
}

/// The review's repro: a SIGKILL of the guard while it waits for Chrome's endpoint left Chrome
/// running, as the reaper learnt Chrome's pid only after the wait.
#[cfg(unix)]
#[test]
fn a_guard_killed_while_chrome_starts_leaves_no_chrome() {
    let scratch = Scratch::new("race_kill_start");
    scratch.chrome_mode("silent");
    let mut guard = Guard::start(guard_command(&scratch, &example("daemon_stub"), &no_args()));
    guard.stderr_until(STARTED);
    let chrome = pid_in(&scratch.path().join("chrome.pid"));
    sigkill(i32::try_from(guard.pid()).unwrap());
    assert_chrome_gone(&scratch, chrome);
    drop(guard);
}

/// The agent (`parent_stub`) is killed while Chrome starts and the test holds the guard's stdin:
/// the guard notices during its wait for Chrome, ends Chrome and exits 1. Before the fix it read
/// its parent only once relaying, by when the parent it read was its adoptive one.
#[cfg(unix)]
#[test]
fn a_parent_killed_while_chrome_starts_ends_the_guard() {
    use crate::support::{GUARD, guard_args, host_env};
    let scratch = Scratch::new("race_parent_start");
    scratch.chrome_mode("silent");
    let guard_pid_file = scratch.path().join("guard.pid");
    let mut command = std::process::Command::new(example("parent_stub"));
    command
        .arg(GUARD)
        .args(guard_args(&scratch, &example("daemon_stub"), &no_args()))
        .envs(host_env(&scratch))
        .env("PARENT_STUB_CHILD_PID", &guard_pid_file);
    let mut parent = Guard::start(command);
    parent.stderr_until(STARTED);
    let guard = pid_in(&guard_pid_file);
    let chrome = pid_in(&scratch.path().join("chrome.pid"));
    sigkill(i32::try_from(parent.pid()).unwrap());
    assert!(within(|| !alive(guard)), "the guard {guard} is left");
    let stderr = parent.stderr_text();
    assert!(
        stderr.contains(
            "clauseo-mcp browser-guard: the process that started the guard has gone; the guard \
             ended the session's browser"
        ),
        "{stderr}"
    );
    assert_chrome_gone(&scratch, chrome);
    drop(parent);
}

/// Chrome dies just as the agent goes: the stand-in exits right after it prints its endpoint,
/// and the agent's stdin is closed from the start, so agent-browser has answered the close and
/// exited long before Chrome's watcher first looks (250 ms). The guard still exits 1.
#[test]
fn chrome_that_dies_as_the_agent_goes_is_still_reported() {
    let scratch = Scratch::new("race_chrome_eof");
    scratch.chrome_mode("listen-exit");
    let log = scratch.log();
    let args = [log.as_os_str()];
    let output = run(guard_command(&scratch, &example("echo_stub"), &args));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("clauseo-mcp browser-guard: Chrome exited; the guard ended agent-browser"),
        "{stderr}"
    );
    assert!(scratch.log().exists(), "agent-browser ran");
    assert_eq!(
        crate::support::run_files(&scratch.cwd()),
        Vec::<PathBuf>::new()
    );
}

/// The code owner's repro (M1): guard A is killed once Chrome listens, and guard B starts at once
/// with the same `--cwd` and ports, as an MCP reconnect does. A's reaper must end A's run alone:
/// B's profile, its staging folder and its daemon stay, for longer than A's reaper takes (it
/// ends A's daemon a second time about 1 s after A dies). Before each run had a profile and a
/// daemon socket of its own, A's reaper deleted B's profile in 5 of 5 runs.
#[cfg(unix)]
#[test]
fn a_killed_guards_reaper_leaves_the_next_run_alone() {
    let scratch = Scratch::new("race_rerun");
    let stub = example("daemon_stub");
    let mut first = Guard::start(guard_command(&scratch, &stub, &no_args()));
    first.stderr_until(LISTENING);
    sigkill(i32::try_from(first.pid()).unwrap());
    let mut next = Guard::start(guard_command(&scratch, &stub, &no_args()));
    next.stderr_until(LISTENING);
    let pid = next.pid();
    let profile = std::fs::read_to_string(scratch.path().join(format!("profile-of-{pid}")));
    let profile = PathBuf::from(profile.unwrap());
    let preferences = profile.join("Default").join("Preferences");
    let staging = profile.join("downloads-staging");
    assert!(
        throughout(Duration::from_secs(3), || preferences.is_file()
            && staging.is_dir()),
        "the next run's profile {} or its staging folder went",
        profile.display()
    );
    let daemon = pid_in(&scratch.socket_dir().join(format!("daemon-of-{pid}.pid")));
    #[cfg(target_os = "linux")]
    {
        let reapers = || {
            crate::support::left_in(scratch.path())
                .iter()
                .filter(|process| process.contains("browser-reaper"))
                .count()
        };
        assert!(within(|| reapers() <= 1), "the first run's reaper is left");
    }
    assert!(alive(daemon), "the next run's daemon {daemon} was killed");
    let (status, _) = next.finish();
    assert_eq!(status.code(), Some(0));
    assert!(within(|| !alive(daemon)), "the daemon {daemon} is left");
    let left = || crate::support::run_files(&scratch.cwd());
    assert!(within(|| left().is_empty()), "left: {:?}", left());
    drop(first);
}

/// A daemon that binds its socket only after the guard's cleanup and the reaper's first kill
/// (`daemon_stub late`, in a group of its own) is still ended: the reaper ends the run's daemon
/// once more about 1 s after the guard dies. Before, it lived until its idle timeout.
#[cfg(unix)]
#[test]
fn a_daemon_that_binds_after_a_killed_guards_cleanup_is_ended() {
    let scratch = Scratch::new("race_late_kill");
    let late = [std::ffi::OsStr::new("late")];
    let mut guard = Guard::start(guard_command(&scratch, &example("daemon_stub"), &late));
    guard.stderr_until(LISTENING);
    let daemon = pid_in(&scratch.socket_dir().join("late.pid"));
    let chrome = pid_in(&scratch.path().join("chrome.pid"));
    sigkill(i32::try_from(guard.pid()).unwrap());
    // Chrome gone: the reaper has run its first daemon kill, which found no socket.
    assert!(within(|| !alive(chrome)), "Chrome {chrome} is left");
    crate::write::write(scratch.socket_dir().join("late-go"), "").unwrap();
    assert!(
        within(|| !alive(daemon)),
        "the late daemon {daemon} is left"
    );
    drop(guard);
}

/// The same after the guard's own cleanup at the end of its stdin: the reaper, told `done`,
/// ends the run's daemon once more about 1 s later.
#[cfg(unix)]
#[test]
fn a_daemon_that_binds_after_the_guards_cleanup_is_ended() {
    let scratch = Scratch::new("race_late_done");
    let late = [std::ffi::OsStr::new("late")];
    let mut guard = Guard::start(guard_command(&scratch, &example("daemon_stub"), &late));
    guard.stderr_until(LISTENING);
    let daemon = pid_in(&scratch.socket_dir().join("late.pid"));
    let (status, _) = guard.finish();
    assert_eq!(status.code(), Some(0));
    crate::write::write(scratch.socket_dir().join("late-go"), "").unwrap();
    assert!(
        within(|| !alive(daemon)),
        "the late daemon {daemon} is left"
    );
}
