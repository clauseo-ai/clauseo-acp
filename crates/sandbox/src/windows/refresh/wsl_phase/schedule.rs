//! `--wsl-deny`'s schedule (ADR 0049, "The WSL deny task"): when its passes run and when the run
//! ends. Every pass is [`pass`]: the same re-read and re-apply under the set-up lock, the deny's
//! only writer. The run never looks at a pool user's process.
//!
//! - **Watch, then read.** The run arms one change notification on
//!   `HKLM\SOFTWARE\Classes\AppID` (the 64-bit view's machine key, never `HKEY_CLASSES_ROOT`)
//!   and on Windows Installer's `InProgress` ([`WslWatch::watch`]) before every pass, the first
//!   included, so a change made while a pass reads is still caught by the next wait.
//! - **The floor.** A pass every [`WSL_RECHECK_PAUSE`] for [`WSL_RECHECKS`] more passes, change
//!   or none, as before: with no install and no change, the run is the old one.
//! - **Wake on change.** A change wakes the wait at once. The run settles [`WSL_WAKE_SETTLE`],
//!   and at least [`WSL_WAKE_GAP`] after its last pass, so a storm of changes (the installer's,
//!   or the run's own write) gives at most one pass a second; then it re-arms and passes.
//! - **The hold.** While `InProgress` is there, and until it has been gone for
//!   [`WSL_INSTALL_QUIET`], the run keeps waiting for changes, past the floor. WSL's installer
//!   deletes its `AppID` keys ~5 s in and writes them back ~21 s in, with stock permissions; the
//!   wake re-denies them. The Windows Installer service writes `InProgress` for any user's
//!   install, so a pool user's per-user MSI can hold the run too: up to [`WSL_DENY_BUDGET`],
//!   re-checking the deny, never weakening it. `InProgress` is read at two paths, neither
//!   confirmed on a VM, so the run also holds while any `AppID` the record lists
//!   (`WslDenyAppIds`) has no key ([`WslWatch::recorded_key_missing`]): such a pass is never
//!   verified, and WSL's keys-absent window keeps the run whatever `InProgress` says. Only
//!   while WSL is present ([`WslWatch::wsl_present`]): once it is uninstalled, its `AppID`s stay
//!   in the record and their keys are gone for good, so they hold nothing (logged once).
//!   Windows Installer's `_MSIExecute` mutex is a hint only, never a hold: it can be held by a
//!   process that isn't an install, and stays after one. It is logged when held with no
//!   `InProgress`, so a VM run shows the two disagree.
//! - **The end.** Once the floor is done and `InProgress` has been gone for
//!   [`WSL_INSTALL_QUIET`], a pass that verifies every target ends the run. One that doesn't
//!   is tried again every [`WSL_RECHECK_PAUSE`] after an install, or ends the run when the run
//!   saw none (as before). [`WSL_DENY_BUDGET`] ends the run whatever it is waiting for, inside
//!   the task's 6-minute limit (`rules::WSL_DENY_EXECUTION_LIMIT`).
//!
//! A repeated failure is logged once while it repeats, so a run that waits out an install with a
//! key missing writes one line, not one a pass.

use std::time::Duration;

use super::super::{Failed, Refresher, WSL_LOCK_WAIT};
use super::{WSL_RECHECK_PAUSE, WSL_RECHECKS, pass};

/// How long a wake waits before its pass, so one install step's writes give one pass.
pub(in crate::windows::refresh) const WSL_WAKE_SETTLE: Duration = Duration::from_millis(250);
/// The least time between a pass and a wake's pass after it: a storm's passes, one a second.
pub(in crate::windows::refresh) const WSL_WAKE_GAP: Duration = Duration::from_secs(1);
/// The longest wait while the run holds for an install: it re-reads `InProgress` this often,
/// should a notification be missed.
const WSL_INSTALL_POLL: Duration = Duration::from_secs(1);
/// How long `InProgress` must have been gone before the run may end.
pub(in crate::windows::refresh) const WSL_INSTALL_QUIET: Duration = Duration::from_secs(3);
/// The run's own bound: the task's 6 minutes less 30 s. No pass starts that could end past it.
pub(in crate::windows::refresh) const WSL_DENY_BUDGET: Duration = Duration::from_secs(330);
/// The longest a woken pass takes: its wait before the pass, then the lock's.
const PASS_WORST: Duration = WSL_WAKE_GAP.saturating_add(WSL_LOCK_WAIT);

/// What [`WslWatch::installer`] read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Installer {
    /// Windows Installer's `InProgress` key or value is there: an install is under way, any
    /// user's (the service writes it for a per-user MSI too). Only administrators and SYSTEM
    /// can write it directly. With a recorded `AppID`'s key missing, it holds the run.
    pub in_progress: bool,
    /// `Global\_MSIExecute` is there: a hint, logged, never a hold.
    pub mutex: bool,
}

/// What `--wsl-deny`'s schedule needs besides the passes: a clock, the change notification and
/// Windows Installer's state.
pub trait WslWatch: Refresher {
    /// A monotonic clock: the time since a fixed point.
    fn now(&mut self) -> Duration;
    /// Arms one notification (re-arms it, after the first) of a change under
    /// `HKLM\SOFTWARE\Classes\AppID`, subtree, a name, a value or a descriptor, and of `InProgress`
    /// appearing or going. A change before the arm that wasn't waited for is dropped: the pass
    /// after the arm reads it.
    fn watch(&mut self) -> Result<(), Failed>;
    /// Waits up to `time` for the armed notification: `true` when a change came. With none
    /// armed it waits `time` and gives `false`.
    fn wait_change(&mut self, time: Duration) -> bool;
    /// Windows Installer's state now.
    fn installer(&mut self) -> Installer;
    /// Whether any `AppID` in the record's `WslDenyAppIds` has no key under
    /// `HKLM\SOFTWARE\Classes\AppID`. A read that fails is the error, which holds as a missing
    /// key does.
    fn recorded_key_missing(&mut self) -> Result<bool, Failed>;
    /// Whether WSL is present (`wsl::inspect` gives anything but `NoWsl`, a read that fails
    /// included); asked only when a recorded key is missing.
    fn wsl_present(&mut self) -> bool;
}

/// The run's state between its passes; times are since its start.
struct Schedule {
    start: Duration,
    /// The floor's passes still owed.
    ticks: u32,
    /// Whether `InProgress` was seen in this run, or a recorded `AppID`'s key was missing.
    held: bool,
    /// Since when `InProgress` has been gone; `None` while it is there.
    idle_since: Option<Duration>,
    /// When the mutex was first seen with no `InProgress`.
    hint_only: Option<Duration>,
    /// When a recorded key was first found missing with WSL not installed.
    no_wsl: Option<Duration>,
    last_pass: Duration,
    verified: bool,
    /// Whether the notification is armed and re-armed; once arming fails, the timed passes go
    /// on alone.
    watching: bool,
    /// The last failed pass's line, while the failure repeats.
    repeating: Option<String>,
}

/// The passes, as the module says: the first included, once the run is allowed.
pub(super) fn passes<F: WslWatch>(fs: &mut F) {
    let start = fs.now();
    let mut run = Schedule {
        start,
        ticks: WSL_RECHECKS,
        held: false,
        idle_since: None,
        hint_only: None,
        no_wsl: None,
        last_pass: Duration::ZERO,
        verified: false,
        watching: true,
        repeating: None,
    };
    run.arm(fs);
    run.pass(fs, Duration::ZERO);
    loop {
        let at = run.at(fs);
        run.observe(at, fs.installer());
        if run.done(at) {
            break;
        }
        let left = WSL_DENY_BUDGET
            .saturating_sub(PASS_WORST)
            .saturating_sub(at);
        if left.is_zero() {
            // An unverified deny is an error; an install still running with the deny in place,
            // information.
            let (error, why) = if run.verified {
                (false, "Windows Installer's install was still in progress")
            } else {
                (true, "WSL's deny wasn't verified")
            };
            fs.log(
                error,
                &format!("WSL's deny task stopped at its time limit: {why}"),
            );
            break;
        }
        let woke = fs.wait_change(run.wait(at).min(left));
        if woke {
            let since = run.at(fs).saturating_sub(run.last_pass);
            fs.pause(WSL_WAKE_SETTLE.max(WSL_WAKE_GAP.saturating_sub(since)));
            run.arm(fs);
        }
        let at = run.at(fs);
        let tick = run.ticks > 0 && at >= run.next_tick();
        if tick {
            run.ticks -= 1;
        }
        if woke || tick || run.final_due(at) {
            if !woke {
                run.arm(fs);
            }
            run.pass(fs, at);
        }
    }
    if let Some(at) = run.hint_only {
        let secs = at.as_secs();
        fs.log(
            false,
            &format!(
                "WSL's deny task saw Windows Installer's mutex held with no install in progress \
                 {secs} s in, and didn't wait for it"
            ),
        );
    }
}

impl Schedule {
    /// The time since the run started.
    fn at(&self, fs: &mut impl WslWatch) -> Duration {
        fs.now().saturating_sub(self.start)
    }

    /// When the next floor pass is due.
    fn next_tick(&self) -> Duration {
        WSL_RECHECK_PAUSE.saturating_mul(WSL_RECHECKS - self.ticks + 1)
    }

    /// Arms the notification again, when it is armed at all; a failure is logged once, and the
    /// timed passes go on.
    fn arm(&mut self, fs: &mut impl WslWatch) {
        if !self.watching {
            return;
        }
        if let Err(why) = fs.watch() {
            self.watching = false;
            let line = format!("WSL's AppID keys aren't watched, so only timed passes run: {why}");
            fs.log(true, &line);
        }
    }

    /// Notes Windows Installer's state at `at`.
    fn observe(&mut self, at: Duration, installer: Installer) {
        if installer.in_progress {
            self.held = true;
            self.idle_since = None;
        } else {
            self.idle_since.get_or_insert(at);
            if installer.mutex {
                self.hint_only.get_or_insert(at);
            }
        }
    }

    /// Whether `InProgress` has been gone for [`WSL_INSTALL_QUIET`] at `at`; its start if so.
    fn quiet(&self, at: Duration) -> Option<Duration> {
        self.idle_since
            .filter(|since| at.saturating_sub(*since) >= WSL_INSTALL_QUIET)
    }

    /// Whether the run may end at `at`.
    fn done(&self, at: Duration) -> bool {
        if self.ticks > 0 {
            return false;
        }
        self.quiet(at).is_some_and(|since| {
            let after = self.last_pass >= since.saturating_add(WSL_INSTALL_QUIET);
            !self.held || (self.verified && after)
        })
    }

    /// Whether a pass is owed at `at` with no wake and no floor pass: the final one, once
    /// `InProgress` has been gone long enough, and its retries.
    fn final_due(&self, at: Duration) -> bool {
        if self.ticks > 0 {
            return false;
        }
        self.quiet(at).is_some_and(|since| {
            let stale = self.last_pass < since.saturating_add(WSL_INSTALL_QUIET);
            let retry = !self.verified && at.saturating_sub(self.last_pass) >= WSL_RECHECK_PAUSE;
            stale || retry
        })
    }

    /// How long to wait for a change at `at`.
    fn wait(&self, at: Duration) -> Duration {
        if self.ticks > 0 {
            return self.next_tick().saturating_sub(at);
        }
        match self.idle_since {
            None => WSL_INSTALL_POLL,
            Some(since) if self.quiet(at).is_none() => since
                .saturating_add(WSL_INSTALL_QUIET)
                .saturating_sub(at)
                .min(WSL_INSTALL_POLL),
            Some(_) => WSL_RECHECK_PAUSE.saturating_sub(at.saturating_sub(self.last_pass)),
        }
    }

    /// One pass at `at`, logged, a failure that repeats the last one not again.
    fn pass(&mut self, fs: &mut impl WslWatch, at: Duration) {
        let pass = pass(fs);
        self.last_pass = at;
        // A recorded AppID with no key is never verified, and holds the run (WSL's installer
        // deletes the keys, then writes them back ~16 s later).
        let mut missing = fs.recorded_key_missing().unwrap_or(true);
        // With WSL uninstalled, its recorded AppIDs' keys never come back: no hold.
        if missing && !fs.wsl_present() {
            missing = false;
            if self.no_wsl.replace(at).is_none() {
                fs.log(false, "WSL not installed; recorded AppIDs not held");
            }
        }
        self.held |= missing;
        self.verified = pass.verified && !missing;
        let Some((error, line)) = pass.line else {
            self.repeating = None;
            return;
        };
        if pass.verified {
            self.repeating = None;
            fs.log(error, &line);
        } else if self.repeating.as_deref() != Some(line.as_str()) {
            fs.log(error, &line);
            self.repeating = Some(line);
        }
    }
}
