//! The fake steps: a world that outlives one set of launchers (the "disk": slot claims, open
//! sessions, the data folder), a log of every call, a step that fails, panics, waits or takes
//! time on demand, and a clock the retries and slow steps move. The pool user's processes are a
//! count: leftovers, or the outer's tree, which [`FakeSteps::spawn_outer`] starts as a real
//! process that exits at once (`spawn_fails` makes it fail instead).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launched, SpawnOptions};

use super::super::{Ending, LaunchSteps, RunRefusal, Slot, Stopped, Walk};
use super::pipe::{FakePipe, Outer, PIPE, Spawned, host_var, install, quick_outer};
use crate::test_support::TempDir;
use crate::windows::pool::PoolUser;
use crate::windows::pool::assign::Assign;
use crate::windows::sids::Sid;
use crate::{Error, Policy, SetAside};

/// Numbers each world's data folder.
static WORLDS: AtomicU64 = AtomicU64::new(1);

/// The fake's slots.
pub(super) const SLOTS: u8 = 2;
/// The matters the tests use: `MatterId` text forms.
pub(super) const MATTER: &str = "01890a5d-ac96-774b-8cce-b302099a8001";
pub(super) const OTHER: &str = "01890a5d-ac96-774b-8cce-b302099a8002";

/// A step that waits once reached, until the test lets it go on.
struct Gate {
    step: &'static str,
    reached: Sender<()>,
    go: Receiver<()>,
}

/// The test's side of a [`Gate`].
pub(super) struct Held {
    reached: Receiver<()>,
    go: Sender<()>,
}

impl Held {
    /// Waits until the step is reached.
    pub(super) fn reached(&self) {
        self.reached
            .recv_timeout(Duration::from_secs(10))
            .expect("the step wasn't reached");
    }

    /// Lets it go on.
    pub(super) fn go(&self) {
        self.go.send(()).unwrap();
    }
}

#[derive(Default)]
pub(super) struct World {
    log: Mutex<Vec<String>>,
    logged: std::sync::Condvar,
    /// The slot records: each slot's number and the claim it names.
    slots: Mutex<BTreeMap<u8, u64>>,
    open: Mutex<BTreeMap<String, BTreeSet<String>>>,
    fail_at: Mutex<Option<&'static str>>,
    assign_refuses: Mutex<Option<Assign>>,
    run_answers: Mutex<VecDeque<RunRefusal>>,
    setup_always_running: Mutex<bool>,
    gate: Mutex<Option<Gate>>,
    clock: Mutex<Duration>,
    pauses: Mutex<u32>,
    groups: Mutex<u32>,
    leftovers: Mutex<u32>,
    stubborn: Mutex<bool>,
    sweeper: Mutex<bool>,
    panic_at: Mutex<Option<&'static str>>,
    slow: Mutex<Option<(&'static str, Duration)>>,
    apply_ignores_cancel: Mutex<bool>,
    progressing: Mutex<Option<(&'static str, Duration)>>,
    end_run_fails: Mutex<u32>,
    dir: OnceLock<TempDir>,
    /// The outer's side of the world (`pipe.rs`).
    pub(super) outer: Outer,
}

impl World {
    pub(super) fn new() -> Arc<World> {
        let world = World::default();
        let number = WORLDS.fetch_add(1, Ordering::Relaxed);
        let _new = world
            .dir
            .set(TempDir::new(&format!("launch-world-{number}")));
        Arc::new(world)
    }

    /// The data folder its launchers lock.
    pub(super) fn dir(&self) -> &Path {
        self.dir.get().map_or(Path::new(""), TempDir::path)
    }

    /// Makes `step` panic once reached.
    pub(super) fn panic_at(&self, step: &'static str) {
        *self.panic_at.lock().unwrap() = Some(step);
    }

    /// Makes `step` take `time` on the clock.
    pub(super) fn slow(&self, step: &'static str, time: Duration) {
        *self.slow.lock().unwrap() = Some((step, time));
    }

    /// Makes the work step `step` take `time` on the clock, counting an item every 10 s.
    pub(super) fn progressing(&self, step: &'static str, time: Duration) {
        *self.progressing.lock().unwrap() = Some((step, time));
    }

    /// Moves the clock on by `time`, as a step that hangs would let it.
    pub(super) fn advance(&self, time: Duration) {
        *self.clock.lock().unwrap() += time;
    }

    /// Makes the next `count` `EndRun`s fail.
    pub(super) fn end_run_fails(&self, count: u32) {
        *self.end_run_fails.lock().unwrap() = count;
    }

    /// The claim slot `index`'s record names, if it is claimed.
    pub(super) fn slot_claim(&self, index: u8) -> Option<u64> {
        self.slots.lock().unwrap().get(&index).copied()
    }

    /// `apply` finishes its walk even once cancelled, as one past its last folder would.
    pub(super) fn apply_ignores_cancel(&self) {
        *self.apply_ignores_cancel.lock().unwrap() = true;
    }

    /// Every call so far, taken.
    pub(super) fn calls(&self) -> Vec<String> {
        std::mem::take(&mut *self.log.lock().unwrap())
    }

    pub(super) fn fail_at(&self, step: &'static str) {
        *self.fail_at.lock().unwrap() = Some(step);
    }

    pub(super) fn assign_refuses(&self, refusal: Assign) {
        *self.assign_refuses.lock().unwrap() = Some(refusal);
    }

    /// `PrepareRun` answers these, in order, then makes the group.
    pub(super) fn run_answers(&self, answers: impl IntoIterator<Item = RunRefusal>) {
        self.run_answers.lock().unwrap().extend(answers);
    }

    pub(super) fn setup_always_running(&self) {
        *self.setup_always_running.lock().unwrap() = true;
    }

    /// Makes `step` wait once reached.
    pub(super) fn hold(&self, step: &'static str) -> Held {
        let (reached_tx, reached) = channel();
        let (go, go_rx) = channel();
        *self.gate.lock().unwrap() = Some(Gate {
            step,
            reached: reached_tx,
            go: go_rx,
        });
        Held { reached, go }
    }

    /// The pool user has `count` processes running; `stubborn` ones survive the stop.
    pub(super) fn leftovers(&self, count: u32, stubborn: bool) {
        *self.leftovers.lock().unwrap() = count;
        *self.stubborn.lock().unwrap() = stubborn;
    }

    /// The end sweeper an outer started runs as the pool user, or has finished.
    pub(super) fn sweeper(&self, running: bool) {
        *self.sweeper.lock().unwrap() = running;
    }

    pub(super) fn claimed_slots(&self) -> BTreeSet<u8> {
        self.slots.lock().unwrap().keys().copied().collect()
    }

    pub(super) fn open_sessions(&self, matter: &str) -> BTreeSet<String> {
        self.open
            .lock()
            .unwrap()
            .get(matter)
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn pauses(&self) -> u32 {
        *self.pauses.lock().unwrap()
    }

    pub(super) fn now(&self) -> Duration {
        *self.clock.lock().unwrap()
    }

    pub(super) fn log(&self, call: impl Into<String>) {
        self.log.lock().unwrap().push(call.into());
        self.logged.notify_all();
    }

    /// Waits until `call` is in the log (not taken), from any thread.
    pub(super) fn wait_for(&self, call: &str) {
        let log = self.log.lock().unwrap();
        let (_log, waited) = self
            .logged
            .wait_timeout_while(log, Duration::from_secs(10), |log| {
                !log.iter().any(|each| each == call)
            })
            .unwrap();
        assert!(!waited.timed_out(), "{call} never came");
    }

    /// A work step: [`World::step`], then the time it takes, its items counted.
    fn work(&self, step: &'static str, walk: &Walk<'_>) -> Result<(), Error> {
        self.step(step)?;
        let progressing = *self.progressing.lock().unwrap();
        if let Some((_, time)) = progressing.filter(|(each, _)| *each == step) {
            let mut done = Duration::ZERO;
            while done < time {
                *self.clock.lock().unwrap() += Duration::from_secs(10);
                done += Duration::from_secs(10);
                walk.advance(1);
            }
        }
        Ok(())
    }

    /// Logs `step`, waits at its gate, takes its time, panics or fails it if asked to.
    fn step(&self, step: &'static str) -> Result<(), Error> {
        self.log(step);
        let gate = {
            let mut gate = self.gate.lock().unwrap();
            if gate.as_ref().is_some_and(|gate| gate.step == step) {
                gate.take()
            } else {
                None
            }
        };
        if let Some(gate) = gate {
            gate.reached.send(()).unwrap();
            gate.go.recv().unwrap();
        }
        let slow = *self.slow.lock().unwrap();
        if let Some((_, time)) = slow.filter(|(slow, _)| *slow == step) {
            *self.clock.lock().unwrap() += time;
        }
        let panics = *self.panic_at.lock().unwrap() == Some(step);
        assert!(!panics, "fake {step} panicked");
        if *self.fail_at.lock().unwrap() == Some(step) {
            return Err(Error::Policy(format!("fake {step} failed")));
        }
        Ok(())
    }
}

/// The pool user the fake assigns.
pub(super) fn pool_user() -> PoolUser {
    PoolUser {
        name: "ClauseoSb01".to_owned(),
        sid: Sid::parse("S-1-5-21-1111111111-2222222222-3333333333-1001").unwrap(),
    }
}

/// What the launch pass and the after-session pass move.
pub(super) fn moved(at: &str) -> SetAside {
    SetAside {
        from: PathBuf::from(at),
        to: PathBuf::from(format!("set-aside/{at}")),
    }
}

pub(super) struct FakeSteps {
    world: Arc<World>,
    dir: PathBuf,
}

impl FakeSteps {
    /// Steps over `world` and its data folder.
    pub(super) fn new(world: &Arc<World>) -> FakeSteps {
        FakeSteps::elsewhere(world, world.dir())
    }

    /// Steps over `world`'s records with another data folder, `dir`: launchers that don't
    /// share the folder's lock.
    pub(super) fn elsewhere(world: &Arc<World>, dir: &Path) -> FakeSteps {
        FakeSteps {
            world: Arc::clone(world),
            dir: dir.to_path_buf(),
        }
    }
}

impl LaunchSteps for FakeSteps {
    type Plan = ();
    type Check = ();
    type RunGroup = u32;
    type Cursor = ();
    type Repaired = ();
    type Passed = Vec<SetAside>;
    type Pipe = FakePipe;

    fn data_dir(&self) -> &Path {
        &self.dir
    }

    fn clear_stale(&self) -> Result<(), Error> {
        self.world.step("clear_stale")?;
        self.world.outer.files.lock().unwrap().clear();
        self.world.slots.lock().unwrap().clear();
        Ok(())
    }

    fn setup_ready(&self) -> Result<(), Error> {
        self.world.step("setup_ready")
    }

    fn open_session(&self, matter: &str, session: &str, running: &[&str]) -> Result<(), Error> {
        self.world.step("open_session")?;
        let mut open = self.world.open.lock().unwrap();
        let sessions = open.entry(matter.to_owned()).or_default();
        sessions.retain(|other| running.contains(&other.as_str()));
        sessions.insert(session.to_owned());
        Ok(())
    }

    fn close_session(&self, matter: &str, session: &str) {
        let _never_fails = self.world.step("close_session");
        if let Some(sessions) = self.world.open.lock().unwrap().get_mut(matter) {
            sessions.remove(session);
        }
    }

    fn assign(&self, matter: &str, session: &str) -> Result<PoolUser, Assign> {
        self.world.log("assign");
        if let Some(refusal) = self.world.assign_refuses.lock().unwrap().take() {
            return Err(refusal);
        }
        let busy = self
            .world
            .open_sessions(matter)
            .iter()
            .any(|other| other != session);
        if busy {
            return Err(Assign::Busy(matter.parse().unwrap()));
        }
        Ok(pool_user())
    }

    fn end_sweeper_running(&self, _pool: &PoolUser) -> bool {
        self.world.log("end_sweeper");
        *self.world.sweeper.lock().unwrap()
    }

    fn claim_slot(&self, _matter: &str, _pool: &PoolUser, claim: u64) -> Result<Slot, Error> {
        self.world.step("claim_slot")?;
        let mut slots = self.world.slots.lock().unwrap();
        let Some(index) = (0..SLOTS).find(|index| !slots.contains_key(index)) else {
            return Err(Error::PoolFull {
                cause: crate::windows::pool::PoolFullCause::default(),
            });
        };
        slots.insert(index, claim);
        let base = 46100 + u16::from(index) * 2;
        Ok(Slot {
            index,
            ports: vec![base, base + 1],
            claim,
        })
    }

    fn release_slot(&self, slot: &Slot) {
        self.world.log("release_slot");
        let mut slots = self.world.slots.lock().unwrap();
        if slots.get(&slot.index) == Some(&slot.claim) {
            slots.remove(&slot.index);
        }
    }

    fn plan(&self, _policy: &Policy, _pool: &PoolUser, walk: &Walk<'_>) -> Result<(), Error> {
        self.world.work("plan", walk)
    }

    fn check_launch(&self, (): &(), walk: &Walk<'_>) -> Result<(), Error> {
        self.world.work("check_launch", walk)
    }

    fn check_network(&self, _policy: &Policy, _pool: &PoolUser) -> Result<(), Error> {
        self.world.step("check_network")
    }

    fn pool_user_processes(&self, _pool: &PoolUser) -> Result<Vec<u32>, Error> {
        self.world.step("pool_processes")?;
        // The end sweeper, pid 9000 here, is known exactly and never listed.
        let sweeper = (*self.world.sweeper.lock().unwrap()).then_some(9000);
        let found = (1..=*self.world.leftovers.lock().unwrap()).chain(sweeper);
        Ok(found.filter(|&pid| Some(pid) != sweeper).collect())
    }

    fn stop_pool_user(&self, _pool: &PoolUser) -> Result<Stopped, Error> {
        self.world.step("stop")?;
        let mut leftovers = self.world.leftovers.lock().unwrap();
        if *self.world.stubborn.lock().unwrap() {
            return Ok(Stopped::default());
        }
        let ended = std::mem::take(&mut *leftovers);
        Ok(Stopped { ended })
    }

    fn sweep_pool_user(&self, _pool: &PoolUser) -> Result<(), Error> {
        self.world.step("sweep")
    }

    fn before_launch(
        &self,
        (): &(),
        (): &(),
        _session: &str,
        _running: &[&str],
        walk: &Walk<'_>,
    ) -> Result<Vec<SetAside>, Error> {
        self.world.work("before_launch", walk)?;
        Ok(vec![moved("CLAUDE.md")])
    }

    fn prepare_run(&self, _slot: &Slot, _pool: &PoolUser) -> Result<u32, RunRefusal> {
        self.world
            .step("prepare_run")
            .map_err(|error| RunRefusal::Failed(error.to_string()))?;
        if *self.world.setup_always_running.lock().unwrap() {
            return Err(RunRefusal::SetUpRunning);
        }
        if let Some(answer) = self.world.run_answers.lock().unwrap().pop_front() {
            return Err(answer);
        }
        let mut groups = self.world.groups.lock().unwrap();
        *groups += 1;
        Ok(*groups)
    }

    fn end_run(&self, group: &u32) -> Result<(), Error> {
        self.world.log(format!("end_run {group}"));
        let mut fails = self.world.end_run_fails.lock().unwrap();
        if *fails > 0 {
            *fails -= 1;
            return Err(Error::Policy("fake end_run failed".to_owned()));
        }
        Ok(())
    }

    fn apply(&self, (): &(), (): &(), walk: &Walk<'_>) -> Result<(), Error> {
        self.world.work("apply", walk)?;
        if walk.cancelled() && !*self.world.apply_ignores_cancel.lock().unwrap() {
            return Err(Error::Cancelled);
        }
        walk.walked(3);
        Ok(())
    }

    fn create_pipe(&self) -> Result<(FakePipe, String), Error> {
        self.world.step("create_pipe")?;
        let pipe = FakePipe {
            world: Some(Arc::clone(&self.world)),
            ..self.world.outer.pipe()
        };
        Ok((pipe, PIPE.to_owned()))
    }

    fn write_launch(
        &self,
        _policy: &Policy,
        _pool: &PoolUser,
        _spec: &LaunchSpec,
        denial_pipe: &str,
    ) -> Result<PathBuf, Error> {
        self.world.step("write_launch")?;
        *self.world.outer.denial_pipe.lock().unwrap() = Some(denial_pipe.to_owned());
        let file = PathBuf::from("run/0123456789abcdef.json");
        self.world.outer.files.lock().unwrap().insert(file.clone());
        Ok(file)
    }

    fn remove_launch(&self, launch: &Path) -> Result<(), Error> {
        self.world.step("remove_launch")?;
        self.world.outer.files.lock().unwrap().remove(launch);
        Ok(())
    }

    fn runner(&self) -> PathBuf {
        install().join("clauseo-sandbox-run.exe")
    }

    fn host_var(&self, name: &str) -> Option<OsString> {
        host_var(name)
    }

    /// Records what it was given, then fails, or with [`World::spawn_starts`] starts a real
    /// process that exits at once and "starts" the outer's two processes.
    fn spawn_outer(
        &self,
        spec: &LaunchSpec,
        opts: &SpawnOptions,
    ) -> Result<Launched, clauseo_process::Error> {
        self.world
            .step("spawn_outer")
            .map_err(|_failed| clauseo_process::Error::Closing)?;
        *self.world.outer.spawned.lock().unwrap() = Some(Spawned::of(spec, opts));
        if !*self.world.outer.spawn_starts.lock().unwrap() {
            return Err(clauseo_process::Error::Closing);
        }
        *self.world.leftovers.lock().unwrap() = 2;
        quick_outer()
    }

    /// Logs the processes running when it starts: they exit within the wait, unless they
    /// linger, which costs the whole wait.
    fn wait_exited(&self, _pool: &PoolUser, within: Duration) -> Result<Vec<u32>, Error> {
        let mut leftovers = self.world.leftovers.lock().unwrap();
        self.world.log(format!("wait_exited {}", *leftovers));
        if *self.world.fail_at.lock().unwrap() == Some("wait_exited") {
            return Err(Error::Policy("fake wait_exited failed".to_owned()));
        }
        let mut lingering = self.world.outer.lingering.lock().unwrap();
        if *lingering > 0 && *leftovers > 0 {
            *lingering -= 1;
            *self.world.clock.lock().unwrap() += within;
            return Ok((1..=*leftovers).collect());
        }
        *leftovers = 0;
        Ok(Vec::new())
    }

    fn end_outer(&self, _pool: &PoolUser) -> Result<(), Error> {
        self.world.step("end_outer")
    }

    fn session_cursor(&self, _end: &Ending<'_>) -> Option<()> {
        self.world.log("session_cursor");
        Some(())
    }

    fn repair(&self, end: &Ending<'_>) -> Result<(), Error> {
        self.world.work("repair", end.walk)?;
        if end.walk.cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(())
    }

    fn end_reads(&self, _end: &Ending<'_>) {
        self.world.log("end_reads");
    }

    fn set_aside_after(&self, end: &Ending<'_>) -> Result<Vec<SetAside>, Error> {
        self.world.work("set_aside_after", end.walk)?;
        if end.walk.cancelled() {
            return Err(Error::Cancelled);
        }
        if let Some(sessions) = self.world.open.lock().unwrap().get_mut(end.matter) {
            sessions.remove(end.session);
        }
        Ok(vec![moved("AGENTS.md")])
    }

    fn moved(&self, passed: &Vec<SetAside>) -> Vec<SetAside> {
        passed.clone()
    }

    fn write_clean_close(
        &self,
        _end: &Ending<'_>,
        cursor: Option<&()>,
        repaired: &Result<(), Error>,
        passed: &Result<Vec<SetAside>, Error>,
    ) {
        self.world.log(format!(
            "write_clean_close cursor={} repaired={} passed={}",
            cursor.is_some(),
            repaired.is_ok(),
            passed.is_ok()
        ));
    }

    fn now(&self) -> Duration {
        self.world.now()
    }

    fn pause(&self, pause: Duration) {
        *self.world.pauses.lock().unwrap() += 1;
        *self.world.clock.lock().unwrap() += pause;
    }
}
