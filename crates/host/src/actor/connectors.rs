//! The connector commands and the connector state on the actor (R24, PR5).
//!
//! - Every `ConnectorCommand` runs here. With `tools.connectors` off, all but `SignOut` and
//!   `Remove` are refused (`settings.locked` when policy set it). With no connectors wired
//!   (`HostConfig::with_connectors`), every one is refused. None stores a receipt (decision
//!   D2), and a `command_id` a connector command used is never used again (see [`UsedIds`]).
//! - The work is off the actor (`jobs.rs`); a command whose work reads or writes something is
//!   answered when its result comes back, after the state it changed is published. `Add` is
//!   answered once the browser shows the sign-in page, and `AddCredits` once its pay link is in
//!   the state ([`ConnectorView::pay_url`](clauseo_protocol::ConnectorView), decision D1).
//! - The account is read at most once a minute for a paid result, `Check` and
//!   `RefreshBalance` ([`Reads`], by the host's clock); a sign-in, a new key or a paid recharge
//!   reads it at once. The connectors crate's own copy is as old as a minute at most.
//! - The relays' events (`ConnectorEvents`, bound at start) arrive as inputs: a credential that
//!   ended, out of credit (with a watch on its recharge), a paid result (a read, if due).
//! - The state reaches every app subscriber as `ConnectorsChanged` when it changes, and every
//!   app catch-up carries it ([`current`]).
//! - Sign-out, remove and the switch going off bump the credential generation: a key or a
//!   sign-in still in flight stores nothing, and a read that started before is not applied.
//! - The case-law hint (`hint.rs`): offered once per matter at a turn's end, and
//!   `DismissCaseLawHint` keeps it away from a matter (`hints.json`).
//! - `OpenPayLink` opens only `AddCredits`' checked https pay link through the native confirm
//!   (`ConnectorOpener::open_confirmed`); no link comes from the window, the relay, a tool
//!   result or the agent (`CaseLaw::pay_link`).

pub(super) mod hint;
mod jobs;
mod limits;
mod pay_link;
mod watch;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use clauseo_connectors::{CASE_LAW, CancelFlag, ConnectorHandle, CredentialStatus, PayUrl};
use clauseo_protocol::{
    CommandId, CommandOutcome, ConnectorCommand, ConnectorsState, ErrorCode, HostCommand, HostEvent,
};

use super::Actor;
use super::commands::CommandCtx;
use super::input::Input;
use crate::connectors::{ConnectorOpener, ConnectorsWiring, CredentialEnd, RelayEvent};
use crate::error::Error;
use jobs::{Change, Finished, Job, Post, Reach, Refusal};
use limits::{Reads, UsedIds};
use watch::{Watch, end_recharges, open_recharge, out_of_credit};

/// The least time between two account reads that a paid result, `Check` or `RefreshBalance`
/// asks for.
pub(crate) const READ_INTERVAL: Duration = Duration::from_mins(1);

/// How many `command_id`s of connector commands the host remembers.
const USED_IDS: usize = 4_096;

/// What a connector job or a relay posts to the actor.
#[derive(Debug)]
pub(crate) enum ConnectorInput {
    /// What a relay heard.
    Relay(RelayEvent),
    /// The browser shows the sign-in page: answers `Add`.
    SignInStarted {
        command: Option<CommandId>,
        generation: u64,
        cancel: CancelFlag,
    },
    /// `AddCredits`' recharge is open: answers it once its link is published. `watch` names
    /// the watch it was started under; a newer one or a sign-out makes it stale.
    PayLink {
        command: Option<CommandId>,
        watch: u64,
        recharge: String,
        pay_url: PayUrl,
    },
    /// A job's result.
    Finished(Box<Finished>),
    /// A recharge watch's thread ended, however it ended (a panic too): the watch numbered
    /// `watch`, if still the current one, goes with its pay link.
    WatchEnded { watch: u64 },
    /// `OpenPayLink`'s confirm and open ended: answers it.
    PayLinkOpened {
        command: CommandId,
        reply: Result<(), clauseo_protocol::HostError>,
    },
}

/// The connectors on the actor.
pub(crate) struct ConnectorsOnActor {
    wired: Option<Wired>,
    state: crate::connectors::state::CaseLaw,
    /// The state last published, so an unchanged one is not sent again.
    published: Option<ConnectorsState>,
    reads: Reads,
    /// The sign-in in flight: its number (a newer `Add` makes the older one's end stale) and
    /// its cancel.
    sign_in: (u64, Option<CancelFlag>),
    /// The recharge watch: its number, the recharge once known, and its cancel.
    watch: Option<Watch>,
    /// The last watch's number.
    watches: u64,
    /// The `AddCredits` whose recharge is still being opened: the number its watch will take
    /// when its pay link arrives, and its cancel. The current watch stays until then, so a
    /// refused `AddCredits` leaves an out-of-credit recharge watched.
    opening: Option<(u64, CancelFlag)>,
    used: UsedIds,
    /// The credential generation: sign-out, remove and the switch going off bump it. A job
    /// carries the one it started under; a credential job stores its credential only while it
    /// still holds (asked under the connectors crate's credential lock), and what an older
    /// job read about the credential or the account is not applied.
    credentials: Arc<AtomicU64>,
    /// The case-law hint's marks and the matters waiting on liveness.
    hint: hint::HintState,
}

struct Wired {
    connectors: Arc<clauseo_connectors::Connectors>,
    handle: ConnectorHandle,
    opener: Arc<dyn ConnectorOpener>,
    events: Arc<crate::connectors::ConnectorEvents>,
}

impl ConnectorsOnActor {
    /// The connectors as `HostConfig::with_connectors` wired them, or none.
    pub(crate) fn new(wiring: Option<ConnectorsWiring>) -> Self {
        let wired = wiring.and_then(|wiring| {
            let handle = wiring.connectors.get(CASE_LAW.id)?;
            Some(Wired {
                connectors: wiring.connectors,
                handle,
                opener: wiring.opener,
                events: wiring.events,
            })
        });
        Self {
            wired,
            state: crate::connectors::state::CaseLaw::default(),
            published: None,
            reads: Reads::default(),
            sign_in: (0, None),
            watch: None,
            watches: 0,
            opening: None,
            used: UsedIds::default(),
            credentials: Arc::new(AtomicU64::new(0)),
            hint: hint::HintState::default(),
        }
    }
}

/// Start-up: binds the relays' events to the actor and reads the credential, off the actor.
pub(super) fn start(actor: &mut Actor) {
    let Some(wired) = &actor.connectors.wired else {
        return;
    };
    let post = actor.post.clone();
    wired.events.bind(Box::new(move |event| {
        let _gone = post.try_send(Input::Connector(Box::new(ConnectorInput::Relay(event))));
    }));
    if let Err(error) = spawn(actor, None, Job::Status) {
        tracing::warn!(error = %crate::log_file::Cause(&error), "the connector's state was not read");
    }
}

/// Whether `command` under `id` is a reuse a connector command forbids: a connector command
/// whose id is pending or was used, or any command whose id a connector command used.
pub(super) fn reused(actor: &Actor, id: CommandId, command: &HostCommand) -> bool {
    actor.connectors.used.contains(id)
        || (matches!(command, HostCommand::Connector(_)) && actor.commands.contains_key(&id))
}

/// Remembers that a connector command used `id`.
pub(super) fn record(actor: &mut Actor, id: CommandId, command: &HostCommand) {
    if matches!(command, HostCommand::Connector(_)) {
        actor.connectors.used.insert(id);
    }
}

/// One connector command: refused, answered now, or answered by its work.
pub(super) fn run(actor: &mut Actor, ctx: &CommandCtx, command: ConnectorCommand) {
    match begin(actor, ctx.id, command) {
        Ok(true) => actor.finish_command(ctx.id, Ok(CommandOutcome::Done)),
        Ok(false) => {}
        Err(error) => actor.finish_command(ctx.id, Err(error)),
    }
}

/// The checks and the work of one command; `Ok(true)` answers it now.
fn begin(actor: &mut Actor, id: CommandId, command: ConnectorCommand) -> Result<bool, Error> {
    let tidy = matches!(
        command,
        ConnectorCommand::SignOut { .. } | ConnectorCommand::Remove { .. }
    );
    if !actor.effective.settings().tools.connectors && !tidy {
        let code = if actor.effective.is_locked("tools.connectors") {
            ErrorCode::SettingsLocked
        } else {
            ErrorCode::CommandInvalid
        };
        return Err(Error::refused(code, "data connectors are turned off"));
    }
    if actor.connectors.wired.is_none() {
        return Err(refused("no data connectors on this install"));
    }
    // An error shown lasts until the next command.
    actor.connectors.state.error = None;
    let job = match command {
        ConnectorCommand::DismissCaseLawHint { matter } => {
            hint::dismiss(actor, matter)?;
            return Ok(true);
        }
        // The job reads the account only if it finds the connector signed in.
        ConnectorCommand::Check {} => Job::Check {
            slot: actor.connectors.reads.take(actor.now().mono),
        },
        ConnectorCommand::Add { id: connector } => {
            known(&connector)?;
            if actor
                .connectors
                .state
                .liveness
                .as_ref()
                .is_some_and(|l| !l.live)
            {
                return Err(refused("the connector is not offered yet"));
            }
            cancel_sign_in(actor);
            let generation = actor.connectors.sign_in.0;
            let opener = Arc::clone(&wired(actor)?.opener);
            Job::SignIn { generation, opener }
        }
        ConnectorCommand::AddApiKey { id: connector, key } => {
            known(&connector)?;
            Job::ApiKey(key)
        }
        ConnectorCommand::AddCredits {
            id: connector,
            pack,
        } => {
            known(&connector)?;
            if !actor.connectors.state.signed_in() {
                return Err(refused("the connector is not signed in"));
            }
            let cancel = CancelFlag::new();
            let watch = open_recharge(actor, cancel.clone());
            Job::Credits {
                pack,
                watch,
                cancel,
            }
        }
        ConnectorCommand::SignOut { id: connector } => tidy_up(actor, &connector, Job::SignOut)?,
        ConnectorCommand::Remove { id: connector } => tidy_up(actor, &connector, Job::Remove)?,
        ConnectorCommand::OpenPayLink { id: connector } => {
            known(&connector)?;
            let url = actor
                .connectors
                .state
                .pay_link()
                .ok_or_else(|| refused("there is no checked pay link to open"))?
                .clone();
            let opener = Arc::clone(&wired(actor)?.opener);
            pay_link::open(actor, id, opener, url)?;
            return Ok(false);
        }
        ConnectorCommand::RefreshBalance { id: connector } => {
            known(&connector)?;
            if !actor.connectors.state.signed_in() || !actor.connectors.reads.take(actor.now().mono)
            {
                publish(actor);
                return Ok(true);
            }
            Job::Read
        }
    };
    spawn(actor, Some(id), job)?;
    Ok(false)
}

/// `SignOut` and `Remove`: the credential ends for the work in flight (a key being checked
/// stores nothing), and the sign-in in flight and the recharge watch are cancelled first.
fn tidy_up(actor: &mut Actor, connector: &str, job: Job) -> Result<Job, Error> {
    known(connector)?;
    end_credential(actor);
    cancel_sign_in(actor);
    end_recharges(actor);
    Ok(job)
}

/// Bumps the credential generation: no job started before stores a credential, and what one
/// read is not applied.
fn end_credential(actor: &mut Actor) {
    actor.connectors.credentials.fetch_add(1, Ordering::SeqCst);
}

/// The credential generation now.
fn generation(actor: &Actor) -> u64 {
    actor.connectors.credentials.load(Ordering::SeqCst)
}

fn refused(message: &'static str) -> Error {
    Error::refused(ErrorCode::CommandInvalid, message)
}

/// `command.invalid` for a connector the host does not know.
fn known(id: &str) -> Result<(), Error> {
    if id == CASE_LAW.id {
        Ok(())
    } else {
        Err(refused("no such connector"))
    }
}

fn wired(actor: &Actor) -> Result<&Wired, Error> {
    actor
        .connectors
        .wired
        .as_ref()
        .ok_or_else(|| refused("no data connectors on this install"))
}

/// Starts `job` off the actor; a failed start gives the read slot back.
fn spawn(actor: &mut Actor, command: Option<CommandId>, job: Job) -> Result<(), Error> {
    let slot = matches!(job, Job::Check { slot: true } | Job::Read);
    let wired = wired(actor)?;
    let reach = Reach {
        connectors: Arc::clone(&wired.connectors),
        handle: wired.handle.clone(),
        post: Post(actor.post.clone()),
        credentials: Arc::clone(&actor.connectors.credentials),
        generation: generation(actor),
    };
    let started = jobs::spawn(reach, command, job);
    if started.is_err() && slot {
        actor.connectors.reads.in_flight = false;
    }
    started
}

/// Cancels the sign-in in flight, if any, and makes every sign-in started before stale (one
/// whose page has not opened yet is cancelled when it does).
fn cancel_sign_in(actor: &mut Actor) {
    actor.connectors.sign_in.0 += 1;
    if let Some(cancel) = actor.connectors.sign_in.1.take() {
        cancel.cancel();
    }
    actor.connectors.state.signing_in = false;
}

/// A connector input.
pub(super) fn on_input(actor: &mut Actor, input: ConnectorInput) {
    match input {
        ConnectorInput::Relay(event) => on_relay(actor, event),
        ConnectorInput::SignInStarted {
            command,
            generation,
            cancel,
        } => {
            if generation == actor.connectors.sign_in.0 {
                actor.connectors.sign_in.1 = Some(cancel);
                actor.connectors.state.signing_in = true;
            } else {
                cancel.cancel();
            }
            answer(actor, command, Ok(()));
        }
        ConnectorInput::PayLink {
            command,
            watch,
            recharge,
            pay_url,
        } => watch::on_pay_link(actor, command, watch, (recharge, pay_url)),
        ConnectorInput::Finished(finished) => on_finished(actor, *finished),
        ConnectorInput::PayLinkOpened { command, reply } => {
            actor.finish_command(
                command,
                reply.map(|()| CommandOutcome::Done).map_err(Error::from),
            );
        }
        ConnectorInput::WatchEnded { watch } => watch::on_watch_ended(actor, watch),
    }
}

/// Publishes the state, then answers `command`.
fn answer(actor: &mut Actor, command: Option<CommandId>, reply: Result<(), Refusal>) {
    publish(actor);
    if let Some(id) = command {
        let result = reply
            .map(|()| CommandOutcome::Done)
            .map_err(Refusal::into_error);
        actor.finish_command(id, result);
    }
}

fn on_finished(actor: &mut Actor, mut finished: Finished) {
    let now = actor.now().mono;
    if finished.generation != generation(actor) {
        // Started before a sign-out, a remove or the switch going off: what it read about the
        // credential and the account is older than that end, so the state is read again.
        let read = finished.credential.take().is_some();
        finished.account = None;
        finished.out_of_credit = None;
        if read && let Err(error) = spawn(actor, None, Job::Status) {
            tracing::warn!(error = %crate::log_file::Cause(&error), "the connector's state was not read");
        }
    }
    let connectors = &mut actor.connectors;
    if finished.slot {
        connectors.reads.in_flight = false;
    } else if finished.account.is_some() {
        connectors.reads.last = Some(now);
    }
    let state = &mut connectors.state;
    let liveness = finished.liveness.is_some();
    if let Some(liveness) = finished.liveness {
        state.liveness = Some(liveness);
    }
    if let Some(credential) = finished.credential {
        state.set_credential(credential);
    }
    // Only while signed in: a read that came back after the credential ended is dropped.
    if let Some(account) = finished.account
        && state.signed_in()
    {
        state.set_account(account);
    }
    match finished.change {
        Change::Nothing => {}
        Change::NoLiveness => connectors.hint.no_liveness(),
        Change::SignInEnded { generation, error } => {
            if generation == connectors.sign_in.0 {
                connectors.sign_in.1 = None;
                state.signing_in = false;
                state.error = error.or(state.error);
            }
        }
        Change::Failed(message) => state.error = Some(message),
        Change::Reset => {
            state.account = None;
            state.out_of_credit = None;
            state.pay_url = None;
        }
        Change::Recharge { recharge, paid } => {
            let current = connectors
                .watch
                .as_ref()
                .is_some_and(|watch| watch.follows(&recharge));
            if current {
                connectors.watch = None;
                state.pay_url = None;
            }
            if paid {
                state.out_of_credit = None;
            }
        }
    }
    if let Some(payment) = finished.out_of_credit {
        out_of_credit(actor, payment);
    }
    if liveness {
        hint::liveness_known(actor);
    }
    answer(actor, finished.command, finished.reply);
}

/// What a relay heard.
fn on_relay(actor: &mut Actor, event: RelayEvent) {
    match event {
        RelayEvent::CredentialEnded { ended, .. } => {
            let credential = match ended {
                CredentialEnd::NotAdded => CredentialStatus::NotAdded,
                CredentialEnd::SignedOut => CredentialStatus::SignedOut,
                CredentialEnd::KeyRevoked => CredentialStatus::KeyRevoked,
            };
            // What a job read before this end is not applied.
            end_credential(actor);
            actor.connectors.state.set_credential(credential);
            end_recharges(actor);
        }
        RelayEvent::OutOfCredit { payment, .. } => out_of_credit(actor, payment),
        RelayEvent::CaseLawFound { session, found } => {
            super::turn::case_law::found(actor, session.thread, found);
            return;
        }
        RelayEvent::PaidResult { .. } => {
            // A paid call went through: there is credit.
            actor.connectors.state.out_of_credit = None;
            if actor.connectors.state.signed_in()
                && actor.connectors.reads.take(actor.now().mono)
                && let Err(error) = spawn(actor, None, Job::Read)
            {
                tracing::warn!(error = %crate::log_file::Cause(&error), "no balance read");
            }
        }
    }
    publish(actor);
}

/// The state as the app sees it now, if connectors are wired.
fn view(actor: &Actor) -> Option<ConnectorsState> {
    actor.connectors.wired.as_ref()?;
    let available = actor.effective.settings().tools.connectors;
    let locked = actor.effective.is_locked("tools.connectors");
    Some(actor.connectors.state.view(&CASE_LAW, available, locked))
}

/// Sends the state to every app subscriber when it changed since the last one sent.
fn publish(actor: &mut Actor) {
    let Some(state) = view(actor) else {
        return;
    };
    if actor.connectors.published.as_ref() == Some(&state) {
        return;
    }
    actor.connectors.published = Some(state.clone());
    super::app::publish(actor, &HostEvent::ConnectorsChanged(state));
}

/// `tools.connectors` changed: off ends the credential for the work in flight (a key being
/// checked stores nothing), cancels a sign-in in flight and ends the recharge watch (no more
/// polling) with its pay link; either way the state goes out.
pub(super) fn switch_changed(actor: &mut Actor) {
    if !actor.effective.settings().tools.connectors {
        end_credential(actor);
        cancel_sign_in(actor);
        end_recharges(actor);
        actor.connectors.state.pay_url = None;
    }
    publish(actor);
}

/// The app catch-up's part: the whole state, when connectors are wired.
pub(super) fn current(actor: &Actor) -> Option<HostEvent> {
    view(actor).map(HostEvent::ConnectorsChanged)
}

/// Shutdown: the sign-in and the recharge watch are cancelled; their threads post to nobody.
pub(super) fn shutdown(actor: &mut Actor) {
    cancel_sign_in(actor);
    end_recharges(actor);
}
