//! The updater's state machine (decision record 0037): pure, with no I/O, no clock read and no
//! randomness of its own. [`Machine::start`] builds it from what start-up read; [`step`] takes
//! one [`Input`] and the time, and gives the next machine and the [`Effect`]s the actor carries
//! out, or a [`Refusal`] for a command the state does not take.
//!
//! Its transitions are R23's table:
//!
//! | From | Input | To |
//! |---|---|---|
//! | `idle`, `failed` | timer, `CheckRequested` | `checking` |
//! | `checking` | nothing offered | `idle`, `checked_at` set |
//! | `checking` | an offer | `downloading` (`ready` when it is the cached version) |
//! | `downloading` | done | `ready` |
//! | `ready` | `InstallRequested(now)` | `installing`, after cancelling running turns |
//! | `ready` | `InstallRequested(after_turn)` | `scheduled`, or `installing` if no turn runs |
//! | `scheduled` | no turn running | `installing` |
//! | `scheduled` | `InstallRequested(now)` | as `ready` with `now` |
//! | `scheduled`, `installing` before its install began | `CancelScheduled` | `ready` |
//! | `checking`, `downloading` | an error | `failed` |
//! | any | the install kind, `update.mode: none` or policy says managed | `managed` |
//!
//! `installing` to `failed` happens at the next start: the marker `{ from, to }` with `from`
//! the running version. Events that arrive in a state they no longer concern (a check's result
//! after policy made the run managed, a timer a check on command cleared) change nothing. The
//! timers and the once-a-day counts are in `timers.rs`.

use clauseo_counts::schema::UpdateResult;
use clauseo_protocol::{
    Count, FailedStage, InstallWhen, ManagedBy, Timestamp, UpdateMode, UpdateState, UpdateView,
};

use super::{
    CachedDownload, Downloaded, InstallKind, MarkerState, Offer, UpdateMarker, UpdaterError,
};

mod jitter;
mod timers;
mod vocab;
pub(crate) use jitter::Jitter;
use timers::once_a_day;
pub(crate) use vocab::{Effect, Input, ModeInForce, Refusal, Start};

#[cfg(test)]
mod tests;

const MINUTE: i64 = 60_000;
/// How often a managed run counts `Managed`.
const DAY: i64 = 24 * 60 * MINUTE;

/// How far an install has gone, behind `installing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallStep {
    /// None asked for, or taken back.
    None,
    /// `now` with turns running: new sends held, waiting for the cancelled turns to end.
    StoppingTurns,
    /// `BeginInstall` has gone out: nothing takes the install back.
    Begun,
}

/// Where the updater stands, and what it needs to decide the next step.
#[derive(Debug, Clone)]
pub(crate) struct Machine {
    current: String,
    state: UpdateState,
    mode: ModeInForce,
    kind: InstallKind,
    checked_at: Option<Timestamp>,
    updated_from: Option<String>,
    /// The download behind `ready`, `scheduled` and `installing`.
    download: Option<Downloaded>,
    /// Last run's download, until the first check that ends.
    cached: Option<CachedDownload>,
    /// The check running is start-up's in `auto` with `cached`: the same version installs.
    install_at_start: bool,
    /// How far an install has gone.
    install: InstallStep,
    turns_running: bool,
    /// The running check, and the download it started, are the lawyer's (`CheckRequested`):
    /// their failure always counts.
    asked: bool,
    timer: Option<Timestamp>,
    managed_counted: Option<Timestamp>,
    /// When a background check or download failure last counted.
    failure_counted: Option<Timestamp>,
    jitter: Jitter,
}

impl Machine {
    /// The machine after start-up: the marker read, then `managed`, an immediate check (`auto`
    /// with last run's download), or `idle` with the first check 1 to 10 minutes away.
    pub(crate) fn start(start: Start, now: Timestamp) -> (Self, Vec<Effect>) {
        let mut m = Self {
            current: start.current,
            state: UpdateState::Idle,
            mode: start.mode,
            kind: start.kind,
            checked_at: None,
            updated_from: None,
            download: None,
            cached: None,
            install_at_start: false,
            install: InstallStep::None,
            turns_running: false,
            timer: None,
            asked: false,
            managed_counted: None,
            failure_counted: None,
            jitter: start.jitter,
        };
        let mut fx = Vec::new();
        let mut install_failed = false;
        if let Some(marker) = start.marker {
            fx.push(Effect::ClearMarker);
            if marker.to == m.current {
                m.updated_from = Some(marker.from);
                fx.push(Effect::Count(UpdateResult::Installed));
            } else if marker.from == m.current {
                install_failed = true;
                m.state = UpdateState::Failed {
                    version: Some(marker.to),
                    at: now,
                    stage: FailedStage::Install,
                };
                fx.push(Effect::Count(UpdateResult::Failed));
            }
        }
        // A download whose install just failed is fetched again, never retried from the file.
        let cached = start
            .cached
            .filter(|c| !install_failed && c.version != m.current);
        if let Some(by) = m.managed_by() {
            m.enter_managed(by, now, &mut fx);
        } else if let Some(cached) = cached {
            m.install_at_start = m.mode.mode == UpdateMode::Auto;
            m.cached = Some(cached);
            if m.install_at_start {
                m.begin_check(false, &mut fx);
            } else {
                timers::arm_first_check(&mut m, now, &mut fx);
            }
        } else {
            timers::arm_first_check(&mut m, now, &mut fx);
        }
        fx.push(Effect::Publish);
        (m, fx)
    }

    /// What `GetUpdate` answers and `UpdateChanged` carries.
    pub(crate) fn view(&self) -> UpdateView {
        UpdateView {
            current: self.current.clone(),
            state: self.state.clone(),
            mode: self.mode.mode,
            checked_at: self.checked_at,
            updated_from: self.updated_from.clone(),
        }
    }

    /// When the armed timer fires, if one is armed.
    pub(crate) fn timer(&self) -> Option<Timestamp> {
        self.timer
    }

    /// Whether new sends are held queued.
    pub(crate) fn gated(&self) -> bool {
        self.install == InstallStep::StoppingTurns
    }

    /// Who manages updates, when someone else does: policy first (`none` in force), then the
    /// install kind.
    fn managed_by(&self) -> Option<ManagedBy> {
        if self.mode.mode == UpdateMode::None {
            return Some(ManagedBy::Policy);
        }
        match self.kind {
            InstallKind::CurrentUser => None,
            InstallKind::PerMachine => Some(ManagedBy::PerMachine),
            InstallKind::ReadOnlyLocation => Some(ManagedBy::ReadOnlyLocation),
            InstallKind::Package => Some(ManagedBy::Package),
        }
    }

    fn apply(&mut self, input: Input, now: Timestamp, fx: &mut Vec<Effect>) -> Result<(), Refusal> {
        match input {
            Input::TimerFired => timers::timer_fired(self, now, fx),
            Input::CheckRequested => {
                if !self.may_check() {
                    return Err(Refusal::NotNow);
                }
                self.begin_check(true, fx);
            }
            Input::CheckDone(result) => {
                if self.state == UpdateState::Checking {
                    self.check_done(result, now, fx);
                }
            }
            Input::Progress(p) => {
                if let UpdateState::Downloading {
                    received, total, ..
                } = &mut self.state
                {
                    *received = Count::clamped(p.received);
                    *total = p.total.map(Count::clamped);
                }
            }
            Input::DownloadDone(result) => self.download_done(result, now, fx),
            Input::InstallRequested(when) => {
                // `scheduled` takes `now` too: "Restart now" upgrades the wait in place.
                let upgrade =
                    when == InstallWhen::Now && matches!(self.state, UpdateState::Scheduled { .. });
                if !upgrade && !matches!(self.state, UpdateState::Ready { .. }) {
                    return Err(Refusal::NotNow);
                }
                self.install(when, fx);
            }
            Input::CancelScheduled => self.cancel_scheduled(fx)?,
            Input::ModeSet(mode) => {
                if self.mode.locked {
                    return Err(Refusal::Locked);
                }
                if mode == UpdateMode::None {
                    return Err(Refusal::NotNow);
                }
                self.set_mode(
                    ModeInForce {
                        mode,
                        locked: false,
                    },
                    now,
                    fx,
                );
            }
            Input::PolicyChanged(mode) => self.set_mode(mode, now, fx),
            Input::TurnsRunning(running) => {
                self.turns_running = running;
                if !running && matches!(self.state, UpdateState::Scheduled { .. }) {
                    self.begin_install(true, fx);
                }
            }
            Input::TurnsCancelled => {
                if self.waiting_for_cancel() {
                    self.begin_install(false, fx);
                }
            }
        }
        Ok(())
    }

    fn may_check(&self) -> bool {
        matches!(self.state, UpdateState::Idle | UpdateState::Failed { .. })
    }

    /// `installing` under `now`, its turns cancelled but not all ended yet.
    fn waiting_for_cancel(&self) -> bool {
        self.install == InstallStep::StoppingTurns
    }

    fn begin_check(&mut self, asked: bool, fx: &mut Vec<Effect>) {
        self.asked = asked;
        self.state = UpdateState::Checking;
        self.timer = None;
        fx.push(Effect::StartCheck);
    }

    fn check_done(
        &mut self,
        result: Result<Option<Offer>, UpdaterError>,
        now: Timestamp,
        fx: &mut Vec<Effect>,
    ) {
        let install_at_start = std::mem::take(&mut self.install_at_start);
        let cached = self.cached.take();
        let offer = match result {
            Err(_) => {
                // Offline at start: no install now, but a later offer of the same version
                // makes it `ready` without a second download.
                self.cached = cached;
                return self.fail(None, FailedStage::Check, now, fx);
            }
            Ok(None) => {
                self.checked_at = Some(now);
                self.state = UpdateState::Idle;
                return timers::arm_interval(self, now, fx);
            }
            Ok(Some(offer)) => offer,
        };
        self.checked_at = Some(now);
        let Some(cached) = cached.filter(|c| c.version == offer.version) else {
            self.state = UpdateState::Downloading {
                version: offer.version.clone(),
                received: Count::ZERO,
                total: None,
            };
            return fx.push(Effect::StartDownload(offer));
        };
        self.ready(Downloaded {
            version: offer.version,
            path: cached.path,
            handle: offer.handle,
        });
        if install_at_start {
            self.install(InstallWhen::AfterTurn, fx);
        }
    }

    fn download_done(
        &mut self,
        result: Result<Downloaded, UpdaterError>,
        now: Timestamp,
        fx: &mut Vec<Effect>,
    ) {
        let UpdateState::Downloading { version, .. } = &self.state else {
            return;
        };
        if let Ok(downloaded) = result {
            self.ready(downloaded);
        } else {
            let version = Some(version.clone());
            self.fail(version, FailedStage::Download, now, fx);
        }
    }

    fn ready(&mut self, downloaded: Downloaded) {
        self.state = UpdateState::Ready {
            version: downloaded.version.clone(),
        };
        self.download = Some(downloaded);
    }

    fn fail(
        &mut self,
        version: Option<String>,
        stage: FailedStage,
        now: Timestamp,
        fx: &mut Vec<Effect>,
    ) {
        self.state = UpdateState::Failed {
            version,
            at: now,
            stage,
        };
        // Always when the lawyer asked; once a day in the background, so an offline laptop
        // counts one failure a day, not one an hour.
        if self.asked || once_a_day(&mut self.failure_counted, now) {
            fx.push(Effect::Count(UpdateResult::Failed));
        }
        timers::arm_interval(self, now, fx);
    }

    /// From `ready`, or `scheduled` with `now`: at once when no turn runs; else `now` stops the
    /// turns and holds new sends, and `after_turn` waits, letting new turns start and push it
    /// back.
    fn install(&mut self, when: InstallWhen, fx: &mut Vec<Effect>) {
        if !self.turns_running {
            return self.begin_install(false, fx);
        }
        let Some(version) = self.download.as_ref().map(|d| d.version.clone()) else {
            return;
        };
        match when {
            InstallWhen::Now => {
                self.state = UpdateState::Installing { version };
                self.install = InstallStep::StoppingTurns;
                fx.extend([Effect::GateNewTurns(true), Effect::CancelTurns]);
            }
            InstallWhen::AfterTurn => self.state = UpdateState::Scheduled { version },
        }
    }

    fn begin_install(&mut self, after_answer: bool, fx: &mut Vec<Effect>) {
        let Some(downloaded) = self.download.clone() else {
            return;
        };
        self.state = UpdateState::Installing {
            version: downloaded.version.clone(),
        };
        self.install = InstallStep::Begun;
        self.timer = None;
        fx.push(Effect::WriteMarker(UpdateMarker {
            from: self.current.clone(),
            to: downloaded.version.clone(),
            state: MarkerState::Installing,
        }));
        if after_answer {
            fx.push(Effect::Count(UpdateResult::AfterAnswer));
        }
        fx.push(Effect::BeginInstall(downloaded));
    }

    fn cancel_scheduled(&mut self, fx: &mut Vec<Effect>) -> Result<(), Refusal> {
        let waiting = self.waiting_for_cancel();
        if !waiting && !matches!(self.state, UpdateState::Scheduled { .. }) {
            return Err(Refusal::NotNow);
        }
        if let Some(downloaded) = self.download.take() {
            self.ready(downloaded);
        }
        self.ungate(fx);
        Ok(())
    }

    fn ungate(&mut self, fx: &mut Vec<Effect>) {
        if self.install == InstallStep::StoppingTurns {
            self.install = InstallStep::None;
            fx.push(Effect::GateNewTurns(false));
        }
    }

    fn set_mode(&mut self, mode: ModeInForce, now: Timestamp, fx: &mut Vec<Effect>) {
        let was = self.managed_by();
        self.mode = mode;
        // Only `auto` installs last run's download by itself; in `notify` it only saves the
        // download.
        if mode.mode != UpdateMode::Auto {
            self.install_at_start = false;
        }
        // The host is already shutting down to install: the new mode applies at the next start.
        if self.install == InstallStep::Begun {
            return;
        }
        match (was, self.managed_by()) {
            (Some(_), None) => {
                self.state = UpdateState::Idle;
                timers::arm_first_check(self, now, fx);
            }
            (was, Some(by)) if was != Some(by) => self.enter_managed(by, now, fx),
            (None | Some(_), None | Some(_)) => {}
        }
    }

    fn enter_managed(&mut self, by: ManagedBy, now: Timestamp, fx: &mut Vec<Effect>) {
        if matches!(self.state, UpdateState::Downloading { .. }) {
            fx.push(Effect::CancelDownload);
        }
        self.ungate(fx);
        self.download = None;
        self.cached = None;
        self.install_at_start = false;
        self.state = UpdateState::Managed { by };
        timers::count_managed(self, now, fx);
    }
}

/// One step: the next machine and its effects, with [`Effect::Publish`] last whenever the view
/// changed; or the refusal of a command the state does not take, which changes nothing.
pub(crate) fn step(
    state: &Machine,
    input: Input,
    now: Timestamp,
) -> Result<(Machine, Vec<Effect>), Refusal> {
    let before = state.view();
    let mut next = state.clone();
    let mut fx = Vec::new();
    next.apply(input, now, &mut fx)?;
    if next.view() != before {
        fx.push(Effect::Publish);
    }
    Ok((next, fx))
}
