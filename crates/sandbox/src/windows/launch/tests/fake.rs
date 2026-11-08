//! The fake steps: a world that outlives one set of launchers (the "disk": slot claims, open
//! sessions), a log of every call, a step that fails or waits on demand, and a clock the
//! retries move.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launched, SpawnOptions};

use super::super::{Ending, LaunchSteps, RunRefusal, Slot, Stopped, Walk};
use crate::windows::pool::PoolUser;
use crate::windows::pool::assign::Assign;
use crate::windows::sids::Sid;
use crate::{Error, Policy, SetAside};

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
    slots: Mutex<BTreeSet<u8>>,
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
}

impl World {
    pub(super) fn new() -> Arc<World> {
        Arc::new(World::default())
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

    pub(super) fn claimed_slots(&self) -> BTreeSet<u8> {
        self.slots.lock().unwrap().clone()
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

    fn log(&self, call: impl Into<String>) {
        self.log.lock().unwrap().push(call.into());
    }

    /// Logs `step`, waits at its gate, then fails it if asked to.
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

pub(super) struct FakeSteps(pub(super) Arc<World>);

impl LaunchSteps for FakeSteps {
    type Plan = ();
    type Check = ();
    type RunGroup = u32;
    type Cursor = ();
    type Repaired = ();
    type Passed = Vec<SetAside>;

    fn clear_stale(&self) -> Result<(), Error> {
        self.0.step("clear_stale")?;
        self.0.slots.lock().unwrap().clear();
        Ok(())
    }

    fn setup_ready(&self) -> Result<(), Error> {
        self.0.step("setup_ready")
    }

    fn open_session(&self, matter: &str, session: &str, running: &[&str]) -> Result<(), Error> {
        self.0.step("open_session")?;
        let mut open = self.0.open.lock().unwrap();
        let sessions = open.entry(matter.to_owned()).or_default();
        sessions.retain(|other| running.contains(&other.as_str()));
        sessions.insert(session.to_owned());
        Ok(())
    }

    fn close_session(&self, matter: &str, session: &str) {
        let _never_fails = self.0.step("close_session");
        if let Some(sessions) = self.0.open.lock().unwrap().get_mut(matter) {
            sessions.remove(session);
        }
    }

    fn assign(&self, matter: &str, session: &str) -> Result<PoolUser, Assign> {
        self.0.log("assign");
        if let Some(refusal) = self.0.assign_refuses.lock().unwrap().take() {
            return Err(refusal);
        }
        let busy = self
            .0
            .open_sessions(matter)
            .iter()
            .any(|other| other != session);
        if busy {
            return Err(Assign::Busy(matter.parse().unwrap()));
        }
        Ok(pool_user())
    }

    fn claim_slot(&self, _matter: &str, _pool: &PoolUser) -> Result<Slot, Error> {
        self.0.step("claim_slot")?;
        let mut slots = self.0.slots.lock().unwrap();
        let Some(index) = (0..SLOTS).find(|index| !slots.contains(index)) else {
            return Err(Error::PoolFull {
                cause: crate::windows::pool::PoolFullCause::default(),
            });
        };
        slots.insert(index);
        let base = 46100 + u16::from(index) * 2;
        Ok(Slot {
            index,
            ports: vec![base, base + 1],
        })
    }

    fn release_slot(&self, slot: &Slot) {
        self.0.log("release_slot");
        self.0.slots.lock().unwrap().remove(&slot.index);
    }

    fn plan(&self, _policy: &Policy, _pool: &PoolUser) -> Result<(), Error> {
        self.0.step("plan")
    }

    fn check_launch(&self, (): &()) -> Result<(), Error> {
        self.0.step("check_launch")
    }

    fn check_network(&self, _policy: &Policy, _pool: &PoolUser) -> Result<(), Error> {
        self.0.step("check_network")
    }

    fn pool_user_processes(&self, _pool: &PoolUser) -> Result<Vec<u32>, Error> {
        self.0.step("pool_processes")?;
        Ok((1..=*self.0.leftovers.lock().unwrap()).collect())
    }

    fn stop_pool_user(&self, _pool: &PoolUser) -> Result<Stopped, Error> {
        self.0.step("stop")?;
        let mut leftovers = self.0.leftovers.lock().unwrap();
        if *self.0.stubborn.lock().unwrap() {
            return Ok(Stopped::default());
        }
        let ended = std::mem::take(&mut *leftovers);
        Ok(Stopped { ended })
    }

    fn sweep_pool_user(&self, _pool: &PoolUser) -> Result<(), Error> {
        self.0.step("sweep")
    }

    fn before_launch(
        &self,
        (): &(),
        (): &(),
        _session: &str,
        _running: &[&str],
    ) -> Result<Vec<SetAside>, Error> {
        self.0.step("before_launch")?;
        Ok(vec![moved("CLAUDE.md")])
    }

    fn prepare_run(&self, _slot: &Slot, _pool: &PoolUser) -> Result<u32, RunRefusal> {
        self.0
            .step("prepare_run")
            .map_err(|error| RunRefusal::Failed(error.to_string()))?;
        if *self.0.setup_always_running.lock().unwrap() {
            return Err(RunRefusal::SetUpRunning);
        }
        if let Some(answer) = self.0.run_answers.lock().unwrap().pop_front() {
            return Err(answer);
        }
        let mut groups = self.0.groups.lock().unwrap();
        *groups += 1;
        Ok(*groups)
    }

    fn end_run(&self, group: u32) {
        self.0.log(format!("end_run {group}"));
    }

    fn apply(&self, (): &(), (): &(), walk: &Walk<'_>) -> Result<(), Error> {
        self.0.step("apply")?;
        if walk.cancelled() {
            return Err(Error::Cancelled);
        }
        walk.walked(3);
        Ok(())
    }

    fn write_launch(
        &self,
        _policy: &Policy,
        _pool: &PoolUser,
        _spec: &LaunchSpec,
    ) -> Result<PathBuf, Error> {
        self.0.step("write_launch")?;
        Ok(PathBuf::from("run/0123456789abcdef.json"))
    }

    fn spawn_outer(
        &self,
        _launch: &Path,
        _opts: &SpawnOptions,
    ) -> Result<Launched, clauseo_process::Error> {
        self.0.log("spawn_outer");
        Err(clauseo_process::Error::Closing)
    }

    fn session_cursor(&self, _end: &Ending<'_>) -> Option<()> {
        self.0.log("session_cursor");
        Some(())
    }

    fn repair(&self, _end: &Ending<'_>) -> Result<(), Error> {
        self.0.step("repair")
    }

    fn end_reads(&self, _end: &Ending<'_>) {
        self.0.log("end_reads");
    }

    fn set_aside_after(&self, end: &Ending<'_>) -> Result<Vec<SetAside>, Error> {
        self.0.step("set_aside_after")?;
        if let Some(sessions) = self.0.open.lock().unwrap().get_mut(end.matter) {
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
        self.0.log(format!(
            "write_clean_close cursor={} repaired={} passed={}",
            cursor.is_some(),
            repaired.is_ok(),
            passed.is_ok()
        ));
    }

    fn now(&self) -> Duration {
        self.0.now()
    }

    fn pause(&self, pause: Duration) {
        *self.0.pauses.lock().unwrap() += 1;
        *self.0.clock.lock().unwrap() += pause;
    }
}
