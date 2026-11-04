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

use super::{Ended, RING_BYTES, Ran, RunSpec, run};
use crate::cancel::Cancel;
use crate::signin::PasteCode;
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
