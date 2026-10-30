//! The shutdown ladder and the exit report, against `fake_agent`'s `idle`,
//! `ignore-stdin-close` and `exit-mid-turn` scenarios.

use std::fmt::Write as _;
use std::io;
use std::time::Duration;

use async_io::block_on;
use clauseo_process::{Error, Launched, ManualTimer, ShutdownOutcome};
use clauseo_protocol::{DetachReason, ErrorCode};
use futures::{AsyncWriteExt, join};
use serde_json::json;

use crate::harness::{RunDir, launch, launch_program, lines};

/// Each of the 200 stderr lines `exit-mid-turn` writes before it exits 1.
const FILLER: &str = "fake_agent exit-mid-turn stderr filler.\n";

#[test]
fn idle_exits_on_stdin_close() {
    let dir = RunDir::new();
    let Launched {
        mut stdin,
        stdout: _stdout,
        stderr,
        handle,
        ..
    } = launch("idle", &dir).unwrap();
    let timer = ManualTimer::new();
    let (outcome, pumped, armed) = block_on(async {
        let (outcome, pumped) = join!(handle.shutdown(&timer), stderr.drain());
        (outcome, pumped, timer.next_armed().await)
    });
    let ShutdownOutcome::StdinClosed(report) = outcome else {
        panic!("expected StdinClosed, got {outcome:?}");
    };
    assert_eq!(report.code(), Some(0));
    assert_eq!(report.signal(), None);
    assert_eq!(report.stderr_tail(), "");
    pumped.unwrap();
    // The 2 s wait was armed, and the agent exited before anything fired it.
    assert_eq!(armed, Duration::from_secs(2));
    assert_eq!(timer.fired(), 0);
    // The handle closed stdin, so the writer's next write fails.
    let error = block_on(stdin.write_all(b"{}\n")).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    block_on(stdin.close()).unwrap();
}

#[cfg(unix)]
#[test]
fn ignore_stdin_close_escalates_to_sigterm() {
    let dir = RunDir::new();
    let Launched {
        stdout: _stdout,
        stderr,
        handle,
        ..
    } = launch("ignore-stdin-close", &dir).unwrap();
    let timer = ManualTimer::new();
    let (pumped, mut lines) = lines(stderr);
    let script = async {
        // The agent has read the end of stdin and holds: only the timer moves the ladder on.
        lines.until("fake_agent: stdin closed, holding").await;
        assert_eq!(timer.next_armed().await, Duration::from_secs(2));
        assert!(timer.fire());
        assert_eq!(timer.next_armed().await, Duration::from_secs(3));
    };
    let (outcome, (), pumped) = block_on(async { join!(handle.shutdown(&timer), script, pumped) });
    let ShutdownOutcome::Terminated(report) = outcome else {
        panic!("expected Terminated, got {outcome:?}");
    };
    assert_eq!(
        report.signal(),
        Some(nix::sys::signal::Signal::SIGTERM as i32)
    );
    assert_eq!(report.code(), None);
    pumped.unwrap();
    assert_eq!(timer.fired(), 1, "the 3 s wait was armed and never fired");
}

/// Starts `scenario` with SIGTERM ignored, which the agent inherits across exec (std resets only
/// SIGPIPE in the child), so the SIGTERM step cannot end it. A stopped agent will not do: macOS
/// ends a stopped process on SIGTERM, which raced the 3 s wait.
#[cfg(unix)]
#[expect(
    unsafe_code,
    reason = "nix's signal() is unsafe; the agent inherits SIGTERM ignored across exec"
)]
fn launch_ignoring_sigterm(scenario: &str, dir: &RunDir) -> Launched {
    use nix::sys::signal::{SigHandler, Signal, signal};

    // SAFETY: SIG_IGN installs no handler code, so nothing can run in signal context. Nextest runs
    // this test alone in its own process, and the old disposition is put back right after the
    // launch, so nothing else in the process depends on SIGTERM meanwhile.
    let old = unsafe { signal(Signal::SIGTERM, SigHandler::SigIgn) }.expect("ignore SIGTERM");
    let launched = launch(scenario, dir);
    // SAFETY: `old` is the disposition the process had a moment ago, restored as it was.
    unsafe { signal(Signal::SIGTERM, old) }.expect("restore SIGTERM");
    launched.unwrap()
}

#[cfg(unix)]
#[test]
fn sigterm_ignoring_agent_is_killed() {
    let dir = RunDir::new();
    let Launched {
        stdout: _stdout,
        stderr,
        handle,
        ..
    } = launch_ignoring_sigterm("ignore-stdin-close", &dir);
    let timer = ManualTimer::new();
    let (pumped, mut lines) = lines(stderr);
    let script = async {
        // It has read the end of stdin and holds, and SIGTERM is ignored: only SIGKILL ends it.
        lines.until("fake_agent: stdin closed, holding").await;
        assert_eq!(timer.next_armed().await, Duration::from_secs(2));
        assert!(timer.fire());
        assert_eq!(timer.next_armed().await, Duration::from_secs(3));
        assert!(timer.fire());
    };
    let (outcome, (), pumped) = block_on(async { join!(handle.shutdown(&timer), script, pumped) });
    let ShutdownOutcome::Killed(report) = outcome else {
        panic!("expected Killed, got {outcome:?}");
    };
    assert_eq!(
        report.signal(),
        Some(nix::sys::signal::Signal::SIGKILL as i32)
    );
    assert_eq!(report.code(), None);
    pumped.unwrap();
    assert_eq!(timer.fired(), 2);
}

#[cfg(windows)]
#[test]
fn ignore_stdin_close_terminates_job() {
    let dir = RunDir::new();
    let Launched {
        stdout: _stdout,
        stderr,
        handle,
        ..
    } = launch("ignore-stdin-close", &dir).unwrap();
    let timer = ManualTimer::new();
    let (pumped, mut lines) = lines(stderr);
    let script = async {
        lines.until("fake_agent: stdin closed, holding").await;
        assert_eq!(timer.next_armed().await, Duration::from_secs(2));
        assert!(timer.fire());
    };
    let (outcome, (), pumped) = block_on(async { join!(handle.shutdown(&timer), script, pumped) });
    let ShutdownOutcome::Killed(report) = outcome else {
        panic!("expected Killed, got {outcome:?}");
    };
    // `TerminateJobObject`'s exit code; Windows has no SIGTERM step, so no 3 s wait.
    assert_eq!(report.code(), Some(1));
    assert_eq!(report.signal(), None);
    pumped.unwrap();
    assert_eq!(timer.fired(), 1);
    assert_eq!(timer.armed(), 1);
}

#[test]
fn exit_mid_turn_reports_code_and_tail() {
    let dir = RunDir::new();
    let Launched {
        mut stdin,
        stdout: _stdout,
        stderr,
        mut handle,
        ..
    } = launch("exit-mid-turn", &dir).unwrap();
    let requests = [
        json!({"jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {"protocolVersion": 1, "clientCapabilities": {}}}),
        json!({"jsonrpc": "2.0", "id": 1, "method": "session/new",
            "params": {"cwd": dir.path(), "mcpServers": []}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "session/prompt",
            "params": {"sessionId": "sess-1", "prompt": [{"type": "text", "text": "Go."}]}}),
    ];
    let mut text = String::new();
    for request in &requests {
        writeln!(text, "{request}").unwrap();
    }
    let (report, pumped) = block_on(async {
        stdin.write_all(text.as_bytes()).await.unwrap();
        stdin.flush().await.unwrap();
        join!(handle.exited(), stderr.drain())
    });
    pumped.unwrap();
    assert_eq!(report.code(), Some(1));
    assert_eq!(report.signal(), None);
    let tail = report.stderr_tail();
    assert!(tail.len() <= 4096, "{}", tail.len());
    // 200 lines of 40 bytes: the tail keeps the last whole ones, the 200th last.
    assert_eq!(tail, FILLER.repeat(4096 / FILLER.len()));
    assert_eq!(
        report.detach_reason(),
        DetachReason::Exited {
            code: Some(1),
            stderr_tail: tail,
        }
    );
}

#[test]
fn missing_program_is_launch_failed() {
    let dir = RunDir::new();
    let missing = dir.path().join("no-such-agent");
    let error = launch_program(missing.clone(), &[], &dir).unwrap_err();
    let Error::Spawn { program, source } = &error else {
        panic!("expected Spawn, got {error:?}");
    };
    assert_eq!(program, &missing);
    assert_eq!(source.kind(), io::ErrorKind::NotFound);
    assert_eq!(error.code(), ErrorCode::AgentLaunchFailed);
}
