//! Denials (R18 PR 3a): what the jail, the egress proxy or a failed tool call refused, taken in
//! on the actor and logged as `DenialRecorded` in the thread's stream.
//!
//! - **In:** `Input::Denial` ([`DenialInput`]), posted by a launch's sinks through
//!   [`feed::DenialFeed`] (R17 PR-D wires them) or by the tool-call watch here ([`tool_seen`]). The
//!   block list's refusals (`Denial::Blocked`) stay on `blocked::site_blocked`, and are ignored
//!   here, as is `UpstreamUnavailable` (the firm's proxy lookup failed; nothing to widen).
//! - **Intake** ([`count`]): the per-thread rate cap, the counts toward the pause, one card per
//!   kind and target per turn. A pause logs `DenialsPaused` and sends `session/cancel` once.
//! - **What is offered** ([`offer`]): a path is resolved off the actor ([`resolve`]), then the
//!   one pure rule decides.
//! - **Network paths** ([`network`]): a path that names a share, a device or a remote mount,
//!   or a link on the way to one, is found from its text before anything touches it, and its
//!   card is never offered. Every source goes through it, the tool-call watch included.
//! - **`DismissDenial`:** logs `DenialDismissed` for a denial a loaded thread holds.
//!
//! The log names a kind, a source or a count, never a path or a host.

mod count;
pub(crate) mod feed;
pub(crate) mod network;
pub(crate) mod offer;
mod resolve;
mod tool_call;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_protocol::{
    CommandOutcome, DenialDismissed, DenialId, DenialKind, DenialRecorded, DenialSource,
    DenialsPaused, ErrorCode, MatterId, StreamId, ThreadId, ToolCallId, ToolCallStatus, TurnId,
};
use clauseo_sandbox::{Access, Denial, PathClass};
use futures::task::LocalSpawnExt;

pub(crate) use count::{Denials, Recorded};
use network::NetEnv;
use offer::{KnownMatter, Offer, OfferRules, Refused};
use resolve::{Fs, RealFs, Resolved};
pub(crate) use tool_call::Seen;

use super::super::commands::{AfterCommand, CommandCtx};
use super::super::input::Input;
use super::super::{Actor, Tag};
use super::out::Out;
use super::{TurnInput, cancel, running_turn, updates};
use crate::agent::codex::prepare::{Job, off_actor};
use crate::config::SessionKey;
use crate::error::Error;
use crate::log_file::Cause;

/// One refusal for the actor: the launch it came from, what the sandbox or the proxy reported
/// (or the tool-call watch made: a `Path` of class `Outside`, or `LinkTarget` when a matter
/// path led out), and who reported it.
#[derive(Debug)]
pub(crate) struct DenialInput {
    pub(crate) key: SessionKey,
    pub(crate) kind: Denial,
    pub(crate) source: DenialSource,
}

/// A path denial made canonical and decided off the actor.
#[derive(Debug)]
pub(crate) struct Checked {
    thread: ThreadId,
    turn: Option<TurnId>,
    source: DenialSource,
    offer: Offer,
    /// Why the path was not checked (a network location, or it could not be resolved): logged
    /// as such, never offered.
    unchecked: Option<Unchecked>,
}

/// Why a path's card was decided without resolving it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unchecked {
    /// It names a network location, or a link on the way does.
    Network,
    /// A loop, a link that can't be read, or an I/O error on the way.
    Unresolved,
}

impl Unchecked {
    fn word(self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::Unresolved => "unresolved",
        }
    }
}

/// `Input::Denial`: counts it toward the pause, and, past the rate cap, decides its card
/// (off the actor for a path) and records it.
pub(crate) fn on_denial(actor: &mut Actor, input: DenialInput) {
    let DenialInput { key, kind, source } = input;
    let thread = key.thread;
    if actor.check_thread(thread).is_err() {
        tracing::debug!(%thread, "a denial for a thread the host no longer has was dropped");
        return;
    }
    let (counted, rated) = match &kind {
        Denial::Dropped { count } => (*count, false),
        Denial::Path {
            class: PathClass::ReadOnly,
            ..
        }
        | Denial::UpstreamAuth { .. } => (0, true),
        Denial::Path { .. } | Denial::Host { .. } => (1, true),
        // The block list has its own notice and its own allow (decision record 0053).
        Denial::Blocked { .. } | Denial::UpstreamUnavailable { .. } | _ => {
            tracing::debug!(%thread, kind = word(&kind), "a denial left to its own path");
            return;
        }
    };
    let turn = running_turn(actor, thread);
    let mut denials = std::mem::take(&mut actor.denials);
    denials.forget_ended(|thread| running_turn(actor, thread));
    actor.denials = denials;
    let now = actor.now().mono;
    let admitted = actor.denials.admit(thread, turn, now, counted, rated);
    if let Some((turn, refused)) = admitted.pause {
        pause(actor, thread, turn, refused);
    }
    if !admitted.pass {
        return;
    }
    let Some(rules) = rules_of(actor, key.matter) else {
        return;
    };
    let site = |host: &str, shared_hosting, why| {
        let refused = Refused::Site {
            host,
            shared_hosting,
            why,
        };
        offer::offer(&rules, refused)
    };
    let decided = match kind {
        Denial::Path { path, class, .. } => {
            return check_path(actor, thread, turn, source, rules, path, class);
        }
        Denial::Host {
            host,
            why,
            shared_hosting,
            ..
        } => site(&host, shared_hosting, why),
        Denial::UpstreamAuth { upstream } => offer::offer(
            &rules,
            Refused::UpstreamAuth {
                upstream: &upstream,
            },
        ),
        Denial::Dropped { .. }
        | Denial::UpstreamUnavailable { .. }
        | Denial::Blocked { .. }
        | _ => return,
    };
    record(
        actor,
        Checked {
            thread,
            turn,
            source,
            offer: decided,
            unchecked: None,
        },
    );
}

/// A path's check: canonical on a thread of its own, then the offer rules; back on the actor
/// as `TurnInput::Denial`. A path that does not resolve is dropped.
fn check_path(
    actor: &mut Actor,
    thread: ThreadId,
    turn: Option<TurnId>,
    source: DenialSource,
    rules: OfferRules,
    path: PathBuf,
    class: PathClass,
) {
    let job = Job::short("denial check", Some(rules.matter));
    let clock = Arc::clone(&actor.clock);
    spawn_maybe(actor, async move {
        let work = move || {
            let net = NetEnv::current();
            let rules = rules.canonical(&net);
            Ok(decide(&rules, &path, class, &RealFs, &net))
        };
        let (offer, unchecked) = off_actor(clock, job, work).await.ok()?;
        let checked = Checked {
            thread,
            turn,
            source,
            offer,
            unchecked,
        };
        Some(Input::Turn(TurnInput::Denial(Box::new(checked))))
    });
}

/// Off the actor: where the path leads on `fs`, then its card. A path that names a network
/// location or can't be resolved is never touched further and never offered; the second value
/// says which.
pub(crate) fn decide(
    rules: &OfferRules,
    path: &Path,
    class: PathClass,
    fs: &dyn Fs,
    net: &NetEnv,
) -> (Offer, Option<Unchecked>) {
    let unchecked = |why| {
        let shown = path.display().to_string();
        (
            offer::offer(rules, Refused::Unchecked { path: &shown }),
            Some(why),
        )
    };
    match resolve::resolve(path, fs, net) {
        Resolved::Network => unchecked(Unchecked::Network),
        Resolved::Unresolved => unchecked(Unchecked::Unresolved),
        Resolved::Local { path, is_file } => {
            let refused = Refused::Path {
                path: &path,
                is_file,
                class,
            };
            (offer::offer(rules, refused), None)
        }
    }
}

/// `TurnInput::Denial`: a path's check came back.
pub(crate) fn on_checked(actor: &mut Actor, checked: Checked) {
    record(actor, checked);
}

/// Records a decided denial: a new card, or its count grown.
fn record(actor: &mut Actor, checked: Checked) {
    let Checked {
        thread,
        turn,
        source,
        offer,
        unchecked,
    } = checked;
    let wall = actor.now().wall;
    let mut denials = std::mem::take(&mut actor.denials);
    let running = running_turn(actor, thread);
    let recorded = denials.record(thread, turn, running, &offer.kind, || actor.ids.mint(wall));
    actor.denials = denials;
    let (denial, count) = match recorded {
        Ok(Recorded::Log { denial, count }) => (denial, count),
        Ok(Recorded::Capped { first }) => {
            if first {
                tracing::info!(%thread, "the turn's denial cards are at their cap; new ones are not recorded");
            }
            return;
        }
        Err(error) => {
            tracing::warn!(%thread, error = %Cause(&error), "a denial was not recorded");
            return;
        }
    };
    if let (1, Some(why)) = (count, unchecked) {
        // The kind and why alone: a path, network or not, never goes in the log.
        tracing::info!(
            %thread,
            kind = kind_word(&offer.kind),
            source = source_word(&source),
            unchecked = why.word(),
            "denial recorded for a path not checked; nothing is offered"
        );
    } else if count == 1 {
        tracing::info!(
            %thread,
            kind = kind_word(&offer.kind),
            source = source_word(&source),
            offered = offer.offered,
            "denial recorded"
        );
    }
    let event = DenialRecorded {
        turn,
        denial,
        kind: offer.kind,
        source,
        offered: offer.offered,
        count,
    };
    if let Err(error) = updates::commit(actor, thread, [event.into()], Vec::new()) {
        tracing::warn!(%thread, error = %Cause(&error), "a denial was not logged");
    }
}

/// Too many refusals: `DenialsPaused`, then the turn cancelled (`session/cancel`), once.
fn pause(actor: &mut Actor, thread: ThreadId, turn: TurnId, refused: u16) {
    tracing::info!(%thread, refused, "too many refusals: the answer is paused");
    let event = DenialsPaused { turn, refused };
    if let Err(error) = updates::commit(actor, thread, [event.into()], Vec::new()) {
        tracing::warn!(%thread, error = %Cause(&error), "a pause was not logged");
    }
    cancel::cancel_current(actor, thread);
}

/// The tool-call watch: keeps the paths a live tool call names, and when it ends, starts "in
/// a row" again or not, and reads a `failed` end with an access-denied message as a refusal.
pub(super) fn tool_seen(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    call: ToolCallId,
    seen: Seen<'_>,
) {
    actor.denials.watch(thread, turn, call, seen.paths());
    let completed = match seen.status {
        Some(ToolCallStatus::Completed) => true,
        Some(ToolCallStatus::Failed | ToolCallStatus::Cancelled) => false,
        _ => return,
    };
    let named = actor.denials.unwatch(thread, turn, call);
    if seen.failed_access_denied() && !named.is_empty() {
        check_tool_call(actor, thread, call, named);
    }
    actor.denials.tool_ended(thread, turn, completed);
}

/// Off the actor: the first path the call named that resolves outside the jail (the matter
/// folder, and Clauseo's data folder, which holds the run folders a launch is given) becomes
/// a `Denial::Path`, `LinkTarget` when it is a matter path that leads out.
fn check_tool_call(actor: &mut Actor, thread: ThreadId, call: ToolCallId, named: Vec<String>) {
    let Some(agent) = updates::agent(actor, thread) else {
        return;
    };
    let (launch, folder) = (agent.id, agent.folder.clone());
    let Ok(matter) = actor.check_thread(thread).map(|entry| entry.row.matter) else {
        return;
    };
    let data_dir = actor.effective.data_dir().to_path_buf();
    let job = Job::short("tool call denial check", Some(matter));
    let clock = Arc::clone(&actor.clock);
    let key = SessionKey {
        matter,
        thread,
        launch,
    };
    spawn_maybe(actor, async move {
        let work = move || {
            let net = NetEnv::current();
            Ok(outside_jail(&folder, &data_dir, &named, &RealFs, &net))
        };
        let (path, class) = off_actor(clock, job, work).await.ok().flatten()?;
        let input = DenialInput {
            key,
            kind: Denial::Path {
                path,
                access: Access::Read,
                class,
            },
            source: DenialSource::ToolCall { tool_call: call },
        };
        Some(Input::Denial(Box::new(input)))
    });
}

/// The first of `named` that leads outside `folder` and `data_dir` on `fs`, with its class.
/// A network path (a UNC path, `/net/…`, a link to one) is outside without being touched:
/// taken as written, never joined to the folder. One under the folder by its text alone (a
/// matter on a share) is inside, also untouched.
pub(crate) fn outside_jail(
    folder: &Path,
    data_dir: &Path,
    named: &[String],
    fs: &dyn Fs,
    net: &NetEnv,
) -> Option<(PathBuf, PathClass)> {
    // The host's own folders: a share among them is compared as written.
    let host = |p: &Path| {
        if net.is_network(p) {
            p.to_path_buf()
        } else {
            dunce::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
        }
    };
    let (root, data) = (host(folder), host(data_dir));
    let under = |path: &Path, prefix: &Path| offer::under(path, prefix, net.fold_case);
    named.iter().find_map(|text| {
        let raw = Path::new(text);
        let asked = if net.is_network(raw) {
            raw.to_path_buf()
        } else {
            folder.join(raw)
        };
        let lexical = !asked
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
            && (under(&asked, folder) || under(&asked, &root));
        let class = if lexical {
            PathClass::LinkTarget
        } else {
            PathClass::Outside
        };
        if net.is_network(&asked) {
            let text_under = |prefix: &Path| network::under_text(&asked, prefix, net.fold_case);
            let inside = !asked
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
                && (text_under(folder) || text_under(&root));
            return (!inside).then_some((asked, PathClass::Outside));
        }
        match resolve::resolve(&asked, fs, net) {
            // A network path, or one not known to be inside: refused, its card never offered.
            Resolved::Network | Resolved::Unresolved => Some((asked, class)),
            Resolved::Local { path, .. } => {
                let inside = under(&path, &root) || under(&path, &data);
                (!inside).then_some((path, class))
            }
        }
    })
}

/// `DismissDenial`: `DenialDismissed` for a denial `thread` holds, with its receipt; the
/// thread's own refusal when it is not there, else `command.invalid` for a denial it does not
/// hold.
pub(crate) fn dismiss(actor: &mut Actor, ctx: CommandCtx, thread: ThreadId, denial: DenialId) {
    if let Err(error) = actor.check_thread(thread) {
        return actor.finish_command(ctx.id, Err(error));
    }
    if !holds(actor, thread, denial) {
        let refusal = Error::refused(ErrorCode::CommandInvalid, "no such denial");
        return actor.finish_command(ctx.id, Err(refusal));
    }
    let logged = (|| {
        let mut out = Out::new(actor, thread, Some(ctx.id))?;
        out.push(actor, DenialDismissed { denial })?;
        let now = actor.now().wall;
        let stream = Some(StreamId::Thread(thread));
        out.draft
            .receipt(ctx.receipt(stream, CommandOutcome::Done, now));
        let tag = Tag::Command {
            id: ctx.id,
            outcome: CommandOutcome::Done,
            after: AfterCommand::Nothing,
        };
        out.send_tagged(actor, tag)
    })();
    if let Err(error) = logged {
        actor.finish_command(ctx.id, Err(error));
    }
}

/// Whether `thread`'s loaded state holds `denial`.
fn holds(actor: &Actor, thread: ThreadId, denial: DenialId) -> bool {
    actor
        .loaded(StreamId::Thread(thread))
        .is_some_and(|loaded| match &loaded.state {
            clauseo_reducer::StreamState::Thread(state) => state.denial(denial).is_some(),
            clauseo_reducer::StreamState::Matter(_) => false,
        })
}

/// The rules for `matter`'s agents, as the host holds them (not canonical yet).
fn rules_of(actor: &Actor, matter: MatterId) -> Option<OfferRules> {
    let matter_root = PathBuf::from(&actor.check_matter(matter).ok()?.row.folder_path);
    let matters = actor
        .matters
        .values()
        .chain(actor.forgotten.values())
        .map(|entry| KnownMatter {
            id: entry.row.id,
            name: entry.row.name.clone(),
            root: PathBuf::from(&entry.row.folder_path),
        })
        .collect();
    Some(OfferRules {
        home: actor.home.clone(),
        data_dir: actor.effective.data_dir().to_path_buf(),
        matter,
        matter_root,
        refused: actor
            .refused_folders
            .iter()
            .chain(&actor.refused_trees)
            .cloned()
            .collect(),
        matters,
        fold_case: offer::FOLD_CASE,
    })
}

/// Runs `task` on the actor's pool and posts what it gives, if anything.
fn spawn_maybe(actor: &Actor, task: impl Future<Output = Option<Input>> + 'static) {
    let post = actor.post.clone();
    let spawned = actor.spawner.spawn_local(async move {
        if let Some(input) = task.await {
            // The actor may be gone by then; nobody is left to tell.
            let _gone = post.try_send(input);
        }
    });
    if let Err(error) = spawned {
        tracing::error!(%error, "the host pool refused a task");
    }
}

/// A sandbox denial's kind, for the log.
fn word(kind: &Denial) -> &'static str {
    match kind {
        Denial::Path { .. } => "path",
        Denial::Host { .. } => "host",
        Denial::UpstreamAuth { .. } => "upstream_auth",
        Denial::Dropped { .. } => "dropped",
        Denial::UpstreamUnavailable { .. } => "upstream_unavailable",
        Denial::Blocked { .. } => "blocked",
        _ => "other",
    }
}

fn kind_word(kind: &DenialKind) -> &'static str {
    match kind {
        DenialKind::Folder { .. } => "folder",
        DenialKind::File { .. } => "file",
        DenialKind::Site { .. } => "site",
        DenialKind::UpstreamAuth { .. } => "upstream_auth",
        DenialKind::Unknown { .. } => "unknown",
    }
}

fn source_word(source: &DenialSource) -> &'static str {
    match source {
        DenialSource::Sandbox => "sandbox",
        DenialSource::Proxy => "proxy",
        DenialSource::ToolCall { .. } => "tool_call",
        DenialSource::Unknown { .. } => "unknown",
    }
}
