//! Every OS effect of tier B's launcher, behind one trait, so the order of [`Launchers`]
//! (`launch.rs`) is tested on every OS with fakes. The real steps (`OsSteps`, C5) call the
//! ledger, `MatterSids`, `set_aside`, `clean_close`, `apply`, `wfp`, the set-up server and the
//! runner's launch file.
//!
//! [`Launchers`]: super::Launchers

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launched, SpawnOptions};

use super::feed::DenialPipe;
use super::prepare::Walk;
use crate::windows::pool::PoolUser;
use crate::windows::pool::assign::Assign;
use crate::{Error, Policy, SetAside};

/// A network slot: its number, the host ports (`H_s`) it holds and the claim its record names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    /// The slot's number.
    pub index: u8,
    /// Its host ports.
    pub ports: Vec<u16>,
    /// The reservation's claim (unique in the process) that the slot's record names, so a late
    /// release of an old claim never frees a newer claim's record of the same slot.
    pub claim: u64,
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
    /// The end's progress and stop, for its work steps ([`LaunchSteps::repair`],
    /// [`LaunchSteps::set_aside_after`]): the walk's stall rule and cap. A step that stalls
    /// stops (`Cancelled`), and the end goes on with the next.
    pub walk: &'a Walk<'a>,
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
/// **No step waits unbounded.** The walk looks at its cancel, its waits' 180 s budget and its
/// work's 60 s stall timer and 20 min cap between steps (`walk.rs`), but can't interrupt a step
/// blocked in an OS call: every wait a real step makes (a scheduled task, the set-up server's
/// pipe, a process handle, a file lock) is a timed one, no longer than the budget; the work
/// steps ([`plan`](Self::plan), [`check_launch`](Self::check_launch),
/// [`before_launch`](Self::before_launch), [`apply`](Self::apply), and at the end
/// [`repair`](Self::repair) and [`set_aside_after`](Self::set_aside_after)) count their progress
/// ([`Walk::advance`]) every few seconds, from the thread doing the work, and stop once
/// [`Walk::cancelled`] says so. A step that never returns makes its matter `MatterStuck`
/// until Clauseo restarts.
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
    /// A launch's denial pipe (`feed.rs`; `OsDenialPipe` on Windows).
    type Pipe: DenialPipe;

    /// The sandbox's data folder, which [`Launchers::with_steps`](super::Launchers::with_steps)
    /// locks (`tier-b-launchers.lock`) for as long as the launchers live: one set of launchers
    /// per folder, across processes. It must exist.
    fn data_dir(&self) -> &Path;

    /// At [`Launchers::with_steps`](super::Launchers::with_steps), with the folder's lock
    /// held, before any reservation:
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
    /// A free network slot for the pool user (`wfp::check_slot`), its record naming `claim`: a
    /// record only. It adds no firewall permit and changes no filter (there is no static permit
    /// since #791; PR6's connect permits come only after `check_launch`, behind the job gate);
    /// the slot names the ports the host's proxy binds.
    fn claim_slot(&self, matter: &str, pool: &PoolUser, claim: u64) -> Result<Slot, Error>;
    /// Gives the slot back: removes its record only while the record still names `slot.claim`.
    fn release_slot(&self, slot: &Slot);

    /// The folder gate and the plan (`admit`, `plan`): reads only.
    /// A work step: ticks `walk`, stops once it is cancelled.
    fn plan(&self, policy: &Policy, pool: &PoolUser, walk: &Walk<'_>) -> Result<Self::Plan, Error>;
    /// The launch check (`clean_close::check_launch`: the admin copy, the clean-close record,
    /// the journal). A work step, as `plan` is.
    fn check_launch(&self, plan: &Self::Plan, walk: &Walk<'_>) -> Result<Self::Check, Error>;
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
    /// The set-aside's launch pass (`before_launch_checked`): what it moved. A work step.
    fn before_launch(
        &self,
        plan: &Self::Plan,
        check: &Self::Check,
        session: &str,
        running: &[&str],
        walk: &Walk<'_>,
    ) -> Result<Vec<SetAside>, Error>;
    /// The SYSTEM side's `PrepareRun{slot}` (C7): hardens the loose tasks, makes the run's
    /// group and adds the pool user.
    fn prepare_run(&self, slot: &Slot, pool: &PoolUser) -> Result<Self::RunGroup, RunRefusal>;
    /// The SYSTEM side's `EndRun{group}`: deletes the group. A failure keeps the pool user and
    /// its slot held back until a later `EndRun` of the group succeeds.
    fn end_run(&self, group: &Self::RunGroup) -> Result<(), Error>;
    /// Applies the plan (`apply_checked`): the walk, which stops at the next folder once
    /// `walk` is cancelled ([`Error::Cancelled`]) and counts the folders it walked (its
    /// progress).
    fn apply(&self, plan: &Self::Plan, check: &Self::Check, walk: &Walk<'_>) -> Result<(), Error>;

    /// Makes a launch's denial pipe (`OsDenialPipe::create`): it, and its name for the launch
    /// file.
    fn create_pipe(&self) -> Result<(Self::Pipe, String), Error>;
    /// Writes the protected launch file for `spec` (`LaunchMessage::from_policy`,
    /// `write_launch`), naming `denial_pipe`, the pipe the outer writes its records to: its path.
    ///
    /// TODO(#789): the launch message's new field (`denial_pipe`) carries the name.
    fn write_launch(
        &self,
        policy: &Policy,
        pool: &PoolUser,
        spec: &LaunchSpec,
        denial_pipe: &str,
    ) -> Result<PathBuf, Error>;
    /// Removes the launch file `launch`: it holds the agent's environment. One already gone is
    /// `Ok`. A failure is noted and tried again at each end and undo, and the next start's
    /// stale sweep ([`clear_stale`](Self::clear_stale)) removes any left.
    fn remove_launch(&self, launch: &Path) -> Result<(), Error>;
    /// The outer runner: `<install>\clauseo-sandbox-run.exe` (`RUNNER_EXE`).
    fn runner(&self) -> PathBuf;
    /// The host's own variable `name`, for the outer's minimal environment.
    fn host_var(&self, name: &str) -> Option<OsString>;
    /// Starts the outer runner as `spec` says, in the host's tree (`spawn_in_tree`).
    fn spawn_outer(
        &self,
        spec: &LaunchSpec,
        opts: &SpawnOptions,
    ) -> Result<Launched, clauseo_process::Error>;
    /// Ends the outer a launcher may have started for `pool`'s run, after a panic in the
    /// launcher: kills its job and waits until the job is truly empty (#697), or fails. Nothing
    /// to end is fine. TODO(C4/C5): the real one, by the job the spawn made.
    fn end_outer(&self, pool: &PoolUser) -> Result<(), Error>;
    /// Waits up to `within` for every process running as the pool user to have truly exited:
    /// found by its token's user and each handle waited on, not by the job's count, which reads
    /// empty while processes still tear down (stage D, #697). The pids still running then.
    fn wait_exited(&self, pool: &PoolUser, within: Duration) -> Result<Vec<u32>, Error>;

    /// The journal's cursor, before the repair (`session_cursor`).
    fn session_cursor(&self, end: &Ending<'_>) -> Option<Self::Cursor>;
    /// The repair (`repair_after_session`). A work step: counts `end.walk`, stops once it is
    /// cancelled.
    fn repair(&self, end: &Ending<'_>) -> Result<Self::Repaired, Error>;
    /// Ends R18's reads of other matters (`end_reads`).
    fn end_reads(&self, end: &Ending<'_>);
    /// The set-aside's after-session pass (`set_aside::after_session`), which takes the session
    /// out of the open sessions. A work step, as `repair` is.
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
