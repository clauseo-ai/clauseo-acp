//! The lawyer's Stop while a turn waits for its agent's launch ends the turn at once and stops
//! the launch, leaving nothing running (`actor/turn/cancel.rs` `abandon_launch`): during the
//! guard's self-test (a guard that never answers, on the host's stopped clock, so only the Stop
//! can end it: `guard_selftest::ask_until`), and during a Claude session call with the settings
//! canary on, the first one or the canary's retry, which the agent never answers. The agent's
//! child, which a stop ends with the agent's tree, stands in for the agent.
//!
//! A process the host kills stays its living parent's unreaped zombie on Unix, so [`gone_within`]
//! asks the OS whether it still runs, under a deadline (as `cancel_leftovers.rs` does).

use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_host::HostConfig;
use clauseo_protocol::{CommandOutcome, DomainEvent, HostCommand, StopReason, StreamId};

use crate::support::{Dir, Running, Scenario, matter_with_thread, wait};

/// The agent id that makes a fake agent a Claude agent.
const CLAUDE: &str = "claude-acp";

/// How long a stopped process may take to stop running, in real time: far under the
/// self-test's 10 s limit, which the stopped clock never reaches anyway.
const GONE_WITHIN: Duration = Duration::from_secs(5);

/// The pid in `file` once something wrote it whole, asked every 10 ms.
fn pid_when_written(file: &Path) -> u32 {
    wait("a pid file", async {
        loop {
            let text = std::fs::read_to_string(file).unwrap_or_default();
            if let Ok(pid) = text.trim().parse() {
                return pid;
            }
            async_io::Timer::after(Duration::from_millis(10)).await;
        }
    })
}

/// Whether `pid` stops running within [`GONE_WITHIN`] of real time, asked every 10 ms.
fn gone_within(pid: u32) -> bool {
    wait("a process to stop running", async {
        for _ in 0..GONE_WITHIN.as_millis() / 10 {
            if !clauseo_process::running(pid) {
                return true;
            }
            async_io::Timer::after(Duration::from_millis(10)).await;
        }
        !clauseo_process::running(pid)
    })
}

/// Kills the processes a failed assert leaves running.
struct Reap(u32);

impl Drop for Reap {
    fn drop(&mut self) {
        if clauseo_process::running(self.0) {
            clauseo_process::kill(self.0);
        }
    }
}

/// Prompts a new Claude thread, waits for the launch wait to start (`pid_file` of the matter
/// folder written), sends Stop, and checks the turn ends cancelled at once, the process in
/// `pid_file` is ended, and no session attached.
fn stop_during_launch(host: &Running, dir: &Dir, pid_file: impl Fn(&Path) -> PathBuf) {
    let (_, thread, folder) = matter_with_thread(dir, host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let (turn, _) = host.prompt(thread, "Summarise clause 4.");
    let pid = pid_when_written(&pid_file(&folder));
    let reap = Reap(pid);
    assert!(
        clauseo_process::running(pid),
        "the launch wait is not running"
    );
    assert_eq!(
        host.ok(HostCommand::Cancel { thread }),
        CommandOutcome::Done
    );
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::Cancelled, "{}", events.dump());
    assert!(
        gone_within(pid),
        "the stopped launch left its process running"
    );
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    let attached = events.count(|e| matches!(e, DomainEvent::AgentSessionAttached(_)));
    assert_eq!(attached, 0, "a stopped launch attached: {}", events.dump());
    let failed = events.count(|e| matches!(e, DomainEvent::TurnFailed(_)));
    assert_eq!(failed, 0, "{}", events.dump());
    drop(reap);
}

/// A guard whose self-test never answers: Stop ends the turn and the self-test's process, and
/// no agent starts.
#[cfg(unix)]
#[test]
fn stop_during_the_guard_self_test_ends_the_turn_and_the_self_test() {
    let dir = Dir::new(
        "launch_stop_selftest",
        &[(CLAUDE, Scenario::Local("claude-mode-new"))],
    );
    let pid_file = dir.work_path("guard.pid");
    let guard: PathBuf = dir.work_path("hanging-guard");
    // It notes its pid (kept by `exec`), then never answers.
    let script = format!(
        "#!/bin/sh\necho $$ > '{}'\nexec sleep 120\n",
        pid_file.display()
    );
    crate::write::write(&guard, script.as_bytes());
    crate::write::make_executable(&guard);
    let host = dir.start_with(0, |config: HostConfig| {
        config
            .with_pre_write_guard(guard.clone())
            .with_guard_selftest(true)
    });
    stop_during_launch(&host, &dir, |_| pid_file.clone());
    host.shutdown();
}

/// With the settings canary on, Stop while the agent holds its first `session/new`, or the
/// canary's retry after a first call whose canary never ran, ends the turn and stops the agent.
#[test]
fn stop_during_a_session_call_or_the_canary_retry_stops_the_agent() {
    for (test, scenario) in [
        ("launch_stop_new", "claude-canary-stop-new"),
        ("launch_stop_retry", "claude-canary-stop-retry"),
    ] {
        let dir = Dir::new(test, &[(CLAUDE, Scenario::Local(scenario))]);
        let host = dir.start_with(0, |config: HostConfig| {
            config
                // unix-paths-ok: a planted value, never run (the canary's entry names it).
                .with_pre_write_guard(PathBuf::from("/opt/clauseo/clauseo-desktop"))
                .with_settings_canary(true)
        });
        // The agent writes its child's pid in its working folder, the matter's.
        stop_during_launch(&host, &dir, |folder| folder.join("launch-wait.pid"));
        host.shutdown();
    }
}
