//! The default matters folder when it comes after the start
//! (`HostConfig::with_documents_later`). The desktop looks Documents up off the host's start, as
//! a Documents folder redirected to a share that is down holds the lookup for its whole wait,
//! and the host starts without it. Until the answer comes, and only while no default matters
//! folder is known (`matters.default_location` or one beside `CLAUSEO_DATA_DIR` is known from
//! the start), what needs the default location waits for it, held here and off the actor:
//! `GetDefaultLocation`, a `FolderSupport` of the default location, and a `CreateMatter` in it
//! (which stays in `Actor::commands` meanwhile, so a resent id joins it and runs once). The
//! answer runs each through its normal path; the Scratch matter's scan starts then too
//! (`scratch.rs`).
//!
//! The wait is bounded: [`HOLD_BOUND`] on the host clock after the start, a little past the
//! lookup's own 2 s, or a lookup whose sender is dropped, refuses what is held with the
//! retryable `matter.folder_missing`, as does anything asking after the bound and before an
//! answer, or past [`MAX_HELD`] held requests. Shutdown answers what is held with
//! `Error::Closed`, as it does a command it will not run, and never waits for it.
//!
//! No check reads Documents: the refused folders never list it, so a pick or a register is
//! checked the same before and after the answer. An answer with no folder, or a dropped sender,
//! leaves issue #365's way: no default matters folder, no Scratch, and New matter asks for a
//! folder.

use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_protocol::{ErrorCode, HostQuery, MatterSource, Principal, QueryResult, Title};
use clauseo_settings::Locations;
use futures::channel::oneshot;
use futures::future::AbortHandle;

use super::commands::CommandCtx;
use super::input::{Input, Reply};
use super::matters::Details;
use super::{Actor, HostPhase};
use crate::error::Error;

/// How long after the start what needs the default location may wait for the lookup: a little
/// past its own 2 s bound (`clauseo_settings`' `DOCUMENTS_WAIT`).
pub(crate) const HOLD_BOUND: Duration = Duration::from_secs(5);

/// How many requests may be held for the answer at once. Past it a request runs at once and
/// meets [`not_yet`], as after the bound: a window that keeps asking cannot grow the list.
pub(crate) const MAX_HELD: usize = 64;

/// The folders with the default matters folder under Documents, as the lookup gives them
/// (`Locations::with_documents`), once it answers.
pub(crate) type Later = oneshot::Receiver<Locations>;

/// What reaches the actor about the lookup.
pub(crate) enum DocumentsInput {
    /// Its answer; `None` when its sender was dropped without one.
    Answer(Option<Locations>),
    /// [`HOLD_BOUND`] passed since the start.
    Bound,
}

/// A request held for the answer.
enum Held {
    /// `GetDefaultLocation`, or `FolderSupport` of the default location.
    Query(Principal, HostQuery, Reply<QueryResult>),
    /// `CreateMatter` in the default location, its receipt checked and not stored.
    Create {
        ctx: CommandCtx,
        name: Title,
        details: Details,
    },
}

/// Where the late default matters folder stands.
#[derive(Default)]
pub(crate) struct DocumentsWait {
    /// The answer to come, until start-up takes it to wait on.
    later: Option<Later>,
    /// The lookup has not answered yet.
    pending: bool,
    /// [`HOLD_BOUND`] passed with no answer: nothing more is held.
    bound: bool,
    /// The [`HOLD_BOUND`] timer.
    timer: Option<AbortHandle>,
    /// The default matters folder the answer named, under Documents.
    found: Option<PathBuf>,
    /// The requests held for the answer.
    held: Vec<Held>,
}

impl DocumentsWait {
    /// Waiting for `later`'s answer, or for nothing.
    pub(crate) fn new(later: Option<Later>) -> Self {
        Self {
            pending: later.is_some(),
            later,
            ..Self::default()
        }
    }
}

/// Starts the wait for the answer, on the host pool, and the [`HOLD_BOUND`] timer: nothing at
/// start-up waits on either.
pub(super) fn start(actor: &mut Actor) {
    let Some(later) = actor.documents.later.take() else {
        return;
    };
    actor.spawn_post(async move { Input::Documents(DocumentsInput::Answer(later.await.ok())) });
    let timer = actor.post_after(HOLD_BOUND, Input::Documents(DocumentsInput::Bound));
    actor.documents.timer = Some(timer);
}

/// Where new matters go: the effective settings' folder (`matters.default_location`, or the
/// one their folders give), or else the one the late answer named.
pub(super) fn default_matters_dir(actor: &Actor) -> Option<&Path> {
    actor
        .effective
        .default_matters_dir()
        .or(actor.documents.found.as_deref())
}

/// Whether the default matters folder is still being looked up: none is known yet and the
/// lookup has not answered.
pub(super) fn pending(actor: &Actor) -> bool {
    actor.documents.pending && default_matters_dir(actor).is_none()
}

/// Whether a request that needs the default location waits for the answer: it is still
/// looked up, the bound has not passed, the host is not stopping and fewer than [`MAX_HELD`]
/// wait. Otherwise the request runs at once, and while the lookup is pending meets [`not_yet`].
fn holds(actor: &Actor) -> bool {
    pending(actor)
        && !actor.documents.bound
        && !matches!(actor.phase, HostPhase::Stopping(_))
        && actor.documents.held.len() < MAX_HELD
}

/// Holds `query` with its `reply` when it needs the default location and [`holds`] says so;
/// gives `reply` back when the query runs now.
pub(super) fn hold_query(
    actor: &mut Actor,
    principal: &Principal,
    query: &HostQuery,
    reply: Reply<QueryResult>,
) -> Result<(), Reply<QueryResult>> {
    let needs = matches!(
        query,
        HostQuery::GetDefaultLocation
            | HostQuery::FolderSupport {
                source: MatterSource::DefaultLocation
            }
    );
    if !needs || !holds(actor) {
        return Err(reply);
    }
    let held = Held::Query(principal.clone(), query.clone(), reply);
    actor.documents.held.push(held);
    Ok(())
}

/// Holds a `CreateMatter` named `name` in the default location when [`holds`] says so; `false`
/// when it runs now. The command stays in `Actor::commands`, its waiters with it.
pub(super) fn hold_create(
    actor: &mut Actor,
    ctx: &CommandCtx,
    name: &Title,
    details: &Details,
) -> bool {
    if !holds(actor) {
        return false;
    }
    let held = Held::Create {
        ctx: *ctx,
        name: name.clone(),
        details: details.clone(),
    };
    actor.documents.held.push(held);
    true
}

/// The retryable refusal for a create or check in the default location before the answer.
pub(super) fn not_yet() -> Error {
    Error::refused(
        ErrorCode::MatterFolderMissing,
        "the Documents folder has not answered yet, so the default location is not known; \
         try again in a moment",
    )
}

/// One [`DocumentsInput`].
pub(super) fn on_input(actor: &mut Actor, input: DocumentsInput) {
    match input {
        DocumentsInput::Answer(Some(located)) => answered(actor, &located),
        DocumentsInput::Answer(None) => {
            tracing::warn!("the Documents lookup ended without an answer");
            actor.documents.pending = false;
            refuse_held(actor, &not_yet);
            disarm(actor);
        }
        DocumentsInput::Bound => {
            actor.documents.timer = None;
            if actor.documents.pending {
                tracing::warn!(
                    held = actor.documents.held.len(),
                    bound_ms = HOLD_BOUND.as_millis(),
                    "the Documents lookup has not answered; what waits for it is refused"
                );
                actor.documents.bound = true;
                refuse_held(actor, &not_yet);
            }
        }
    }
}

/// The lookup answered with `located`. Its default matters folder is kept here (a set
/// `matters.default_location` still wins), what was held runs through its normal path, and the
/// Scratch matter's scan starts when the host still wants one.
fn answered(actor: &mut Actor, located: &Locations) {
    actor.documents.pending = false;
    actor.documents.found = located.default_matters_dir().map(Path::to_path_buf);
    disarm(actor);
    tracing::info!(
        target: crate::log_file::LIFECYCLE,
        default_location = default_matters_dir(actor).is_some(),
        held = actor.documents.held.len(),
        "the Documents lookup answered"
    );
    for held in std::mem::take(&mut actor.documents.held) {
        match held {
            Held::Query(principal, query, reply) => {
                super::query::query(actor, &principal, &query, reply);
            }
            Held::Create { ctx, name, details } => {
                let source = MatterSource::DefaultLocation;
                super::matters::create_matter(actor, &ctx, &name, source, details);
            }
        }
    }
    super::scratch::start_late(actor);
}

/// Shutdown began: what is held is answered `Error::Closed`, so nothing waits on it.
pub(super) fn shutdown(actor: &mut Actor) {
    refuse_held(actor, &|| Error::Closed);
    disarm(actor);
}

/// Answers everything held with `refusal`'s error.
fn refuse_held(actor: &mut Actor, refusal: &dyn Fn() -> Error) {
    for held in std::mem::take(&mut actor.documents.held) {
        match held {
            Held::Query(_, _, reply) => {
                let _gone = reply.send(Err(refusal()));
            }
            Held::Create { ctx, .. } => actor.finish_command(ctx.id, Err(refusal())),
        }
    }
}

/// Stops the [`HOLD_BOUND`] timer.
fn disarm(actor: &mut Actor) {
    if let Some(timer) = actor.documents.timer.take() {
        timer.abort();
    }
}
