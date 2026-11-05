//! The updater in the host (decision record 0037): the actor owns the pure `Machine`
//! (`update/machine.rs`), feeds it what happens and carries out what it asks.
//!
//! - **Start-up** reads `update.json` and last run's download on the file thread
//!   (`update/files.rs`), with `update.mode` in force and its lock from the settings and the
//!   install kind from the [`Updater`], then starts the machine. The host serves only once it
//!   has, so every command finds a machine.
//! - **Commands** map to inputs: `CheckForUpdate`, `InstallUpdate`, `CancelScheduledUpdate`
//!   answer `Done` with no receipt, as `InstallBrowser` does, since they act on state that
//!   lives in memory only; the answer goes out before any effect, so `InstallUpdate`'s reaches
//!   the window before the host starts to stop. `SetUpdateMode` writes `settings.json` first
//!   (`Effective::apply_update_mode`) and keeps a receipt as the other settings commands do.
//!   A refusal is `command.invalid`, or `settings.locked` for a policy-locked mode.
//! - **Effects** are in `update/effects.rs`: the [`Updater`]'s futures on the pool, one timer on
//!   the host clock, the marker on the file thread, Stop for every turn, the hold on new sends,
//!   `UpdateChanged`, the counts and the shutdown that ends in `Exit::Install`.
//! - **Turns.** After every input the loop calls [`tick`]: the machine hears `TurnsRunning`
//!   each time \"any turn running\" flips (`turn/install.rs` says what counts), and, after a
//!   `CancelTurns`, `TurnsCancelled` once no turn is left without its `TurnEnded`.
//!
//! The machine hears nothing once the host stops: a quit never turns into an install.

mod effects;
mod files;
mod turns;

use std::sync::Arc;

use clauseo_counts::schema::{Event, SettingChanged};
use clauseo_protocol::{
    ErrorCode, HostEvent, InstallWhen, ManagedBy, QueryResult, UpdateMode, UpdateState, UpdateView,
};
use clauseo_settings::{SystemFiles, UpdateChoice};
use futures::future::AbortHandle;

use super::commands::CommandCtx;
use super::input::Reply;
use super::{Actor, HostPhase, counting, settings, turn};
use crate::config::UpdateSeams;
use crate::error::Error;
use crate::update::machine::{Input, Jitter, Machine, ModeInForce, Refusal, Start, step};
use crate::update::{
    CachedDownload, Downloaded, Offer, Progress, UpdateMarker, Updater, UpdaterError,
};

pub(crate) use files::Done as FilesDone;

/// The settings key the machine's mode mirrors.
const MODE_KEY: &str = "update.mode";

/// What the updater's tasks and its file thread post back.
#[derive(Debug)]
pub(crate) enum UpdateInput {
    /// The timer armed with `generation` fired.
    Timer { generation: u64 },
    /// The check started with `generation` ended.
    CheckDone {
        generation: u64,
        result: Result<Option<Offer>, UpdaterError>,
    },
    /// The download started with `generation` posted its progress.
    Progress { generation: u64, progress: Progress },
    /// The download started with `generation` ended.
    DownloadDone {
        generation: u64,
        result: Result<Downloaded, UpdaterError>,
    },
    /// A file thread job ended.
    Files(FilesDone),
}

/// The updater's part of the actor.
pub(crate) struct UpdateHost {
    updater: Arc<dyn Updater>,
    current: String,
    seed: Option<u64>,
    /// What start-up's read found, until the host serves and the machine starts with it.
    read: Option<(Option<UpdateMarker>, Option<CachedDownload>)>,
    /// `None` until the host serves.
    machine: Option<Machine>,
    files: files::Files,
    /// File thread jobs not answered yet.
    files_out: usize,
    timer: Option<AbortHandle>,
    timer_generation: u64,
    check_generation: u64,
    download: Option<AbortHandle>,
    download_generation: u64,
    /// The last `TurnsRunning` the machine heard.
    turns_running: bool,
    /// A `CancelTurns` waits for its `TurnsCancelled`.
    cancelling: bool,
    /// What `BeginInstall` handed over, for `Exit::Install`.
    install: Option<Downloaded>,
}

/// Starts the updater's part at start-up: the file thread, and its read of `update.json` and
/// last run's download. Start-up waits for that read ([`busy`]); the machine starts with it
/// once the host serves ([`serving`]), so nothing it does (an install at start in `auto`)
/// can stop a host that has not answered `Host::start` yet.
pub(super) fn start(
    seams: UpdateSeams,
    locations: clauseo_settings::Locations,
    post: async_channel::Sender<super::input::Input>,
) -> UpdateHost {
    let UpdateSeams {
        updater,
        jitter_seed,
        app_version,
    } = seams;
    let mut host = UpdateHost {
        updater,
        current: app_version,
        seed: jitter_seed,
        read: None,
        machine: None,
        files: files::Files::start(locations, post),
        files_out: 0,
        timer: None,
        timer_generation: 0,
        check_generation: 0,
        download: None,
        download_generation: 0,
        turns_running: false,
        cancelling: false,
        install: None,
    };
    let current = host.current.clone();
    host.send(files::Job::Read { current });
    host
}

impl UpdateHost {
    /// Queues a file thread job, counted until its end is posted.
    fn send(&mut self, job: files::Job) {
        self.files_out += 1;
        self.files.send(job);
    }
}

/// Whether start-up or shutdown waits for the updater: a file thread job is still out (start-up's
/// read, or the install's marker).
pub(super) fn busy(actor: &Actor) -> bool {
    actor.update.files_out > 0
}

/// The host now serves: the machine starts from start-up's read.
pub(super) fn serving(actor: &mut Actor) {
    let (marker, cached) = actor.update.read.take().unwrap_or_default();
    start_machine(actor, marker, cached);
}

/// Whether new sends are held: an install under `now` waits for its cancelled turns.
pub(crate) fn gated(actor: &Actor) -> bool {
    actor.update.machine.as_ref().is_some_and(Machine::gated)
}

/// The download a `BeginInstall` handed over, once: the actor's exit is then `Exit::Install`.
pub(super) fn take_install(actor: &mut Actor) -> Option<Downloaded> {
    actor.update.install.take()
}

/// One input from the updater's tasks or its file thread.
pub(super) fn on_input(actor: &mut Actor, input: UpdateInput) {
    let update = &mut actor.update;
    match input {
        UpdateInput::Files(done) => {
            update.files_out = update.files_out.saturating_sub(1);
            match done {
                files::Done::Read { marker, cached } => update.read = Some((marker, cached)),
                files::Done::Wrote(written) => feed(actor, Input::MarkerWritten(written)),
                files::Done::Cleared => {}
            }
        }
        UpdateInput::Timer { generation } => {
            if generation == update.timer_generation {
                update.timer = None;
                feed(actor, Input::TimerFired);
            }
        }
        UpdateInput::CheckDone { generation, result } => {
            if generation == update.check_generation {
                effects::log_failure("check", result.as_ref().err());
                feed(actor, Input::CheckDone(result));
            }
        }
        UpdateInput::Progress {
            generation,
            progress,
        } => {
            if generation == update.download_generation && update.download.is_some() {
                feed(actor, Input::Progress(progress));
            }
        }
        UpdateInput::DownloadDone { generation, result } => {
            if generation == update.download_generation && update.download.take().is_some() {
                effects::log_failure("download", result.as_ref().err());
                feed(actor, Input::DownloadDone(result));
            }
        }
    }
}

/// Builds the machine from what start-up read: the mode in force and its lock, the install
/// kind, the marker, last run's download and the seed.
fn start_machine(actor: &mut Actor, marker: Option<UpdateMarker>, cached: Option<CachedDownload>) {
    if actor.update.machine.is_some() {
        return;
    }
    let now = actor.now();
    let seed = actor.update.seed.unwrap_or_else(|| {
        // Any spread will do: the seed only staggers the checks of many machines.
        let wall = now.wall.as_millis().unsigned_abs();
        wall ^ u64::try_from(now.mono.as_nanos())
            .unwrap_or(u64::MAX)
            .rotate_left(32)
    });
    let start = Start {
        current: actor.update.current.clone(),
        kind: actor.update.updater.install_kind(),
        mode: mode_in_force(actor),
        marker,
        cached,
        jitter: Jitter::new(seed),
    };
    let (machine, fx) = Machine::start(start, now.wall);
    actor.update.machine = Some(machine);
    effects::run(actor, fx);
}

/// `update.mode` in force, from the settings, and whether policy locks it.
fn mode_in_force(actor: &Actor) -> ModeInForce {
    let mode = match actor.effective.settings().update.mode {
        clauseo_settings::UpdateMode::Auto => UpdateMode::Auto,
        clauseo_settings::UpdateMode::Notify => UpdateMode::Notify,
        clauseo_settings::UpdateMode::None => UpdateMode::None,
    };
    ModeInForce {
        mode,
        locked: actor.effective.is_locked(MODE_KEY),
    }
}

/// After every input: the machine hears whether turns run, and whether a `CancelTurns` is
/// done. Only while the host serves.
pub(super) fn tick(actor: &mut Actor) {
    if !matches!(actor.phase, HostPhase::Serving) || actor.update.machine.is_none() {
        return;
    }
    let cancel = if !actor.update.cancelling {
        turns::Cancel::NotAsked
    } else if turn::any_active(actor) {
        turns::Cancel::TurnsLeft
    } else {
        turns::Cancel::AllEnded
    };
    let seen = turns::Seen {
        heard_running: actor.update.turns_running,
        running: turn::any_running(actor),
        cancel,
    };
    let heard = turns::to_hear(seen);
    actor.update.turns_running = seen.running;
    if heard.cancelled {
        actor.update.cancelling = false;
    }
    if let Some(running) = heard.running {
        feed(actor, Input::TurnsRunning(running));
    }
    if heard.cancelled {
        feed(actor, Input::TurnsCancelled);
    }
}

/// Steps the machine with `input` and carries out its effects. A refusal changes nothing (only
/// commands are refused, and they go through [`stepped`]).
fn feed(actor: &mut Actor, input: Input) {
    if !matches!(actor.phase, HostPhase::Serving) {
        return;
    }
    if let Ok(fx) = stepped(actor, input) {
        effects::run(actor, fx);
    }
}

/// Steps the machine with `input`, keeping the next machine; its effects are the caller's.
fn stepped(
    actor: &mut Actor,
    input: Input,
) -> Result<Vec<crate::update::machine::Effect>, Refusal> {
    let Some(machine) = actor.update.machine.as_ref() else {
        return Err(Refusal::NotNow);
    };
    let (next, fx) = step(machine, input, actor.now().wall)?;
    actor.update.machine = Some(next);
    Ok(fx)
}

/// An update command the machine takes as it is.
#[derive(Debug, Clone, Copy)]
pub(super) enum Asked {
    /// `CheckForUpdate`.
    Check,
    /// `InstallUpdate { when }`.
    Install(InstallWhen),
    /// `CancelScheduledUpdate`.
    CancelScheduled,
}

/// `CheckForUpdate`, `InstallUpdate` and `CancelScheduledUpdate`.
pub(super) fn command(actor: &mut Actor, ctx: &CommandCtx, asked: Asked) {
    // The machine must know whether turns run before it decides between `scheduled` and an
    // install at once.
    tick(actor);
    let input = match asked {
        Asked::Check => Input::CheckRequested,
        Asked::Install(when) => Input::InstallRequested(when),
        Asked::CancelScheduled => Input::CancelScheduled,
    };
    match stepped(actor, input) {
        Ok(fx) => {
            // Before any effect: `BeginInstall` stops the host, and a stopping host drops
            // answers.
            actor.finish_command(ctx.id, Ok(clauseo_protocol::CommandOutcome::Done));
            effects::run(actor, fx);
        }
        Err(refusal) => actor.finish_command(ctx.id, Err(refused(refusal))),
    }
}

/// `SetUpdateMode`: the machine's checks first (a lock, `none`), on a step whose result is
/// dropped; then `settings.json`; then the machine takes the mode the settings now hold
/// (`PolicyChanged`), `SettingsChanged` goes out and the receipt is committed. A failed write
/// changes nothing.
pub(super) fn set_mode(actor: &mut Actor, ctx: &CommandCtx, mode: UpdateMode) {
    tick(actor);
    let checked = actor
        .update
        .machine
        .as_ref()
        .ok_or(Refusal::NotNow)
        .and_then(|machine| step(machine, Input::ModeSet(mode), actor.now().wall));
    if let Err(refusal) = checked {
        return actor.finish_command(ctx.id, Err(refused(refusal)));
    }
    let choice = match mode {
        UpdateMode::Auto => UpdateChoice::Auto,
        UpdateMode::Notify => UpdateChoice::Notify,
        // The machine refuses `none` above.
        UpdateMode::None => return actor.finish_command(ctx.id, Err(refused(Refusal::NotNow))),
    };
    let before = actor.effective.settings().update.mode;
    if let Err(error) = actor.effective.apply_update_mode(&SystemFiles, choice) {
        return actor.finish_command(ctx.id, Err(Error::from(error)));
    }
    feed(actor, Input::PolicyChanged(mode_in_force(actor)));
    if before != actor.effective.settings().update.mode {
        counting::bump(
            actor,
            Event::SettingChanged(SettingChanged::UpdateMode(counted_mode(mode))),
        );
        let changed = HostEvent::SettingsChanged(settings::view(actor));
        super::app::publish(actor, &changed);
    }
    if let Err(error) = settings::receipt(actor, ctx) {
        actor.finish_command(ctx.id, Err(error));
    }
}

/// The counts' value of a mode.
fn counted_mode(mode: UpdateMode) -> clauseo_counts::schema::UpdateMode {
    match mode {
        UpdateMode::Auto => clauseo_counts::schema::UpdateMode::Auto,
        UpdateMode::Notify => clauseo_counts::schema::UpdateMode::Notify,
        UpdateMode::None => clauseo_counts::schema::UpdateMode::None,
    }
}

/// The wire's refusal of a command the machine refused.
fn refused(refusal: Refusal) -> Error {
    match refusal {
        Refusal::NotNow => Error::refused(
            ErrorCode::CommandInvalid,
            "the updater's state does not take this command now",
        ),
        Refusal::Locked => Error::refused(
            ErrorCode::SettingsLocked,
            format!("{MODE_KEY} is set by policy"),
        ),
    }
}

/// `GetUpdate`: the machine's view.
pub(super) fn answer(actor: &Actor, reply: Reply<QueryResult>) {
    let view = actor.update.machine.as_ref().map_or_else(
        // Not reached while the host serves: it serves once the machine has started.
        || UpdateView {
            current: actor.update.current.clone(),
            state: UpdateState::Managed {
                by: ManagedBy::Package,
            },
            mode: mode_in_force(actor).mode,
            checked_at: None,
            updated_from: None,
        },
        Machine::view,
    );
    let _gone = reply.send(Ok(QueryResult::Update(view)));
}

/// Whether this run started after an install took (`app_opened`'s `after_update`).
pub(super) fn updated(actor: &Actor) -> bool {
    actor
        .update
        .machine
        .as_ref()
        .is_some_and(|machine| machine.view().updated_from.is_some())
}
