//! Every OS effect of tier B's launcher, behind one trait, so the order of [`Launchers`]
//! (`launch.rs`) is tested on every OS with fakes. The real steps (`OsSteps`, C5) call the
//! ledger, `MatterSids`, `set_aside`, `clean_close`, `apply`, `wfp`, the set-up server and the
//! runner's launch file.
//!
//! [`Launchers`]: super::Launchers

use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launched, SpawnOptions};

use super::prepare::Walk;
use crate::windows::pool::PoolUser;
use crate::windows::pool::assign::Assign;
use crate::{Error, Policy, SetAside};

/// A network slot: its number and the host ports (`H_s`) it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    /// The slot's number.
    pub index: u8,
    /// Its host ports.
    pub ports: Vec<u16>,
}

/// Why the SYSTEM side's `PrepareRun` didn't make the run's group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunRefusal {
    /// A set-up is running and holds the set-up lock: retried within
    /// [`PREPARE_BOUND`](super::prepare::PREPARE_BOUND).
    SetUpRunning,
    /// A task couldn't be hardened, or one that runs as the lawyer changed (`loose_task`):
    /// [`Blocked::LooseTask`](crate::Blocked::LooseTask).
    LooseTask,
    /// Anything else, in words with no path.
    Failed(String),
}

/// What the stop task reported for a pool user (`setup::stop`): how many processes it ended.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stopped {
    /// Processes ended.
    pub ended: u32,
}

/// The session whose end [`LaunchSteps`]' after-session calls handle.
#[derive(Debug, Clone, Copy)]
pub struct Ending<'a> {
    /// The matter.
    pub matter: &'a str,
    /// The session.
    pub session: &'a str,
    /// The policy it ran under.
    pub policy: &'a Policy,
    /// The matter's account.
    pub pool: &'a PoolUser,
}

/// Every OS effect of tier B's launcher. Each call does one step of 0049's PR6 order; the order
/// itself, the undo and the session registry are [`Launchers`](super::Launchers)'.
///
/// **The reserve is bookkeeping only:** [`open_session`](Self::open_session),
/// [`assign`](Self::assign) and [`claim_slot`](Self::claim_slot) write records the sandbox
/// folder holds; none runs anything as the pool user or grants it anything. Grants
/// ([`prepare_run`](Self::prepare_run), [`apply`](Self::apply)) and runs
/// ([`spawn_outer`](Self::spawn_outer)) come only after [`check_launch`](Self::check_launch)
/// passed; so do the stop and the sweep of a leftover's pool user
/// ([`stop_pool_user`](Self::stop_pool_user), [`sweep_pool_user`](Self::sweep_pool_user)).
///
/// **The launching session** is its session id together with its reservation's claim, never
/// its name or a pid alone. C3's own checks (the registry's) use both. TODO(C5): where the real
/// steps must not count the launching session (`open_session`'s mark, then `check_launch` and
/// `before_launch` reading the open sessions), they know it by both too: the session marked by
/// this reservation's `open_session`, not any session of that name.
pub trait LaunchSteps: Send + Sync + 'static {
    /// The admitted plan (`admit` and `acl_plan::plan`: `AdmittedPlan`).
    type Plan: Send;
    /// The launch check's answer (`clean_close::LaunchCheck`).
    type Check: Send;
    /// The run's group (`ClauseoRun-<hex>`), which `EndRun` deletes.
    type RunGroup: Send + 'static;
    /// The journal's cursor at the session's end (`clean_close::Cursor`).
    type Cursor;
    /// What `repair_after_session` returned (`Walked`).
    type Repaired;
    /// What `set_aside::after_session` returned (`Checked`).
    type Passed;

    /// At [`Launchers::with_steps`](super::Launchers::with_steps), before any reservation:
    /// clears what a crashed app left (the stale launch files, `<16 hex>.json`; slot claims and
    /// session marks no live launchers hold), so no crash strands a slot or the matter.
    fn clear_stale(&self) -> Result<(), Error>;

    /// Tier B is set up and ready (`setup_state`).
    fn setup_ready(&self) -> Result<(), Error>;
    /// Marks `session` open in the matter's set-aside record, dropping the open sessions not in
    /// `running` (they crashed), with no scan: what `assign` needs before a new pick.
    fn open_session(&self, matter: &str, session: &str, running: &[&str]) -> Result<(), Error>;
    /// Takes `session` out of the open sessions again, with no pass: nothing ran in it.
    fn close_session(&self, matter: &str, session: &str);
    /// The matter's account (`MatterSids::assign`, over the ledger it loads).
    fn assign(&self, matter: &str, session: &str) -> Result<PoolUser, Assign>;
    /// Whether the detached end sweeper an outer started at a session's end (`--sweep`, as
    /// the pool user) is still running for `pool`, from any session, this app's or a crashed
    /// one's: `reserve` refuses with `MatterBusy` until it has finished, so a pool user is
    /// never handed out alongside it. TODO(C5): identified exactly, by the process handle and
    /// creation time recorded when it was started, never by name.
    fn end_sweeper_running(&self, pool: &PoolUser) -> bool;
    /// A free network slot for the pool user (`wfp::check_slot`).
    fn claim_slot(&self, matter: &str, pool: &PoolUser) -> Result<Slot, Error>;
    /// Gives the slot back.
    fn release_slot(&self, slot: &Slot);

    /// The folder gate and the plan (`admit`, `plan`): reads only.
    fn plan(&self, policy: &Policy, pool: &PoolUser) -> Result<Self::Plan, Error>;
    /// The launch check (`clean_close::check_launch`: the admin copy, the clean-close record,
    /// the journal).
    fn check_launch(&self, plan: &Self::Plan) -> Result<Self::Check, Error>;
    /// The network check (`wfp::check_launch`).
    fn check_network(&self, policy: &Policy, pool: &PoolUser) -> Result<(), Error>;
    /// The processes running as the pool user (their pids), outside any job of this launch's:
    /// leftovers of an earlier run. Asked after the launch check, and again at `launch`. The
    /// end sweeper ([`end_sweeper_running`](Self::end_sweeper_running)), identified exactly,
    /// is never one: it never triggers a refusal or a stop.
    fn pool_user_processes(&self, pool: &PoolUser) -> Result<Vec<u32>, Error>;
    /// The SYSTEM stop task for the pool user (`setup::stop`), which ends its processes; what
    /// it reported.
    fn stop_pool_user(&self, pool: &PoolUser) -> Result<Stopped, Error>;
    /// The pool user's own sweep (the runner's `--sweep`, `runner::sweep`), run to its end:
    /// what it may start at a logon goes too.
    fn sweep_pool_user(&self, pool: &PoolUser) -> Result<(), Error>;
    /// The set-aside's launch pass (`before_launch_checked`): what it moved.
    fn before_launch(
        &self,
        plan: &Self::Plan,
        check: &Self::Check,
        session: &str,
        running: &[&str],
    ) -> Result<Vec<SetAside>, Error>;
    /// The SYSTEM side's `PrepareRun{slot}` (C7): hardens the loose tasks, makes the run's
    /// group and adds the pool user.
    fn prepare_run(&self, slot: &Slot, pool: &PoolUser) -> Result<Self::RunGroup, RunRefusal>;
    /// The SYSTEM side's `EndRun{group}`: deletes the group.
    fn end_run(&self, group: Self::RunGroup);
    /// Applies the plan (`apply_checked`): the walk, which stops at the next folder once
    /// `walk` is cancelled ([`Error::Cancelled`]) and counts the folders it walked.
    fn apply(&self, plan: &Self::Plan, check: &Self::Check, walk: &Walk<'_>) -> Result<(), Error>;

    /// Writes the protected launch file for `spec` (`LaunchMessage::from_policy`,
    /// `write_launch`): its path.
    fn write_launch(
        &self,
        policy: &Policy,
        pool: &PoolUser,
        spec: &LaunchSpec,
    ) -> Result<PathBuf, Error>;
    /// Starts the outer runner on the launch file (`spawn_in_tree` on `RUNNER_EXE`).
    fn spawn_outer(
        &self,
        launch: &Path,
        opts: &SpawnOptions,
    ) -> Result<Launched, clauseo_process::Error>;

    /// The journal's cursor, before the repair (`session_cursor`).
    fn session_cursor(&self, end: &Ending<'_>) -> Option<Self::Cursor>;
    /// The repair (`repair_after_session`).
    fn repair(&self, end: &Ending<'_>) -> Result<Self::Repaired, Error>;
    /// Ends R18's reads of other matters (`end_reads`).
    fn end_reads(&self, end: &Ending<'_>);
    /// The set-aside's after-session pass (`set_aside::after_session`), which takes the session
    /// out of the open sessions.
    fn set_aside_after(&self, end: &Ending<'_>) -> Result<Self::Passed, Error>;
    /// What that pass moved.
    fn moved(&self, passed: &Self::Passed) -> Vec<SetAside>;
    /// The clean-close record (`write_clean_close`), given the cursor and both results.
    fn write_clean_close(
        &self,
        end: &Ending<'_>,
        cursor: Option<&Self::Cursor>,
        repaired: &Result<Self::Repaired, Error>,
        passed: &Result<Self::Passed, Error>,
    );

    /// A monotonic clock, from any origin.
    fn now(&self) -> Duration;
    /// Waits `pause` (a fake moves its clock instead).
    fn pause(&self, pause: Duration);
}
