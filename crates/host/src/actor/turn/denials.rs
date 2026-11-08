//! Denials (R18 PR 3a): what the jail, the egress proxy or a failed tool call refused, taken in
//! on the actor and logged as `DenialRecorded` in the thread's stream.
//!
//! - **In:** `Input::Denial` ([`DenialInput`]), posted by a launch's sinks through
//!   [`feed::DenialFeed`] (R17 PR-D wires them), and the tool-call watch here ([`tool_seen`]).
//!   The block list's refusals (`Denial::Blocked`) stay on `blocked::site_blocked`, and are
//!   ignored here, as is `UpstreamUnavailable` (the firm's proxy lookup failed; nothing to widen).
//! - **Intake** ([`count`]): the per-thread rate cap and checks in flight, run on the actor before
//!   anything starts off it; the counts toward the pause; one card per kind and target per turn.
//!   A pause logs `DenialsPaused` and sends `session/cancel` once.
//! - **What is offered** ([`offer`]): a path is resolved off the actor ([`check`], [`resolve`]),
//!   pinned by identity to the session's known places ([`offer::identity`]), then the one pure
//!   rule decides; a tool call's card is cut down by the ruling ([`offer::tool_call_ruling`]).
//! - **Network paths** ([`network`]): a path that names a share, a device or a remote mount,
//!   or a link on the way to one, is found from its text before anything touches it, and its
//!   card is never offered. Every source goes through it, the tool-call watch included.
//! - **`DismissDenial`:** logs `DenialDismissed` for a denial a loaded thread holds.
//!
//! The log names a kind, a source or a count, never a path or a host.

mod check;
mod count;
pub(crate) mod feed;
pub(crate) mod network;
pub(crate) mod offer;
pub(crate) mod resolve;
mod tool_call;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use clauseo_protocol::{
    CommandOutcome, DenialDismissed, DenialId, DenialKind, DenialRecorded, DenialSource,
    DenialsPaused, ErrorCode, MatterId, StreamId, ThreadId, ToolCallId, ToolCallStatus, TurnId,
};
use clauseo_sandbox::{Denial, PathClass};

pub(crate) use check::{Checked, Decided};
#[cfg(test)]
pub(crate) use count::MAX_IN_FLIGHT;
pub(crate) use count::{Denials, Rate, Record, Recorded};
use offer::identity::Target;
use offer::{KnownMatter, Offer, OfferRules, Refused};
pub(crate) use tool_call::Seen;

use super::super::commands::{AfterCommand, CommandCtx};
use super::super::{Actor, Tag};
use super::out::Out;
use super::{cancel, running_turn, updates};
use crate::config::SessionKey;
use crate::error::Error;
use crate::log_file::Cause;

/// One refusal for the actor: the launch it came from, what the sandbox or the proxy reported,
/// and who reported it.
#[derive(Debug)]
pub(crate) struct DenialInput {
    pub(crate) key: SessionKey,
    pub(crate) kind: Denial,
    pub(crate) source: DenialSource,
}

/// `Input::Denial`: counts it toward the pause, and, past the rate cap, decides its card
/// (off the actor for a path, when a place among the thread's checks is free) and records it.
pub(crate) fn on_denial(actor: &mut Actor, input: DenialInput) {
    let DenialInput { key, kind, source } = input;
    let thread = key.thread;
    if actor.check_thread(thread).is_err() {
        tracing::debug!(%thread, "a denial for a thread the host no longer has was dropped");
        return;
    }
    let (counted, rate) = match &kind {
        Denial::Dropped { count } => (*count, Rate::Unrated),
        Denial::Path {
            class: PathClass::ReadOnly,
            ..
        }
        | Denial::UpstreamAuth { .. } => (0, Rate::Uncounted),
        Denial::Path { .. } | Denial::Host { .. } => (1, Rate::Counted),
        // The block list has its own notice and its own allow (decision record 0053).
        Denial::Blocked { .. } | Denial::UpstreamUnavailable { .. } | _ => {
            tracing::debug!(%thread, kind = word(&kind), "a denial left to its own path");
            return;
        }
    };
    let turn = running_turn(actor, thread);
    prune(actor);
    let now = actor.now().mono;
    let admitted = actor.denials.admit(thread, turn, now, counted, rate);
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
        Decided::of_text(offer::offer(&rules, refused), host)
    };
    let decided = match kind {
        Denial::Path { path, class, .. } => {
            let Some(flight) = actor.denials.start_check(thread) else {
                return dropped_check(actor, thread, turn, counted == 0);
            };
            return check::check_path(actor, key, turn, source, rules, (path, class), flight);
        }
        Denial::Host {
            host,
            why,
            shared_hosting,
            ..
        } => site(&host, shared_hosting, why),
        Denial::UpstreamAuth { upstream } => {
            let refused = Refused::UpstreamAuth {
                upstream: &upstream,
            };
            Decided::of_text(offer::offer(&rules, refused), &upstream)
        }
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
            decided,
        },
    );
}

/// A check with no place free among the thread's: dropped, and counted toward the pause as a
/// `Dropped` is (`count`: unless the intake counted it already).
fn dropped_check(actor: &mut Actor, thread: ThreadId, turn: Option<TurnId>, count: bool) {
    tracing::debug!(%thread, "a denial's check was dropped: the thread's checks are all running");
    if !count {
        return;
    }
    if let Some((turn, refused)) = actor.denials.dropped_check(thread, turn) {
        pause(actor, thread, turn, refused);
    }
}

/// Forgets the denials of threads the host no longer has: on each denial and each tool call's
/// end.
fn prune(actor: &mut Actor) {
    let threads = &actor.threads;
    actor.denials.prune(|thread| threads.contains_key(&thread));
}

/// `TurnInput::Denial`: a path's check came back.
pub(crate) fn on_checked(actor: &mut Actor, checked: Checked) {
    record(actor, checked);
}

/// Records a decided denial: a new card, or its count grown. A tool call's card is cut down by
/// the ruling first ([`offer::tool_call_ruling`]), and it counts toward the pause here.
fn record(actor: &mut Actor, checked: Checked) {
    let Checked {
        thread,
        turn,
        source,
        decided,
    } = checked;
    let os = !matches!(source, DenialSource::ToolCall { .. });
    let (offer, card) = ruled(&decided, os, |asked| {
        actor.denials.confirmed(thread, turn, asked)
    });
    let wall = actor.now().wall;
    let running = running_turn(actor, thread);
    let mut denials = std::mem::take(&mut actor.denials);
    let held = Record {
        kind: &offer.kind,
        offered: offer.offered,
        card: &card,
        asked: decided.asked.as_ref(),
        os,
    };
    let recorded = denials.record(thread, turn, running, held, || actor.ids.mint(wall));
    actor.denials = denials;
    let (denial, count, paused) = match recorded {
        Ok((Recorded::Log { denial, count }, paused)) => (denial, count, paused),
        Ok((Recorded::Capped { first }, _)) => {
            if first {
                tracing::info!(%thread, "the turn's denial cards are at their cap; new ones are not recorded");
            }
            return;
        }
        Ok((Recorded::Stale, _)) => {
            tracing::debug!(%thread, "a denial checked after its turn's cards were gone was dropped");
            return;
        }
        Err(error) => {
            tracing::warn!(%thread, error = %Cause(&error), "a denial was not recorded");
            return;
        }
    };
    if let (1, Some(why)) = (count, decided.unchecked) {
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
    if let Some((turn, refused)) = paused {
        pause(actor, thread, turn, refused);
    }
}

/// The card a decided denial gets from `os` (the sandbox, the proxy) or a tool call, and what
/// it is about: a tool call's cut down by the ruling, `confirmed` telling whether an OS report
/// of the same file came in its turn.
pub(crate) fn ruled(
    decided: &Decided,
    os: bool,
    confirmed: impl FnOnce(&Target) -> bool,
) -> (Offer, Target) {
    match (&decided.facts, &decided.asked) {
        (Some(facts), Some(asked)) if !os => {
            let ruled = offer::tool_call_ruling(decided.offer.clone(), facts, confirmed(asked));
            // A one-file card is about the file asked for.
            let card = if ruled.kind == decided.offer.kind {
                decided.card.clone()
            } else {
                asked.clone()
            };
            (ruled, card)
        }
        _ => (decided.offer.clone(), decided.card.clone()),
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
/// a row" again or not (only for a call seen running: an end for a call never seen changes
/// nothing), and reads a `failed` end with an access-denied message as a refusal.
pub(super) fn tool_seen(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    call: ToolCallId,
    seen: Seen<'_>,
) {
    let running = actor.denials.watching(thread, turn, call);
    actor.denials.watch(thread, turn, call, seen.paths());
    let completed = match seen.status {
        Some(ToolCallStatus::Completed) => true,
        Some(ToolCallStatus::Failed | ToolCallStatus::Cancelled) => false,
        _ => return,
    };
    prune(actor);
    let named = actor.denials.unwatch(thread, turn, call);
    if seen.failed_access_denied() && !named.is_empty() {
        tool_call_denial(actor, thread, turn, call, named);
    }
    if running {
        actor.denials.tool_ended(thread, turn, completed);
    }
}

/// A failed tool call's refusal: through the intake's rate cap and the thread's checks in
/// flight before anything starts off the actor; one past either is dropped and counted toward
/// the pause. Counted otherwise when recorded (and only when an OS report of it has not been).
fn tool_call_denial(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    call: ToolCallId,
    named: Vec<String>,
) {
    let Ok(matter) = actor.check_thread(thread).map(|entry| entry.row.matter) else {
        return;
    };
    let now = actor.now().mono;
    let admitted = actor
        .denials
        .admit(thread, Some(turn), now, 0, Rate::Counted);
    let flight = admitted
        .pass
        .then(|| actor.denials.start_check(thread))
        .flatten();
    let (Some(flight), Some(rules)) = (flight, rules_of(actor, matter)) else {
        return dropped_check(actor, thread, Some(turn), true);
    };
    check::check_tool_call(actor, (thread, turn, call), rules, named, flight);
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
