//! A stetdoc run on the real clock, through the real launcher: one that never answers is
//! killed at its limit with nothing of it left running, and a run is given this process's
//! environment and a stdin that ends at once (no console to wait on, Windows' session 0
//! included).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use clauseo_process::{
    AsyncIoTimer, Error, LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions,
};
use clauseo_profile::skills::stetdoc::{RunFailure, RunOptions, run_for, version_within};
use futures::executor::block_on;

use super::stetdoc_start::{slow, stetdoc_in};
use super::{Scratch, stub};

/// How long a test waits for a killed run's process to be gone.
const GONE_WITHIN: Duration = Duration::from_secs(10);

/// The real launcher, keeping the pid of each run it starts.
struct Pids {
    inner: PassthroughLauncher,
    pids: Mutex<Vec<u32>>,
}

impl Pids {
    fn new() -> Self {
        Self {
            inner: PassthroughLauncher::new(SpawnOptions::new()),
            pids: Mutex::new(Vec::new()),
        }
    }

    fn pids(&self) -> Vec<u32> {
        self.pids.lock().unwrap().clone()
    }
}

impl Launcher for Pids {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, Error> {
        let launched = self.inner.launch(spec)?;
        self.pids.lock().unwrap().push(launched.handle.pid());
        Ok(launched)
    }
}

/// Whether `pid` is gone within [`GONE_WITHIN`]; kills it when it is not, so a failed test
/// leaves nothing behind.
fn gone(pid: u32) -> bool {
    let until = Instant::now() + GONE_WITHIN;
    while Instant::now() < until {
        if !clauseo_process::running(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    clauseo_process::kill(pid);
    false
}

#[test]
fn a_version_run_that_never_answers_is_killed_at_its_limit_and_nothing_is_left() {
    let root = Scratch::new("stetdoc-kill");
    let (program, _cached) = stetdoc_in(&root, "stetdoc 0.9.3");
    slow(&root);
    let launcher = Pids::new();
    let limit = Duration::from_millis(500);
    let started = Instant::now();
    let ran = block_on(version_within(&launcher, &AsyncIoTimer, &program, limit));
    let took = started.elapsed();
    assert!(
        matches!(
            ran,
            Err(clauseo_profile::skills::stetdoc::RunError::TimedOut(_))
        ),
        "{ran:?}"
    );
    assert!(took < limit + Duration::from_secs(5), "took {took:?}");
    let pids = launcher.pids();
    assert_eq!(pids.len(), 1);
    assert!(gone(pids[0]), "the timed-out run is still running");
}

#[test]
fn a_run_gets_the_parent_environment_and_a_stdin_that_ends_at_once() {
    let root = Scratch::new("stetdoc-env");
    let bin = root.join("bin");
    super::write::fresh_dir(&bin).unwrap();
    let program = stub(&bin, "stetdoc 0.9.3", "");
    let launcher = Pids::new();
    let options = RunOptions::new(Duration::from_secs(20));
    let ran = block_on(run_for(
        &launcher,
        &AsyncIoTimer,
        &program,
        &["stub-env".into()],
        &options,
    ));
    let ran = match ran {
        Ok(ran) => ran,
        Err(RunFailure::TimedOut) => panic!("the run waited on its stdin or hung"),
        Err(other) => panic!("{other}"),
    };
    assert_eq!(ran.code, Some(0), "{}", ran.stderr_tail);
    let printed = String::from_utf8(ran.stdout).unwrap();
    assert!(printed.lines().any(|line| line == "stdin eof"), "{printed}");
    // Every variable a Windows program may need that this process has, the run has too: the
    // launcher does not clear the environment. `PATH` is there on every OS.
    let mut essentials = vec!["PATH"];
    if cfg!(windows) {
        essentials.extend([
            "SystemRoot",
            "windir",
            "TEMP",
            "TMP",
            "USERPROFILE",
            "LOCALAPPDATA",
            "PATHEXT",
            "ComSpec",
        ]);
    }
    for name in essentials {
        if std::env::var_os(name).is_some() {
            assert!(
                printed.lines().any(|line| line == format!("var {name}")),
                "{name} is missing: {printed}"
            );
        }
    }
    let pids = launcher.pids();
    assert_eq!(pids.len(), 1);
    assert!(gone(pids[0]), "the run is still running");
}
