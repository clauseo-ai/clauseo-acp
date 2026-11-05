//! The machine's effects, carried out in order (`actor/update.rs`). Nothing here waits: the
//! [`Updater`](crate::update::Updater)'s futures run on the pool and post their results, the
//! timer is the host clock's, and the marker goes to the file thread.

use std::time::Duration;

use clauseo_counts::schema::{Event, Update, UpdateResult};
use clauseo_protocol::HostEvent;
use futures::FutureExt;
use futures::StreamExt;
use futures::future::{AbortHandle, Abortable, Either};
use futures::task::LocalSpawnExt;

use super::files::Job;
use super::{UpdateInput, turn};
use crate::actor::input::Input;
use crate::actor::{Actor, app, counting, shutdown};
use crate::update::machine::Effect;
use crate::update::{Offer, ProgressSink, UpdaterError};

/// How many progress values wait between the download and the actor; the rest are dropped,
/// and the next carries the newer total anyway.
const PROGRESS_CAPACITY: usize = 16;

/// Carries out `fx`, in order.
pub(super) fn run(actor: &mut Actor, fx: Vec<Effect>) {
    for effect in fx {
        match effect {
            Effect::StartCheck => start_check(actor),
            Effect::StartDownload(offer) => start_download(actor, offer),
            Effect::CancelDownload => {
                // Dropping the download's future cancels it (the desktop's adapter aborts its
                // task when its receiver goes).
                if let Some(download) = actor.update.download.take() {
                    download.abort();
                }
            }
            Effect::ArmTimer { at } => arm(actor, at),
            Effect::CancelTurns => {
                actor.update.cancelling = true;
                turn::cancel_all(actor);
                // Answered even when no turn was left to cancel, so a `now` install never
                // holds sends for ever: here when none is, else by `tick` once the last ends.
                super::tick(actor);
            }
            Effect::GateNewTurns(true) => {
                // The machine's own state is the hold (`update::gated`), read by the queue.
            }
            Effect::GateNewTurns(false) => {
                actor.update.cancelling = false;
                turn::release(actor);
            }
            Effect::WriteMarker(marker) => actor.update.send(Job::Write(marker)),
            Effect::ClearMarker => actor.update.send(Job::Clear),
            Effect::BeginInstall(downloaded) => {
                // The marker is on disk (the machine waits for `MarkerWritten`), and the command
                // that asked has had its answer: the shutdown drops later ones.
                actor.update.install = Some(downloaded);
                shutdown::begin_shutdown(actor, None);
            }
            Effect::Publish => {
                if let Some(machine) = &actor.update.machine {
                    let changed = HostEvent::UpdateChanged(machine.view());
                    app::publish(actor, &changed);
                }
            }
            Effect::Count(result) => count(actor, result),
        }
    }
}

/// Counts `update { result }`. `managed` counts once a day across starts too: the machine's
/// once-a-day rule lives in memory, so the open day's counts say whether an earlier run already
/// counted it. A background `failed` stays once a day per run (the machine's rule): it shares
/// its key with the failures the lawyer asked for, which always count.
fn count(actor: &Actor, result: UpdateResult) {
    let update = Update { result };
    let counted_today = result == UpdateResult::Managed
        && actor
            .counts
            .as_ref()
            .is_some_and(|counts| counts.today().update.iter().any(|cell| cell.key == update));
    if !counted_today {
        counting::bump(actor, Event::Update(update));
    }
}

/// The check runs on the pool; its result comes back as [`UpdateInput::CheckDone`].
fn start_check(actor: &mut Actor) {
    actor.update.check_generation += 1;
    let generation = actor.update.check_generation;
    let check = actor.update.updater.check();
    actor.spawn_post(async move {
        Input::Update(UpdateInput::CheckDone {
            generation,
            result: check.await,
        })
    });
}

/// The download runs on the pool beside the pump that posts its progress; aborting it drops
/// both, so nothing more is posted. Its result comes back as [`UpdateInput::DownloadDone`].
fn start_download(actor: &mut Actor, offer: Offer) {
    if let Some(earlier) = actor.update.download.take() {
        earlier.abort();
    }
    actor.update.download_generation += 1;
    let generation = actor.update.download_generation;
    let (sink, progress) = ProgressSink::channel(PROGRESS_CAPACITY);
    let download = actor.update.updater.download(offer, sink);
    let post = actor.post.clone();
    let pump = {
        let post = post.clone();
        async move {
            let mut progress = progress;
            while let Some(mut latest) = progress.next().await {
                // Only the newest of what has piled up matters; the sink holds at most about
                // this many.
                for _ in 0..PROGRESS_CAPACITY {
                    match progress.next().now_or_never() {
                        Some(Some(newer)) => latest = newer,
                        Some(None) | None => break,
                    }
                }
                let posted = post.try_send(Input::Update(UpdateInput::Progress {
                    generation,
                    progress: latest,
                }));
                if posted.is_err() {
                    break;
                }
                // A download that posts without pause must not keep this task, and with it the
                // actor's thread, busy: let the pool run the actor before the next value.
                YieldNow(false).await;
            }
            // The sink is gone: wait for the download's own end.
            std::future::pending::<()>().await;
        }
    };
    let (handle, registration) = AbortHandle::new_pair();
    let task = Abortable::new(
        async move {
            let download = std::pin::pin!(download);
            let pump = std::pin::pin!(pump);
            match futures::future::select(download, pump).await {
                Either::Left((result, _pump)) => result,
                Either::Right(((), download)) => download.await,
            }
        },
        registration,
    );
    let spawned = actor.spawner.spawn_local(async move {
        if let Ok(result) = task.await {
            let _gone = post.try_send(Input::Update(UpdateInput::DownloadDone {
                generation,
                result,
            }));
        }
    });
    match spawned {
        Ok(()) => actor.update.download = Some(handle),
        Err(error) => tracing::error!(%error, "the host pool refused the update download"),
    }
}

/// Pending once, waking itself: the pool runs its other tasks (the actor) before this one goes
/// on.
struct YieldNow(bool);

impl std::future::Future for YieldNow {
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<()> {
        if self.0 {
            return std::task::Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        std::task::Poll::Pending
    }
}

/// Arms the one timer at the wall-clock time `at`, replacing the last. An early fire is the
/// machine's to judge: it arms again.
fn arm(actor: &mut Actor, at: clauseo_protocol::Timestamp) {
    if let Some(earlier) = actor.update.timer.take() {
        earlier.abort();
    }
    actor.update.timer_generation += 1;
    let generation = actor.update.timer_generation;
    let wait = at.as_millis().saturating_sub(actor.now().wall.as_millis());
    let delay = Duration::from_millis(u64::try_from(wait).unwrap_or(0));
    let handle = actor.post_after(delay, Input::Update(UpdateInput::Timer { generation }));
    actor.update.timer = Some(handle);
}

/// Logs a failed check or download by its kind alone: the text may hold a path or a URL
/// (decision record 0047).
pub(super) fn log_failure(stage: &'static str, error: Option<&UpdaterError>) {
    let Some(error) = error else {
        return;
    };
    let kind = match error {
        UpdaterError::Network(_) => "network",
        UpdaterError::Signature(_) => "signature",
        UpdaterError::Io(_) => "io",
        UpdaterError::Other(_) => "other",
    };
    tracing::warn!(stage, kind, "an update step failed");
}
