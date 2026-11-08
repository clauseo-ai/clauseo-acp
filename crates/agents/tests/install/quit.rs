//! Quitting during `npm ci` (#386): `npm ci` is replaced by a fake npm, the real Node running a
//! script that starts a child Node, writes both pids and sleeps. `shutdown` kills both within
//! its wait, a short wait returns in time while a job is stuck, and a drop kills both too. Also
//! `install_tries` after a plain install.
#![expect(
    clippy::disallowed_methods,
    reason = "real processes: the tests poll for their pids and their end, and make the pid folder"
)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use clauseo_agents::testing::trace;
use clauseo_agents::{Agents, InstallState};
use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions};
use clauseo_process::{kill, running};

use crate::support::{Events, Fixture, agent_a, agent_b, states_of};

/// The fake npm: starts a child Node that sleeps, writes `child.pid` then `npm.pid` (each by a
/// rename, so a pid file is never read half written) in the folder its first argument names,
/// then sleeps itself.
const FAKE_NPM: &str = "\
const fs = require('fs'), path = require('path'), cp = require('child_process');
const dir = process.argv[1];
const put = (name, pid) => {
  const file = path.join(dir, name);
  fs.writeFileSync(file + '.tmp', String(pid));
  fs.renameSync(file + '.tmp', file);
};
const child = cp.spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], {
  stdio: 'ignore',
});
put('child.pid', child.pid);
put('npm.pid', process.pid);
setInterval(() => {}, 1000);
";

/// How long a pid file, or a process's end, may take.
const DEADLINE: Duration = Duration::from_secs(30);
/// `shutdown`'s wait in the tests that expect every job to end.
const WAIT: Duration = Duration::from_secs(10);
/// What a bounded wait may overrun by: one poll and the scheduler.
const SLACK: Duration = Duration::from_secs(1);

/// The passthrough launcher with `npm ci` replaced by [`FAKE_NPM`]; each `npm ci` launch first
/// waits until `gate` is open.
struct FakeNpm {
    inner: PassthroughLauncher,
    pids: PathBuf,
    gate: Arc<Gate>,
}

/// Open unless a test closed it; a closed gate holds `npm ci`'s launch until it opens.
#[derive(Default)]
struct Gate {
    closed: Mutex<bool>,
    changed: std::sync::Condvar,
}

impl Gate {
    fn set_closed(&self, closed: bool) {
        *self.closed.lock().unwrap_or_else(PoisonError::into_inner) = closed;
        self.changed.notify_all();
    }

    fn pass(&self) {
        let mut closed = self.closed.lock().unwrap_or_else(PoisonError::into_inner);
        while *closed {
            closed = self
                .changed
                .wait(closed)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

impl Launcher for FakeNpm {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        if !spec.args().iter().any(|arg| arg == "ci") {
            return self.inner.launch(spec);
        }
        self.gate.pass();
        trace(format_args!("launch: the fake npm in place of npm ci"));
        let args: Vec<OsString> = vec![
            "--single-threaded".into(),
            "-e".into(),
            FAKE_NPM.into(),
            self.pids.as_os_str().to_owned(),
        ];
        let env: BTreeMap<OsString, OsString> = spec.env().clone();
        let fake = LaunchSpec::new(spec.program().to_owned(), args, env, spec.cwd().to_owned())?;
        self.inner.launch(&fake)
    }
}

/// A started `Agents` whose `npm ci` is the fake, its events, the pid folder and the gate.
fn start(fx: &Fixture) -> (Agents, Events, PathBuf, Arc<Gate>) {
    let pids = fx.data.path().join("fake-npm");
    std::fs::create_dir_all(&pids).unwrap();
    let gate = Arc::new(Gate::default());
    let launcher: Arc<dyn Launcher> = Arc::new(FakeNpm {
        inner: PassthroughLauncher::new(SpawnOptions::new()),
        pids: pids.clone(),
        gate: Arc::clone(&gate),
    });
    let (agents, events) = Agents::start(fx.config.clone(), launcher, fx.timer.clone());
    let events = Events(events);
    events.first_states();
    (agents, events, pids, gate)
}

/// The pid in `dir/name`, once it is there.
fn read_pid(dir: &Path, name: &str) -> u32 {
    let start = Instant::now();
    loop {
        if let Ok(text) = std::fs::read_to_string(dir.join(name)) {
            return text.trim().parse().expect("a pid");
        }
        assert!(start.elapsed() < DEADLINE, "no {name} in time");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The fake npm's pid and its child's, once both run.
fn fake_pids(dir: &Path) -> [u32; 2] {
    let npm = read_pid(dir, "npm.pid");
    [npm, read_pid(dir, "child.pid")]
}

/// Whether every one of `pids` stops running within [`DEADLINE`].
fn all_end(pids: [u32; 2]) -> bool {
    let start = Instant::now();
    while start.elapsed() < DEADLINE {
        if pids.iter().all(|pid| !running(*pid)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Kills whatever of `pids` still runs (after a failed check), then says which ran.
fn still_running(pids: [u32; 2]) -> Vec<u32> {
    let alive: Vec<u32> = pids.into_iter().filter(|pid| running(*pid)).collect();
    for pid in &alive {
        kill(*pid);
    }
    alive
}

/// Starts agent A's install and waits for its `Installing`, then for the fake npm's pids.
fn install_until_npm(agents: &Agents, events: &Events, pids: &Path) -> [u32; 2] {
    agents.install(&agent_a());
    events.until(&agent_a(), |state| *state == InstallState::Installing);
    fake_pids(pids)
}

#[test]
fn shutdown_kills_a_sleeping_npm_and_its_child() {
    let fx = Fixture::new("quit-shutdown");
    let (agents, events, pids, _gate) = start(&fx);
    let running = install_until_npm(&agents, &events, &pids);
    let start = Instant::now();
    let ended = agents.shutdown(WAIT);
    let took = start.elapsed();
    // Every SIGKILL went before `shutdown` returned; the kernel ends the processes just after.
    let gone = all_end(running);
    let alive = still_running(running);
    assert!(ended, "shutdown says a job is still running");
    assert!(gone, "still running after shutdown: {alive:?}");
    assert!(took < WAIT + SLACK, "shutdown took {took:?}");
}

#[test]
fn a_short_shutdown_returns_within_its_wait() {
    let fx = Fixture::new("quit-bounded");
    let (agents, events, pids, gate) = start(&fx);
    gate.set_closed(true);
    agents.install(&agent_a());
    events.until(&agent_a(), |state| *state == InstallState::Installing);
    // The job is now stuck in npm's launch, where no cancel reaches it.
    let wait = Duration::from_millis(200);
    let start = Instant::now();
    let ended = agents.shutdown(wait);
    let took = start.elapsed();
    gate.set_closed(false);
    assert!(!ended, "shutdown says a stuck job ended");
    assert!(
        took >= wait && took < wait + SLACK,
        "shutdown took {took:?}"
    );
    // Let go, the job starts the fake npm, sees the cancel and kills it.
    assert!(agents.shutdown(WAIT), "the job did not end once let go");
    if let Ok(npm) = std::fs::read_to_string(pids.join("npm.pid")) {
        let pid: u32 = npm.trim().parse().unwrap();
        let both = [pid, read_pid(&pids, "child.pid")];
        let gone = all_end(both);
        let alive = still_running(both);
        assert!(gone, "still running after shutdown: {alive:?}");
    }
}

#[test]
fn dropping_agents_kills_a_sleeping_npm_and_its_child() {
    let fx = Fixture::new("quit-drop");
    let (agents, events, pids, _gate) = start(&fx);
    let running = install_until_npm(&agents, &events, &pids);
    drop(agents);
    let ended = all_end(running);
    let alive = still_running(running);
    assert!(ended, "still running after the drop: {alive:?}");
}

#[test]
fn no_job_starts_after_shutdown() {
    let mut fx = Fixture::new("quit-closed");
    // B is IT's: its `install` answers `Managed` at once, so the test knows the worker has
    // handled every command sent before it.
    fx.config.managed.insert(agent_b());
    let (agents, events, _pids, _gate) = start(&fx);
    assert!(agents.shutdown(WAIT));
    agents.install(&agent_a());
    agents.install(&agent_b());
    let seen = events.until(&agent_b(), |state| *state == InstallState::Managed);
    assert!(agents.shutdown(WAIT));
    assert_eq!(states_of(&seen, &agent_a()), Vec::new(), "{seen:?}");
}

#[test]
fn install_tries_counts_the_successful_try() {
    let fx = Fixture::new("quit-tries");
    let (agents, events) = fx.start();
    events.first_states();
    assert_eq!(agents.install_tries(&agent_a()), None);
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert_eq!(agents.install_tries(&agent_a()), Some(1));
}
