//! `--wsl-deny`'s schedule (ADR 0049, "The WSL deny task"): when its passes run and when the run
//! ends. Every pass is [`pass`]: the same re-read and re-apply under the set-up lock, the deny's
//! only writer. The run never looks at a pool user's process.
//!
//! - **Watch, then read.** The run arms one change notification on
//!   `HKLM\SOFTWARE\Classes\AppID` (the 64-bit view's machine key, never `HKEY_CLASSES_ROOT`)
//!   and on Windows Installer's `InProgress` ([`WslWatch::watch`]) before every pass, the first
//!   included, so a change made while a pass reads is still caught by the next wait. It tries
//!   the arm again on every loop, so a watch that failed (an arm, or a wait) comes back.
//! - **The floor.** A pass every [`WSL_RECHECK_PAUSE`] for [`WSL_RECHECKS`] more passes, change
//!   or none, as before: with no install and no change, the run is the old one.
//! - **Wake on change.** A change wakes the wait at once. The run settles [`WSL_WAKE_SETTLE`],
//!   and at least [`WSL_WAKE_GAP`] after its last pass, so a storm of changes (the installer's,
//!   or the run's own write) gives at most one pass a second; then it re-arms and passes.
//! - **Timed retries.** After the floor, while the deny isn't verified or the watch is down
//!   (an arm or a wait failed since the last pass that followed a good arm), a pass runs every
//!   [`WSL_RECHECK_PAUSE`], held or not: a failed pass or a lost watch never waits for the
//!   install's end.
//! - **The hold.** While `InProgress` is there, and until it has been gone for
//!   [`WSL_INSTALL_QUIET`], the run keeps waiting for changes, past the floor. WSL's installer
//!   deletes its `AppID` keys ~5 s in and writes them back ~21 s in, with stock permissions; the
//!   wake re-denies them. The Windows Installer service writes `InProgress` for any user's
//!   install, so a pool user's per-user MSI can hold the run too: up to [`WSL_DENY_BUDGET`],
//!   re-checking the deny, never weakening it. `InProgress` is read at two paths, neither
//!   confirmed on a VM, so the run also holds while any `AppID` the record lists
//!   (`WslDenyAppIds`) has no key ([`WslWatch::recorded_key_missing`]): such a pass is never
//!   verified, and WSL's keys-absent window keeps the run whatever `InProgress` says. Only
//!   while WSL is present ([`WslWatch::wsl_present`]): once WSL has read as absent for more than
//!   [`WSL_ABSENT_GRACE`] (longer than its installer's ~16 s between deleting the keys and
//!   writing them back, when its service, classes or package may be gone too), its recorded
//!   `AppID`s, whose keys are gone for good, hold nothing (logged once).
//!   Windows Installer's `_MSIExecute` mutex is a hint only, never a hold: it can be held by a
//!   process that isn't an install, and stays after one. It is logged when held with no
//!   `InProgress`, so a VM run shows the two disagree.
//! - **The end.** Once the floor is done and `InProgress` has been gone for
//!   [`WSL_INSTALL_QUIET`], a pass that verifies every target ends the run, held or not. One
//!   that doesn't is tried again every [`WSL_RECHECK_PAUSE`]. [`WSL_DENY_BUDGET`] ends the run
//!   whatever it is waiting for, inside the task's 6-minute limit
//!   (`rules::WSL_DENY_EXECUTION_LIMIT`); a run again (`run_lock.rs`) gets what is left of it.
//!   An error line with the deny unverified, a warning with it verified (an install still in
//!   progress may yet remove it, or one just over left no time for its settle).
//!
//! A repeated failure is logged once while it repeats, so a run that waits out an install with a
//! key missing writes one line, not one a pass; so is a watch failure that repeats.

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
/// How long WSL must read as absent before its recorded `AppID`s' missing keys stop holding.
pub(in crate::windows::refresh) const WSL_ABSENT_GRACE: Duration = Duration::from_secs(30);
/// The run's own bound: the task's 6 minutes less 30 s. No pass starts that could end past it.
pub(in crate::windows::refresh) const WSL_DENY_BUDGET: Duration = Duration::from_secs(330);
/// The longest a woken pass takes: its wait before the pass, then the lock's.
pub(super) const PASS_WORST: Duration = WSL_WAKE_GAP.saturating_add(WSL_LOCK_WAIT);

/// What [`WslWatch::installer`] read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Installer {
    /// Windows Installer's `InProgress` key or value is there: an install is under way, any
    /// user's (the service writes it for a per-user MSI too). Only administrators and SYSTEM
    /// can write it directly.
    pub in_progress: bool,
    /// `Global\_MSIExecute` is there: a hint, logged, never a hold.
    pub mutex: bool,
}

/// What [`WslWatch::wait_change`] saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Waited {
    /// The armed notification fired.
    Changed,
    /// The time ran out, or none was armed.
    Quiet,
    /// The wait failed: the watch is dropped (the next arm opens it again), and the time was
    /// waited out without it.
    Failed(Failed),
}

/// What `--wsl-deny`'s schedule needs besides the passes: a clock, the change notification and
/// Windows Installer's state.
pub trait WslWatch: Refresher {
    /// A monotonic clock: the time since a fixed point.
    fn now(&mut self) -> Duration;
    /// Arms one notification (re-arms it, after the first; opens the watch first when it has
    /// none) of a change under `HKLM\SOFTWARE\Classes\AppID`, subtree, a name, a value or a
    /// descriptor, and of `InProgress` appearing or going. A change before the arm that wasn't
    /// waited for is dropped: the pass after the arm reads it. One armed and unfired stays.
    fn watch(&mut self) -> Result<(), Failed>;
    /// Waits up to `time` for the armed notification.
    fn wait_change(&mut self, time: Duration) -> Waited;
    /// Windows Installer's state now.
    fn installer(&mut self) -> Installer;
    /// Whether any `AppID` in the record's `WslDenyAppIds` has no key under
    /// `HKLM\SOFTWARE\Classes\AppID`. A read that fails is the error, which holds as a missing
    /// key does.
    fn recorded_key_missing(&mut self) -> Result<bool, Failed>;
    /// Whether WSL is present (`wsl::inspect` gives anything but `NoWsl`, a read that fails
    /// included); asked only when a recorded key is missing.
    fn wsl_present(&mut self) -> bool;
    /// Writes one warning line to the log.
    fn warn(&mut self, line: &str);
}

/// The run's state between its passes; times are since its start.
struct Schedule {
    start: Duration,
    /// The floor's passes still owed.
    ticks: u32,
    /// Since when `InProgress` has been gone; `None` while it is there.
    idle_since: Option<Duration>,
    /// When the mutex was first seen with no `InProgress`.
    hint_only: Option<Duration>,
    /// Since when WSL has read as absent, with a recorded key missing, at every pass.
    absent_since: Option<Duration>,
    /// Whether "WSL not installed" was logged.
    absent_logged: bool,
    last_pass: Duration,
    verified: bool,
    /// Whether an arm or a wait failed since the last pass that followed a good arm.
    down: bool,
    /// The last failed pass's line, while the failure repeats.
    repeating: Option<String>,
    /// The last watch failure logged: one that repeats isn't logged again.
    watch_error: Option<String>,
}

/// The passes, as the module says: the first included, once the run is allowed, within
/// `budget` (what is left of [`WSL_DENY_BUDGET`] for the process, `run_lock.rs`). Whether the
/// run ended before the budget stopped it.
pub(super) fn passes<F: WslWatch>(fs: &mut F, budget: Duration) -> bool {
    let start = fs.now();
    let mut run = Schedule {
        start,
        ticks: WSL_RECHECKS,
        idle_since: None,
        hint_only: None,
        absent_since: None,
        absent_logged: false,
        last_pass: Duration::ZERO,
        verified: false,
        down: false,
        repeating: None,
        watch_error: None,
    };
    let armed = run.arm(fs);
    run.pass(fs, Duration::ZERO, armed);
    let mut in_time = true;
    loop {
        let at = run.at(fs);
        run.arm(fs);
        run.observe(at, fs.installer());
        if run.done(at) {
            break;
        }
        let left = budget.saturating_sub(PASS_WORST).saturating_sub(at);
        if left.is_zero() {
            run.out_of_time(fs);
            in_time = false;
            break;
        }
        let woke = match fs.wait_change(run.wait(at).min(left)) {
            Waited::Changed => true,
            Waited::Quiet => false,
            Waited::Failed(why) => {
                run.watch_failed(fs, &why);
                false
            }
        };
        if woke {
            let since = run.at(fs).saturating_sub(run.last_pass);
            fs.pause(WSL_WAKE_SETTLE.max(WSL_WAKE_GAP.saturating_sub(since)));
        }
        let at = run.at(fs);
        let tick = run.ticks > 0 && at >= run.next_tick();
        if tick {
            run.ticks -= 1;
        }
        if woke || tick || run.due(at) {
            let armed = run.arm(fs);
            run.pass(fs, at, armed);
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
    in_time
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

    /// Arms the notification (again); whether it is armed now.
    fn arm(&mut self, fs: &mut impl WslWatch) -> bool {
        match fs.watch() {
            Ok(()) => true,
            Err(why) => {
                self.watch_failed(fs, &why);
                false
            }
        }
    }

    /// The watch failed: timed passes until a pass follows a good arm. A failure that repeats
    /// the last one logged isn't logged again.
    fn watch_failed(&mut self, fs: &mut impl WslWatch, why: &str) {
        self.down = true;
        if self.watch_error.as_deref() != Some(why) {
            let line = format!("WSL's AppID keys aren't watched, so passes run every 5 s: {why}");
            fs.log(true, &line);
            self.watch_error = Some(why.to_owned());
        }
    }

    /// The budget's line: an error with the deny unverified, else a warning.
    fn out_of_time(&self, fs: &mut impl WslWatch) {
        let stopped = "WSL's deny task stopped at its time limit";
        if !self.verified {
            fs.log(true, &format!("{stopped}: WSL's deny wasn't verified"));
        } else if self.idle_since.is_none() {
            fs.warn(&format!(
                "{stopped}: Windows Installer's install was still in progress"
            ));
        } else {
            // InProgress went too late for its 3 s settle and a final pass inside the budget.
            fs.warn(&format!(
                "{stopped}: Windows Installer's install had just ended, with no time left to \
                 settle and check again"
            ));
        }
    }

    /// Notes Windows Installer's state at `at`.
    fn observe(&mut self, at: Duration, installer: Installer) {
        if installer.in_progress {
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

    /// Whether the last pass came once `InProgress` had been gone long enough, at `at`.
    fn final_seen(&self, at: Duration) -> Option<bool> {
        self.quiet(at)
            .map(|since| self.last_pass >= since.saturating_add(WSL_INSTALL_QUIET))
    }

    /// Whether the run may end at `at`: the floor done, `InProgress` gone long enough, and a
    /// pass since then that verified every target.
    fn done(&self, at: Duration) -> bool {
        self.ticks == 0 && self.verified && self.final_seen(at) == Some(true)
    }

    /// Whether a pass is owed at `at` with no wake and no floor pass: the final one, once
    /// `InProgress` has been gone long enough, and the timed retries.
    fn due(&self, at: Duration) -> bool {
        if self.ticks > 0 {
            return false;
        }
        let retry = self.retrying() && at.saturating_sub(self.last_pass) >= WSL_RECHECK_PAUSE;
        self.final_seen(at) == Some(false) || retry
    }

    /// Whether timed retries run: the deny unverified, or the watch down.
    fn retrying(&self) -> bool {
        !self.verified || self.down
    }

    /// How long to wait for a change at `at`.
    fn wait(&self, at: Duration) -> Duration {
        if self.ticks > 0 {
            return self.next_tick().saturating_sub(at);
        }
        let since_pass = at.saturating_sub(self.last_pass);
        let retry = WSL_RECHECK_PAUSE.saturating_sub(since_pass);
        let wait = match (self.idle_since, self.final_seen(at)) {
            (None, _) => WSL_INSTALL_POLL,
            (Some(since), None) => since
                .saturating_add(WSL_INSTALL_QUIET)
                .saturating_sub(at)
                .min(WSL_INSTALL_POLL),
            (Some(_), Some(false)) => Duration::ZERO,
            (Some(_), Some(true)) => retry,
        };
        if self.retrying() {
            wait.min(retry)
        } else {
            wait
        }
    }

    /// One pass at `at`, after an arm (`armed` when it held), logged, a failure that repeats
    /// the last one not again.
    fn pass(&mut self, fs: &mut impl WslWatch, at: Duration, armed: bool) {
        let pass = pass(fs);
        self.last_pass = at;
        if armed {
            self.down = false;
        }
        let missing = self.missing(fs, at);
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

    /// Whether a recorded `AppID` has no key, at `at`: never verified, so it holds the run
    /// (WSL's installer deletes the keys, then writes them back ~16 s later), unless WSL has read
    /// as absent for more than [`WSL_ABSENT_GRACE`]: uninstalled, its keys never come back.
    fn missing(&mut self, fs: &mut impl WslWatch, at: Duration) -> bool {
        if !fs.recorded_key_missing().unwrap_or(true) {
            self.absent_since = None;
            return false;
        }
        if fs.wsl_present() {
            self.absent_since = None;
            return true;
        }
        let since = *self.absent_since.get_or_insert(at);
        if at.saturating_sub(since) <= WSL_ABSENT_GRACE {
            return true;
        }
        if !self.absent_logged {
            self.absent_logged = true;
            fs.log(false, "WSL not installed; recorded AppIDs not held");
        }
        false
    }
}
