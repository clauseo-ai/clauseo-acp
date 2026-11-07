//! Where the allowed sites are saved from the actor's side (decision record 0053, PR 2): one
//! thread of their own (`blocklist/allowed/integrity.rs`'s `run`) reads them at the start and then
//! makes each save in order, so a Keychain prompt never holds up the actor.
//!
//! - Start: every view empty and the query listing none until the read is back; an allow or
//!   revoke sent meanwhile is held, then run in order. A thread the OS refuses, or a read not
//!   back within [`READ_WAIT`] (a prompt left open), counts as no store for the run: no stored
//!   allows, and changes apply in memory only, logged once; a read that lands later is ignored.
//! - Saves: each allow or revoke sends the whole state as one job and is answered when the
//!   thread reports it, in order: `Done` when saved; else `blocklist.allow_not_saved` for an
//!   allow, and for a revoke `blocklist.revoke_not_saved` unless the stored pair was voided (or
//!   never held the site), so the site cannot come back at the next start.
//! - Stop: the host waits up to [`STOP_WAIT`] (host clock) for the queue to drain. A revoke
//!   still unsaved then deletes the stored hash as the stop's last step, on a thread of its own
//!   waited on for at most [`VOID_WAIT`], so the next start honours no allows rather than the
//!   revoked site; allows still queued are lost (they were not saved). The thread is told to
//!   write nothing more first, so a save it finishes later cannot make a matching pair again.
//! - `persisted` follows what the next start would honour (each entry's `saved`): a saved
//!   job's state, nothing once the pair is voided, unknown while the store has not answered.

use std::sync::{Arc, mpsc};
use std::time::Duration;

use clauseo_connectors::SecretStore;
use clauseo_protocol::{CommandId, CommandOutcome, ErrorCode};

use super::{Actor, Change, Held, refresh, run_allow, run_revoke};
use crate::actor::input::Input;
use crate::blocklist::AllowedSites;
use crate::blocklist::allowed::file::Stored;
use crate::blocklist::allowed::integrity::{self, Job, Opened, Report, Wrote};
use crate::error::Error;

/// The thread's name.
const THREAD_NAME: &str = "clauseo-allowed";
/// The stop's delete's thread.
const VOID_THREAD: &str = "clauseo-allowed-void";
/// How long the start-up read may take before the run goes on without the store.
pub(crate) const READ_WAIT: Duration = Duration::from_secs(30);
/// How long the host's stop waits for the queued saves.
pub(crate) const STOP_WAIT: Duration = Duration::from_secs(2);
/// How long the stop's last delete of the hash is waited on.
pub(crate) const VOID_WAIT: Duration = Duration::from_secs(1);

/// An allow not saved.
pub(crate) const ALLOW_NOT_SAVED: &str = "the allow applies until Clauseo closes; it was not saved";
/// A revoke not saved, while the stored pair may still hold the site.
pub(crate) const REVOKE_NOT_SAVED: &str =
    "the revoke applies until Clauseo closes; the site may be allowed again after a restart";

/// What reaches the actor about the allowed sites' storage.
#[derive(Debug)]
pub(crate) enum AllowedInput {
    /// From the thread.
    Report(Report),
    /// [`READ_WAIT`] passed since the start.
    ReadTimedOut,
    /// [`STOP_WAIT`] passed since the stop began, or the queue drained first.
    StopWaited,
    /// The stop's delete of the hash: whether it went, `None` past [`VOID_WAIT`].
    StopVoided(Option<bool>),
}

/// The secret store, kept for the stop's last delete; `Debug` names nothing in it.
#[derive(Clone)]
pub(crate) struct Secrets(Arc<dyn SecretStore>);

impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secrets")
    }
}

/// Where the host's stop is with the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stop {
    /// No stop yet, or nothing to wait for.
    Running,
    /// Waiting for the queue to drain.
    Draining,
    /// Deleting the hash, as the last step.
    Voiding,
}

/// At the start: `sites` (the seams' views, or a fresh one), empty, and the thread, which reads
/// against `secrets` and then saves; plus the read's deadline.
pub(crate) fn start(
    actor: &mut Actor,
    (sites, secrets): (Arc<AllowedSites>, Option<Arc<dyn SecretStore>>),
) {
    actor.allowed.sites = sites;
    refresh(actor);
    let path = actor.allowed.path.clone();
    let post = actor.post.clone();
    actor.allowed.store = secrets.clone().map(Secrets);
    let abandoned = Arc::clone(&actor.allowed.abandoned);
    let (jobs, queue) = mpsc::channel::<Job>();
    let started = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            integrity::run(&path, secrets, abandoned, &queue, |report| {
                // The host may have stopped; nobody is left to tell.
                let input = AllowedInput::Report(report);
                let _gone = post.try_send(Input::Allowed(Box::new(input)));
            });
        });
    if let Err(error) = started {
        tracing::warn!(
            %error,
            "blocklist: no thread for allowed.json; no allows, and allows made now apply until \
             Clauseo stops"
        );
        actor.allowed.sites.mark_loaded();
        return;
    }
    actor.allowed.writer = Some(jobs);
    actor.allowed.opening = Some(Vec::new());
    // The deadline ends early once the read is applied (`apply` drops `stop`), so no timer is
    // left armed; the input it then posts is ignored.
    let deadline = actor.clock.after(READ_WAIT);
    let (stop, stopped) = futures::channel::oneshot::channel::<()>();
    actor.allowed.read_deadline = Some(stop);
    actor.spawn_post(async move {
        let _either = futures::future::select(deadline, stopped).await;
        Input::Allowed(Box::new(AllowedInput::ReadTimedOut))
    });
}

/// An [`AllowedInput`] on the actor.
pub(crate) fn on_input(actor: &mut Actor, input: AllowedInput) {
    match input {
        AllowedInput::Report(Report::Opened(opened)) => on_opened(actor, opened),
        AllowedInput::Report(Report::Wrote(wrote)) => on_wrote(actor, wrote),
        AllowedInput::StopWaited => on_stop_waited(actor),
        AllowedInput::StopVoided(gone) => {
            match gone {
                Some(true) => tracing::warn!(
                    "blocklist: a revoke was still unsaved at the stop; allowed.json's hash \
                     deleted, so no allows at the next start"
                ),
                Some(false) => tracing::warn!(
                    "blocklist: a revoke was still unsaved at the stop and the hash could not \
                     be deleted; the site may be allowed again at the next start"
                ),
                None => tracing::warn!(
                    wait = ?VOID_WAIT,
                    "blocklist: a revoke was still unsaved at the stop and the hash's delete \
                     did not answer in time; the site may be allowed again at the next start"
                ),
            }
            actor.allowed.stop = Stop::Running;
        }
        AllowedInput::ReadTimedOut => {
            if actor.allowed.opening.is_none() {
                return;
            }
            tracing::warn!(
                wait = ?READ_WAIT,
                "blocklist: the secret store did not answer in time; no allows, and allows made \
                 now apply until Clauseo stops"
            );
            // Dropping the queue ends the thread once its read lands; nothing it reads is used.
            actor.allowed.writer = None;
            let unknown = Opened {
                stored: Stored::default(),
                persisted: None,
            };
            apply(actor, unknown);
        }
    }
}

/// The read is back: applied unless the run already gave up on it.
fn on_opened(actor: &mut Actor, opened: Opened) {
    if actor.allowed.opening.is_some() {
        apply(actor, opened);
    }
}

/// `opened` in place and published, then each held command run in order.
fn apply(actor: &mut Actor, opened: Opened) {
    let held = actor.allowed.opening.take().unwrap_or_default();
    actor.allowed.read_deadline = None;
    actor.allowed.stored = opened.stored;
    actor.allowed.persisted = opened.persisted;
    refresh(actor);
    actor.allowed.sites.mark_loaded();
    for command in held {
        match command {
            Held::Allow {
                id,
                block_id,
                scope,
            } => run_allow(actor, id, block_id, scope),
            Held::Revoke {
                id,
                host,
                scope,
                matter,
            } => run_revoke(actor, id, &host, scope, matter),
        }
    }
}

/// Sends the state now as a job for command `id`, answered when the thread reports; at once,
/// not saved, without a thread.
pub(super) fn save(actor: &mut Actor, id: CommandId, change: Change) {
    let stored = actor.allowed.stored.clone();
    let job = Job {
        stored: stored.clone(),
        revoke: matches!(change, Change::Revoke { .. }),
        now: actor.now().wall,
    };
    let sent = actor
        .allowed
        .writer
        .as_ref()
        .is_some_and(|writer| writer.send(job).is_ok());
    if !sent {
        actor.allowed.writer = None;
        answer(
            actor,
            id,
            &change,
            Wrote::NotSaved { voided: false },
            stored,
        );
        return;
    }
    actor.allowed.waiting.push_back((id, change, stored));
}

/// The oldest job's outcome; the stop's wait ends once the queue is empty.
fn on_wrote(actor: &mut Actor, wrote: Wrote) {
    if let Some((id, change, stored)) = actor.allowed.waiting.pop_front() {
        answer(actor, id, &change, wrote, stored);
    }
    if actor.allowed.stop == Stop::Draining && actor.allowed.waiting.is_empty() {
        actor.allowed.stop = Stop::Running;
        actor.allowed.stop_deadline = None;
    }
}

/// At the host's stop: waits up to [`STOP_WAIT`] for queued saves, if any.
pub(crate) fn shutdown(actor: &mut Actor) {
    if actor.allowed.waiting.is_empty() || actor.allowed.stop != Stop::Running {
        return;
    }
    actor.allowed.stop = Stop::Draining;
    let deadline = actor.clock.after(STOP_WAIT);
    let (stop, stopped) = futures::channel::oneshot::channel::<()>();
    actor.allowed.stop_deadline = Some(stop);
    actor.spawn_post(async move {
        let _either = futures::future::select(deadline, stopped).await;
        Input::Allowed(Box::new(AllowedInput::StopWaited))
    });
}

/// Whether the stop still waits on the queue or its last delete.
pub(crate) fn busy(actor: &Actor) -> bool {
    actor.allowed.stop != Stop::Running
}

/// The stop's wait is over: the thread writes nothing more; an unsaved revoke deletes the
/// hash, waited on for at most [`VOID_WAIT`].
fn on_stop_waited(actor: &mut Actor) {
    if actor.allowed.stop != Stop::Draining {
        return;
    }
    actor.allowed.stop_deadline = None;
    actor
        .allowed
        .abandoned
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let revoke = actor
        .allowed
        .waiting
        .iter()
        .any(|(_, change, _)| matches!(change, Change::Revoke { .. }));
    let store = actor.allowed.store.clone().filter(|_| revoke);
    let Some(store) = store else {
        tracing::warn!(
            left = actor.allowed.waiting.len(),
            "blocklist: allows still saving at the stop are lost"
        );
        actor.allowed.stop = Stop::Running;
        return;
    };
    actor.allowed.stop = Stop::Voiding;
    let (done, gone) = futures::channel::oneshot::channel::<bool>();
    let started = std::thread::Builder::new()
        .name(VOID_THREAD.to_owned())
        .spawn(move || {
            let _gone = done.send(store.0.delete(integrity::ACCOUNT).is_ok());
        });
    if started.is_err() {
        actor.allowed.stop = Stop::Running;
        tracing::warn!("blocklist: no thread to delete allowed.json's hash at the stop");
        return;
    }
    let deadline = actor.clock.after(VOID_WAIT);
    actor.spawn_post(async move {
        let gone = match futures::future::select(gone, deadline).await {
            futures::future::Either::Left((gone, _)) => gone.ok(),
            futures::future::Either::Right(((), _)) => None,
        };
        Input::Allowed(Box::new(AllowedInput::StopVoided(gone)))
    });
}

/// Answers command `id` for `wrote` of its job's state `saved`, and keeps `persisted` in step.
fn answer(actor: &mut Actor, id: CommandId, change: &Change, wrote: Wrote, saved: Stored) {
    let allowed = &mut actor.allowed;
    let result = match wrote {
        Wrote::Saved => {
            allowed.persisted = Some(saved);
            Ok(CommandOutcome::Done)
        }
        Wrote::NotSaved { voided } => {
            if voided {
                allowed.persisted = Some(Stored::default());
            }
            match change {
                Change::Allow => Err(Error::refused(
                    ErrorCode::BlocklistAllowNotSaved,
                    ALLOW_NOT_SAVED,
                )),
                Change::Revoke {
                    host,
                    scope,
                    matter,
                } => {
                    let may_return = allowed
                        .persisted
                        .as_ref()
                        .is_none_or(|persisted| persisted.contains(host, *scope, *matter));
                    if may_return {
                        Err(Error::refused(
                            ErrorCode::BlocklistRevokeNotSaved,
                            REVOKE_NOT_SAVED,
                        ))
                    } else {
                        Ok(CommandOutcome::Done)
                    }
                }
            }
        }
    };
    actor.finish_command(id, result);
}
