//! The folder scan before a matter (`ScanFolder`, `CancelScan`), read off the actor. A
//! short-lived `clauseo-scan` thread tells where the folder lives (`clauseo_matters::place`)
//! and walks it (`clauseo_matters::scan_folder`), posting what it finds to the actor, which
//! publishes it as `HostEvent::FolderScanned` on the app stream. No file system call runs on
//! the actor, and nothing is stored: not even a receipt, so a repeated `command_id` scans again.
//!
//! One scan at a time: a `ScanFolder` sets the cancel flag of the scan running before it starts
//! its own. A cancelled scan's thread stops at the walk's next check (before each folder, entry
//! and stat) and its last event says `cancelled`, unless a newer scan of the same folder has
//! started: then it ends without an event (`ended`). Progress reaches the actor through one slot
//! the thread overwrites, so at most one progress input per scan waits in the channel however
//! fast the walk runs. The actor publishes at most one event per [`THROTTLE`] on the host clock
//! and always the last (`done`), which also answers the scan's command. On shutdown every flag
//! is set and the thread is left to end on its own: nothing waits for it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use clauseo_matters::{FolderScan, Place, ScanProgress, Settings};
use clauseo_protocol::{
    CommandId, CommandOutcome, Count, ErrorCode, FolderPlace, FolderRefId, FolderScanned,
    HostEvent, MatterPath,
};

use super::Actor;
use super::commands::CommandCtx;
use super::input::Input;
use crate::error::Error;
use crate::matter_worker::matter_settings;

/// The least time between two progress events of a scan.
pub(crate) const THROTTLE: Duration = Duration::from_millis(250);

/// The name of the scan threads.
const THREAD_NAME: &str = "clauseo-scan";

/// The walk a scan runs: [`clauseo_matters::scan_folder`], or a stand-in in tests.
pub(crate) type Walk = Arc<
    dyn Fn(
            &Path,
            &[PathBuf],
            &Settings,
            &mut dyn FnMut(ScanProgress<'_>),
            &AtomicBool,
        ) -> Result<FolderScan, clauseo_matters::Error>
        + Send
        + Sync,
>;

/// The walk the host runs.
pub(crate) fn os_walk() -> Walk {
    Arc::new(|folder, refused, settings, progress, cancel| {
        clauseo_matters::scan_folder(folder, refused, settings, progress, cancel)
    })
}

/// The counts a scan's thread found last, waiting for the actor to take them.
#[derive(Debug, Clone)]
struct Counts {
    files: u64,
    bytes: u64,
    cloud_only: u64,
    now_reading: MatterPath,
}

/// The slot a scan's thread overwrites with its latest counts.
type Latest = Arc<Mutex<Option<Counts>>>;

/// What a scan's thread posts.
pub(crate) enum ScanInput {
    /// Where the folder lives; the first input of a scan.
    Place { id: u64, place: FolderPlace },
    /// New counts are in the scan's slot.
    Progress { id: u64 },
    /// The walk ended.
    Ended {
        id: u64,
        result: Result<FolderScan, clauseo_matters::Error>,
    },
    /// The throttle's timer fired.
    Tick { id: u64 },
}

/// The scans the actor knows: the current one and those cancelled whose thread has not ended.
#[derive(Default)]
pub(crate) struct Scans {
    next: u64,
    current: Option<u64>,
    running: BTreeMap<u64, Scan>,
    /// The newest scan started of each folder that still has a scan running, so a superseded
    /// scan of the same folder ends without a word ([`ended`]).
    newest: BTreeMap<FolderRefId, u64>,
    /// The last event published, for an app subscriber that lagged ([`catch_up`]).
    last: Option<FolderScanned>,
}

/// One scan whose thread has not ended.
struct Scan {
    command: CommandId,
    cancel: Arc<AtomicBool>,
    latest: Latest,
    /// The event as it stands.
    event: FolderScanned,
    throttle: Throttle,
}

/// When a scan's progress may be published: at most once per [`THROTTLE`].
#[derive(Debug, Default)]
pub(crate) struct Throttle {
    /// When the last event went out, on the host's monotonic clock.
    sent_at: Option<Duration>,
    /// An event is waiting for the timer.
    held: bool,
    /// The timer is armed.
    armed: bool,
}

/// What to do with new progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Offer {
    /// Publish it now.
    Send,
    /// Keep it; the armed timer publishes it.
    Hold,
    /// Keep it and arm the timer for this delay.
    Arm(Duration),
}

impl Throttle {
    /// New progress at `now`.
    pub(crate) fn offer(&mut self, now: Duration) -> Offer {
        match self.sent_at {
            Some(at) if now < at.saturating_add(THROTTLE) => {
                self.held = true;
                if self.armed {
                    Offer::Hold
                } else {
                    self.armed = true;
                    Offer::Arm(at.saturating_add(THROTTLE).saturating_sub(now))
                }
            }
            _ => {
                self.sent_at = Some(now);
                self.held = false;
                Offer::Send
            }
        }
    }

    /// The timer fired at `now`: whether to publish what is held.
    pub(crate) fn tick(&mut self, now: Duration) -> bool {
        self.armed = false;
        let send = self.held;
        if send {
            self.held = false;
            self.sent_at = Some(now);
        }
        send
    }
}

/// `ScanFolder`: cancels the scan running, then starts one of `folder` on its own thread. An
/// unknown folder ref is `command.invalid`; the command is answered when the walk ends.
pub(super) fn scan_folder(actor: &mut Actor, ctx: &CommandCtx, folder: FolderRefId) {
    if let Err(error) = start(actor, ctx, folder) {
        actor.finish_command(ctx.id, Err(error));
    }
}

fn start(actor: &mut Actor, ctx: &CommandCtx, folder: FolderRefId) -> Result<(), Error> {
    let path = actor.folders.get(&folder).cloned().ok_or_else(|| {
        Error::refused(ErrorCode::CommandInvalid, format!("no folder ref {folder}"))
    })?;
    if let Some(current) = actor.scans.current.take()
        && let Some(scan) = actor.scans.running.get(&current)
    {
        scan.cancel.store(true, Ordering::Relaxed);
    }
    let id = actor.scans.next;
    actor.scans.next += 1;
    let cancel = Arc::new(AtomicBool::new(false));
    let latest = Latest::default();
    let job = Job {
        id,
        path,
        // The same refusals import makes: the configured folders here, the data folder and
        // the matters root through the settings.
        refused: actor.refused_folders.clone(),
        settings: matter_settings(actor.effective.data_dir()),
        walk: Arc::clone(&actor.scan_walk),
        cancel: Arc::clone(&cancel),
        latest: Arc::clone(&latest),
        post: actor.post.clone(),
    };
    std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || job.run())
        .map_err(|error| Error::Internal(format!("starting {THREAD_NAME}: {error}")))?;
    let event = FolderScanned {
        folder,
        files: 0,
        bytes: Count::ZERO,
        now_reading: None,
        place: FolderPlace::Local,
        cloud_only: 0,
        done: false,
        cancelled: false,
    };
    let scan = Scan {
        command: ctx.id,
        cancel,
        latest,
        event,
        throttle: Throttle::default(),
    };
    actor.scans.running.insert(id, scan);
    actor.scans.newest.insert(folder, id);
    actor.scans.current = Some(id);
    Ok(())
}

/// `CancelScan`: sets the flag of every running scan of `folder`; `Done` either way, at once.
pub(super) fn cancel_scan(actor: &mut Actor, ctx: &CommandCtx, folder: FolderRefId) {
    for scan in actor.scans.running.values() {
        if scan.event.folder == folder {
            scan.cancel.store(true, Ordering::Relaxed);
        }
    }
    actor.finish_command(ctx.id, Ok(CommandOutcome::Done));
}

/// Shutdown: sets every scan's flag. Nothing waits for the threads.
pub(super) fn cancel_all(actor: &Actor) {
    for scan in actor.scans.running.values() {
        scan.cancel.store(true, Ordering::Relaxed);
    }
}

/// One input from a scan's thread or timer.
pub(super) fn on_input(actor: &mut Actor, input: ScanInput) {
    match input {
        ScanInput::Place { id, place } => {
            if let Some(scan) = actor.scans.running.get_mut(&id) {
                scan.event.place = place;
                progressed(actor, id);
            }
        }
        ScanInput::Progress { id } => {
            if let Some(scan) = actor.scans.running.get_mut(&id) {
                take_latest(scan);
                progressed(actor, id);
            }
        }
        ScanInput::Tick { id } => {
            let now = actor.now().mono;
            let Some(scan) = actor.scans.running.get_mut(&id) else {
                return;
            };
            take_latest(scan);
            if scan.throttle.tick(now) && actor.scans.current == Some(id) {
                let event = scan.event.clone();
                publish(actor, event);
            }
        }
        ScanInput::Ended { id, result } => ended(actor, id, result),
    }
}

/// Moves the counts in `scan`'s slot into its event.
fn take_latest(scan: &mut Scan) {
    let taken = scan
        .latest
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    if let Some(counts) = taken {
        set_counts(
            &mut scan.event,
            counts.files,
            counts.bytes,
            counts.cloud_only,
        );
        scan.event.now_reading = Some(counts.now_reading);
    }
}

fn set_counts(event: &mut FolderScanned, files: u64, bytes: u64, cloud_only: u64) {
    event.files = u32::try_from(files).unwrap_or(u32::MAX);
    event.bytes = Count::clamped(bytes);
    event.cloud_only = u32::try_from(cloud_only).unwrap_or(u32::MAX);
}

/// Scan `id`'s event changed: publish it now, or hold it for the throttle's timer. Only the
/// current scan's progress goes out; a cancelled one's only says it ended.
fn progressed(actor: &mut Actor, id: u64) {
    if actor.scans.current != Some(id) {
        return;
    }
    let now = actor.now().mono;
    let Some(scan) = actor.scans.running.get_mut(&id) else {
        return;
    };
    match scan.throttle.offer(now) {
        Offer::Send => {
            let event = scan.event.clone();
            publish(actor, event);
        }
        Offer::Hold => {}
        Offer::Arm(delay) => {
            let after = actor.clock.after(delay);
            actor.spawn_post(async move {
                after.await;
                Input::Scan(ScanInput::Tick { id })
            });
        }
    }
}

/// Scan `id`'s walk ended: its last event, then its command's answer. Once a newer scan of the
/// same folder has started, this one's end is not published and the catch-up keeps the newer
/// one's events: `FolderScanned` names no scan, so a late cancelled end would read as that
/// folder's last word. Its command is answered either way.
fn ended(actor: &mut Actor, id: u64, result: Result<FolderScan, clauseo_matters::Error>) {
    let Some(mut scan) = actor.scans.running.remove(&id) else {
        return;
    };
    if actor.scans.current == Some(id) {
        actor.scans.current = None;
    }
    let folder = scan.event.folder;
    let superseded = actor
        .scans
        .newest
        .get(&folder)
        .is_some_and(|newest| *newest > id);
    if !actor
        .scans
        .running
        .values()
        .any(|running| running.event.folder == folder)
    {
        actor.scans.newest.remove(&folder);
    }
    take_latest(&mut scan);
    let mut event = scan.event;
    event.now_reading = None;
    event.done = true;
    let answer = match result {
        Ok(FolderScan::Done(totals)) => {
            set_counts(&mut event, totals.files, totals.bytes, totals.cloud_only);
            Ok(CommandOutcome::Done)
        }
        Ok(FolderScan::Cancelled) => {
            event.cancelled = true;
            Ok(CommandOutcome::Done)
        }
        Err(error) => Err(Error::from(error)),
    };
    if !superseded {
        publish(actor, event);
    }
    actor.finish_command(scan.command, answer);
}

/// Publishes `event` and keeps it for [`catch_up`].
///
/// `FolderScanned` is an app-stream event, and the room is its only reader: the desktop passes
/// it to `main` alone, never to the Settings window (`AppEvents`, 0028). The host throttles it to
/// at most one per 250 ms ([`THROTTLE`]) per scan, plus the last, so a large folder never floods
/// the window (`scan/tests.rs`, the throttle tests).
fn publish(actor: &mut Actor, event: FolderScanned) {
    let wire = HostEvent::FolderScanned(event);
    super::app::publish(actor, &wire);
    if let HostEvent::FolderScanned(event) = wire {
        actor.scans.last = Some(event);
    }
}

/// The last scan event, for an app subscriber that lagged: it replaces what a window holds of
/// that scan whole, so the last `done` is never lost to a full channel.
pub(super) fn catch_up(actor: &Actor) -> Option<HostEvent> {
    actor.scans.last.clone().map(HostEvent::FolderScanned)
}

/// The protocol's place for `matters`'.
fn folder_place(place: Place) -> FolderPlace {
    match place {
        Place::Local => FolderPlace::Local,
        Place::Network => FolderPlace::Network,
        Place::Synced { service } => FolderPlace::Synced { service },
    }
}

/// What a scan's thread needs.
struct Job {
    id: u64,
    path: PathBuf,
    refused: Vec<PathBuf>,
    settings: Settings,
    walk: Walk,
    cancel: Arc<AtomicBool>,
    latest: Latest,
    post: async_channel::Sender<Input>,
}

impl Job {
    /// The thread's body. A post to an actor that is gone fails quietly.
    fn run(self) {
        let Self {
            id,
            path,
            refused,
            settings,
            walk,
            cancel,
            latest,
            post,
        } = self;
        let send = |input: ScanInput| {
            let _gone = post.try_send(Input::Scan(input));
        };
        let place = folder_place(clauseo_matters::place(&path));
        send(ScanInput::Place { id, place });
        let mut progress = |step: ScanProgress<'_>| {
            let counts = Counts {
                files: step.files,
                bytes: step.bytes,
                cloud_only: step.cloud_only,
                now_reading: step.now_reading.clone(),
            };
            let was_empty = latest
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .replace(counts)
                .is_none();
            if was_empty {
                send(ScanInput::Progress { id });
            }
        };
        let result = walk(&path, &refused, &settings, &mut progress, &cancel);
        send(ScanInput::Ended { id, result });
    }
}

#[cfg(test)]
mod tests;
