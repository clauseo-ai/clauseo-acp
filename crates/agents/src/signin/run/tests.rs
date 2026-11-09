//! The runner against small `sh` scripts (`/bin/sh` stands in for Node): the exit code, lines
//! from both streams, a pasted code on stdin, the ring's cap, and a kill by the limit or by
//! cancel. Waits end by the `ManualTimer` or by a line read, never by sleeping.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_process::{ManualTimer, PassthroughLauncher, SpawnOptions};
use futures::executor::block_on;
use futures::future::join;

use clauseo_protocol::AgentId;

use super::{Ended, RING_BYTES, Ran, RunSpec, run};
use crate::cancel::{Cancel, Stop};
use crate::signin::PasteCode;
use crate::signin::reuse::{login_file_for_tests, write_for_tests};
use crate::signin::stopping::{OnStop, Stopping};
use crate::write::TestDir;

/// A launcher that passes the child only the spec's variables, as production's must.
fn launcher() -> PassthroughLauncher {
    PassthroughLauncher::new(SpawnOptions::new().clear_env())
}

/// A spec running `script` (written into `dir`) with `/bin/sh`, `PATH` its only variable.
fn spec(dir: &TestDir, script: &str) -> RunSpec {
    let path = dir.file("run.sh", script.as_bytes());
    let env = BTreeMap::from([(
        OsString::from("PATH"),
        std::env::var_os("PATH").unwrap_or_default(),
    )]);
    RunSpec {
        node: PathBuf::from("/bin/sh"),
        script: path,
        args: Vec::new(),
        env,
        cwd: dir.0.clone(),
        limit: Duration::from_secs(5),
        on_stop: OnStop::KILL,
        agent: AgentId::new("codex-acp").unwrap(),
        lawyer_home: Some(dir.0.join("lawyer")),
    }
}

/// Runs `spec` to its end with no paste, collecting its lines.
fn run_plain(spec: RunSpec) -> (Ran, Vec<String>) {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&lines);
    let ran = block_on(run(
        &launcher(),
        &ManualTimer::new(),
        spec,
        &Cancel::new(),
        None,
        move |line| seen.lock().unwrap().push(line.to_owned()),
    ))
    .unwrap();
    let lines = lines.lock().unwrap().clone();
    (ran, lines)
}

#[test]
fn exit_code_and_lines_from_both_streams() {
    let dir = TestDir::new("run-lines");
    let (ran, lines) = run_plain(spec(
        &dir,
        "printf 'one\\ntwo\\r\\n'\necho err >&2\nprintf 'last'\nexit 3\n",
    ));
    assert_eq!(ran.ended, Ended::Exited(Some(3)));
    let mut sorted = lines.clone();
    sorted.sort();
    assert_eq!(sorted, ["err", "last", "one", "two"], "{lines:?}");
    let text = ran.output.text();
    assert!(text.contains("one\ntwo\r\n") && text.contains("err\n") && text.contains("last"));
}

#[test]
fn a_pasted_code_reaches_stdin_as_one_line() {
    let dir = TestDir::new("run-paste");
    let spec = spec(&dir, "read code\necho \"got:$code\"\n");
    let (codes, paste) = async_channel::bounded(1);
    let lines = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&lines);
    codes
        .try_send(PasteCode::from(" abc\r\n-def \n".to_owned()))
        .unwrap();
    let ran = block_on(run(
        &launcher(),
        &ManualTimer::new(),
        spec,
        &Cancel::new(),
        Some(paste),
        move |line| seen.lock().unwrap().push(line.to_owned()),
    ))
    .unwrap();
    drop(codes);
    assert_eq!(ran.ended, Ended::Exited(Some(0)));
    assert_eq!(*lines.lock().unwrap(), ["got:abc-def"]);
}

#[test]
fn the_ring_keeps_the_last_64_kb() {
    let dir = TestDir::new("run-ring");
    // 200 000 bytes of `a`, then a marker: the ring holds the end, capped.
    let (ran, _) = run_plain(spec(
        &dir,
        "head -c 200000 /dev/zero | tr '\\000' a\necho\necho END\n",
    ));
    assert_eq!(ran.ended, Ended::Exited(Some(0)));
    assert_eq!(ran.output.len(), RING_BYTES);
    assert!(ran.output.text().ends_with("aaaa\nEND\n"));
    assert_eq!(
        format!("{:?}", ran.output),
        format!("Ring({RING_BYTES} bytes)")
    );
}

/// Runs a script that prints `started` and then waits on its stdin for good, and ends it with
/// `stop` once `started` is read. Returns how it ended and how many waits were armed.
fn run_until_stopped(tag: &str, stop: impl FnOnce(&ManualTimer, &Cancel)) -> (Ended, Duration) {
    let dir = TestDir::new(tag);
    let mut spec = spec(&dir, "echo started\nexec cat\n");
    spec.limit = Duration::from_secs(600);
    let timer = ManualTimer::new();
    let cancel = Cancel::new();
    // The paste channel stays open, so stdin does too and `cat` never ends.
    let (codes, paste) = async_channel::bounded::<PasteCode>(1);
    let (started, on_started) = async_channel::bounded(1);
    let launcher = launcher();
    let running = run(&launcher, &timer, spec, &cancel, Some(paste), move |line| {
        if line == "started" {
            started.try_send(()).unwrap_or_default();
        }
    });
    let driver = async {
        let armed = timer.next_armed().await;
        on_started.recv().await.unwrap();
        stop(&timer, &cancel);
        armed
    };
    let (ran, armed) = block_on(join(running, driver));
    drop(codes);
    (ran.unwrap().ended, armed)
}

#[test]
fn the_limit_kills_the_run() {
    let (ended, armed) = run_until_stopped("run-limit", |timer, _| assert!(timer.fire()));
    assert_eq!(ended, Ended::TimedOut);
    assert_eq!(armed, Duration::from_secs(600));
}

#[test]
fn cancel_kills_the_run() {
    let (ended, _) = run_until_stopped("run-cancel", |_, cancel| cancel.cancel());
    assert_eq!(ended, Ended::Cancelled);
}

#[test]
fn a_program_that_cannot_start_is_an_error() {
    let dir = TestDir::new("run-missing");
    let mut spec = spec(&dir, "exit 0\n");
    spec.node = Path::new("/nonexistent/clauseo-node").to_path_buf();
    let started = block_on(run(
        &launcher(),
        &ManualTimer::new(),
        spec,
        &Cancel::new(),
        None,
        |_| {},
    ));
    assert!(started.is_err());
}

/// S18 security review, drift: `RunSpec`'s `Debug` names the child's variables and never
/// prints a value, so a proxy password in the environment stays out of any log.
#[test]
fn run_spec_debug_prints_names_never_values() {
    let dir = TestDir::new("run-debug");
    let mut spec = spec(&dir, "exit 0\n");
    spec.env.insert(
        OsString::from("HTTPS_PROXY"),
        OsString::from("http://user:hunter2@proxy.example:3128"),
    );
    let printed = format!("{spec:?}");
    assert!(!printed.contains("hunter2"), "{printed}");
    assert!(printed.contains("HTTPS_PROXY"), "{printed}");
}

/// The lawyer's grace in these tests: any duration the `ManualTimer` reports as armed.
const GRACE: Duration = Duration::from_secs(2);

/// Runs `script` with `on_stop` and `env` added, the paste channel open (so stdin stays open),
/// and once it prints `started` hands the run to `then` with the timer and the cancel. `then`
/// runs beside the run and may drive it on; returns how the run ended.
fn run_with(
    tag: &str,
    script: &str,
    on_stop: OnStop,
    env: &[(&str, &Path)],
    then: impl AsyncFnOnce(&ManualTimer, &Cancel, &async_channel::Sender<PasteCode>),
) -> Ended {
    let dir = TestDir::new(tag);
    let mut spec = spec(&dir, script);
    spec.limit = Duration::from_secs(600);
    spec.on_stop = on_stop;
    for (name, value) in env {
        spec.env
            .insert(OsString::from(name), value.as_os_str().to_owned());
    }
    let timer = ManualTimer::new();
    let cancel = Cancel::new();
    let (codes, paste) = async_channel::bounded::<PasteCode>(1);
    let (started, on_started) = async_channel::bounded(1);
    let launcher = launcher();
    let running = run(&launcher, &timer, spec, &cancel, Some(paste), move |line| {
        if line == "started" {
            started.try_send(()).unwrap_or_default();
        }
    });
    let driver = async {
        // The run's limit, armed first.
        assert_eq!(timer.next_armed().await, Duration::from_secs(600));
        on_started.recv().await.unwrap();
        then(&timer, &cancel, &codes).await;
    };
    let (ran, ()) = block_on(join(running, driver));
    drop(codes);
    ran.unwrap().ended
}

/// A run that is awaited on a graceful stop (a status, a logout) is not killed: it ends by
/// itself, and its exit is its answer.
#[test]
fn a_waited_run_finishes_after_a_graceful_stop() {
    let ended = run_with(
        "run-wait",
        "echo started\nread code\necho \"got:$code\"\nexit 0\n",
        OnStop::status("test status"),
        &[],
        async |timer, cancel, codes| {
            cancel.stop(Stop::Lawyer(GRACE));
            // The grace is armed once the run has seen the stop and chosen to wait.
            assert_eq!(timer.next_armed().await, GRACE);
            codes
                .send(PasteCode::new(zeroize::Zeroizing::new("go".to_owned())))
                .await
                .unwrap();
        },
    );
    assert_eq!(ended, Ended::Exited(Some(0)));
}

/// A waited run still going when the grace passes is killed.
#[test]
fn a_waited_run_is_killed_when_the_grace_passes() {
    let ended = run_with(
        "run-wait-grace",
        "echo started\nexec cat\n",
        OnStop::status("test status"),
        &[],
        async |timer, cancel, _| {
            cancel.stop(Stop::Lawyer(GRACE));
            assert_eq!(timer.next_armed().await, GRACE);
            assert!(timer.fire());
        },
    );
    assert_eq!(ended, Ended::Cancelled);
}

/// A waited run is killed when the stop at once follows the graceful one (the quit's bound).
#[test]
fn a_waited_run_is_killed_by_the_stop_at_once() {
    let ended = run_with(
        "run-wait-kill",
        "echo started\nexec cat\n",
        OnStop::status("test status"),
        &[],
        async |_, cancel, _| {
            cancel.stop(Stop::Quit);
            cancel.cancel();
        },
    );
    assert_eq!(ended, Ended::Cancelled);
}

/// `/cancel` goes only to a listener the run's own tree holds: with the callback port held by
/// another process (this test), nothing connects to it, and the run is killed at the bound.
#[test]
fn no_cancel_goes_to_a_listener_another_process_holds() {
    let other = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    other.set_nonblocking(true).unwrap();
    let port = other.local_addr().unwrap().port();
    let on_stop = OnStop {
        how: Stopping::Cancel { port },
        what: "codex login",
        writes: false,
    };
    let ended = run_with(
        "run-not-ours",
        "echo started\nexec cat\n",
        on_stop,
        &[],
        async |timer, cancel, _| {
            cancel.stop(Stop::Lawyer(GRACE));
            // Armed after the owner check and the send it would have made. Where the check runs
            // `lsof` (macOS), its 500 ms limit is armed first: passed over, never fired.
            let mut armed = timer.next_armed().await;
            while armed != GRACE {
                assert_eq!(
                    armed,
                    Duration::from_millis(500),
                    "only lsof's limit comes first"
                );
                armed = timer.next_armed().await;
            }
            assert!(timer.fire());
        },
    );
    assert_eq!(ended, Ended::Cancelled);
    let accepted = other.accept();
    assert!(
        matches!(&accepted, Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "a connection reached another process's listener: {accepted:?}"
    );
}

/// After a kill of a run that may write Codex's login file, that run's own home's file is
/// removed when it is 0 bytes, and kept when it holds anything; a run that writes nothing, or
/// another home, is never touched.
#[test]
fn a_kill_removes_only_an_empty_login_file_of_the_runs_own_home() {
    let writes = OnStop {
        how: Stopping::Wait,
        what: "codex logout",
        writes: true,
    };
    let cases: [(&str, &[u8], OnStop, bool); 3] = [
        ("run-kill-empty", b"", writes, false),
        ("run-kill-full", b"{\"x\":1}", writes, true),
        (
            "run-kill-no-write",
            b"",
            OnStop::status("test status"),
            true,
        ),
    ];
    for (tag, contents, on_stop, kept) in cases {
        let homes = TestDir::new(tag);
        let home = homes.0.join("home");
        let file = login_file_for_tests(&home);
        write_for_tests(&file, contents);
        let other = login_file_for_tests(&homes.0.join("other"));
        write_for_tests(&other, b"");
        let ended = run_with(
            tag,
            "echo started\nexec cat\n",
            on_stop,
            &[("CODEX_HOME", &home)],
            async |_, cancel, _| cancel.cancel(),
        );
        assert_eq!(ended, Ended::Cancelled);
        assert_eq!(file.exists(), kept, "{tag}");
        assert!(other.exists(), "{tag}: another home's file was touched");
    }
}

/// A run whose job is already stopped never starts.
#[test]
fn a_stopped_job_starts_no_run() {
    let dir = TestDir::new("run-stopped");
    let marker = dir.0.join("ran");
    let spec = spec(&dir, &format!("touch '{}'\n", marker.display()));
    let cancel = Cancel::new();
    cancel.stop(Stop::Quit);
    let ran = block_on(run(
        &launcher(),
        &ManualTimer::new(),
        spec,
        &cancel,
        None,
        |_| {},
    ))
    .unwrap();
    assert_eq!(ran.ended, Ended::Cancelled);
    assert!(!marker.exists());
}
