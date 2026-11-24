//! `SetAddonScope`, `SetAddonOn` and `RemoveAddon` (decision record 0040, Scope and per-matter
//! switches). The settings half (a pack turned on with its hash, a server enabled, a scope) is
//! written first, then the matters' switches go in one commit with the receipt
//! (`Commit::more_matters`), the host's rows changed at once and put back if it fails. Then
//! `SettingsChanged` when the settings changed, the MCP view, and `AddonsChanged`.
//!
//! - A skill (`skills.rs`): a pack not turned on is read off the actor first, as
//!   `SetSkillPack` reads it, and turning it on is refused as that command refuses it.
//!   Clauseo's own skills are `addon.built_in` for every change.
//! - A tool (`tools.rs`): the lawyer's server, on the actor, queued behind running MCP keychain
//!   work; Clauseo's `case-law` and `browser` take a matter's switch only.
//! - Turning an item on in a matter while it is on nowhere turns it on with scope `matter` and
//!   clears every other matter's switch, so it is on in that matter only.
//! - A switch equal to the scope's default is stored as none, so `switched` is what is stored.
//! - `SetAddonOn` with no matter is one commit for every matter: on is `SetAddonScope` `all`;
//!   off makes the scope `matter` with no switch. Clauseo's own tools refuse it (`addon.built_in`).

mod skills;
pub(in crate::actor) mod tools;

use clauseo_protocol::{
    AddonKind, AddonScope, CommandOutcome, ErrorCode, HostEvent, MatterId, RemoveAddon, SetAddonOn,
    SetAddonScope,
};
use clauseo_store::MatterRow;

pub(in crate::actor) use skills::{SkillBack, on_back};

use super::commands::{AfterCommand, CommandCtx};
use super::{Actor, Tag};
use crate::error::Error;
use crate::pipeline::Draft;

/// What a command does to the matters' switches once the settings half is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::actor) enum Want {
    /// One matter's switch.
    On { matter: MatterId, on: bool },
    /// Every switch cleared, then with `Some` that matter's set on.
    Only(Option<MatterId>),
}

/// `SetAddonScope`.
pub(in crate::actor) fn set_scope(actor: &mut Actor, ctx: &CommandCtx, change: &SetAddonScope) {
    let only = match change.scope {
        AddonScope::All => None,
        AddonScope::Matter(matter) => Some(matter),
    };
    run(actor, ctx, (change.kind, &change.id), Change::Scope(only));
}

/// `SetAddonOn`. Without a matter: on is `SetAddonScope` `all`, off is [`Change::Nowhere`].
pub(in crate::actor) fn set_on(actor: &mut Actor, ctx: &CommandCtx, change: &SetAddonOn) {
    let order = match (change.matter, change.on) {
        (Some(matter), on) => Change::On { matter, on },
        (None, true) => Change::Scope(None),
        (None, false) => Change::Nowhere,
    };
    run(actor, ctx, (change.kind, &change.id), order);
}

/// `RemoveAddon`.
pub(in crate::actor) fn remove(actor: &mut Actor, ctx: &CommandCtx, change: &RemoveAddon) {
    run(actor, ctx, (change.kind, &change.id), Change::Remove);
}

/// One of the three commands, to its kind's half.
fn run(actor: &mut Actor, ctx: &CommandCtx, (kind, id): (AddonKind, &str), change: Change) {
    let started = match kind {
        AddonKind::Skill => skills::start(actor, ctx, id, change),
        AddonKind::Tool => {
            tools::run(actor, ctx, id, change);
            Ok(())
        }
    };
    if let Err(error) = started {
        refused(actor, ctx, kind, error);
    }
}

/// The change a command asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::actor) enum Change {
    /// `SetAddonScope`: `None` for `all`.
    Scope(Option<MatterId>),
    /// `SetAddonOn`.
    On { matter: MatterId, on: bool },
    /// `SetAddonOn` off with no matter: scope `matter` and every switch cleared, so it is on in
    /// no matter. A scope change for the firm's lock.
    Nowhere,
    /// `RemoveAddon`.
    Remove,
}

impl Change {
    /// The matter it names, if any: one the host must know.
    fn matter(self) -> Option<MatterId> {
        match self {
            Self::Scope(matter) => matter,
            Self::On { matter, .. } => Some(matter),
            Self::Nowhere | Self::Remove => None,
        }
    }
}

/// `addon.unknown` for `id`.
fn unknown(kind: AddonKind, id: &str) -> Error {
    Error::refused(ErrorCode::AddonUnknown, format!("no {} {id}", noun(kind)))
}

/// `addon.built_in` for `id`.
fn built_in(id: &str) -> Error {
    Error::refused(ErrorCode::AddonBuiltIn, format!("{id} is built in"))
}

/// `settings.locked` for `key`.
fn locked(key: &str) -> Error {
    Error::Settings(clauseo_settings::Error::Locked {
        key: key.to_owned(),
    })
}

fn noun(kind: AddonKind) -> &'static str {
    match kind {
        AddonKind::Skill => "skill",
        AddonKind::Tool => "tool",
    }
}

/// The settings half is written (`settings`: whether it changed anything a window shows):
/// the matters' switches, the commit with the receipt, then what every window is told.
pub(in crate::actor) fn finish(
    actor: &mut Actor,
    ctx: &CommandCtx,
    (kind, id): (AddonKind, &str),
    want: Want,
    settings: bool,
) {
    let all = super::addon_rules::scope_all(&actor.effective, kind, id)
        || (kind == AddonKind::Tool && super::addon_rules::builtin_tool(id));
    let rows = changed_rows(actor, kind, id, want, all);
    tracing::info!(
        kind = noun(kind),
        matters = rows.len(),
        "a skill's or tool's scope or switch changed"
    );
    let sent = if rows.is_empty() {
        super::settings::receipt(actor, ctx)
    } else {
        commit_rows(actor, ctx, rows)
    };
    if settings {
        let changed = HostEvent::SettingsChanged(super::settings::view(actor));
        super::app::publish(actor, &changed);
    }
    if kind == AddonKind::Tool {
        super::mcp::view::publish(actor);
    }
    actor.mcp.tools_rev += 1;
    super::query::addons::changed(actor);
    if let Err(error) = sent {
        actor.finish_command(ctx.id, Err(error));
    }
}

/// Each known matter's row the change alters, as (before, after).
fn changed_rows(
    actor: &Actor,
    kind: AddonKind,
    id: &str,
    want: Want,
    all: bool,
) -> Vec<(MatterRow, MatterRow)> {
    actor
        .matters
        .values()
        .filter_map(|entry| {
            let before = &entry.row;
            let mut switches = before.addon_switches.clone().unwrap_or_default();
            let value = match want {
                Want::On { matter, on } if matter == before.id => Some((on != all).then_some(on)),
                Want::On { .. } => None,
                Want::Only(Some(matter)) if matter == before.id => Some((!all).then_some(true)),
                Want::Only(_) => Some(None),
            };
            let value = value?;
            if switches.get(kind, id) == value {
                return None;
            }
            switches.set(kind, id, value);
            let after = MatterRow {
                addon_switches: (!switches.is_empty()).then_some(switches),
                ..before.clone()
            };
            Some((before.clone(), after))
        })
        .collect()
}

/// Puts each of `ids`, skills `AddSkills` just recorded (`settings/add_skills.rs`), in
/// `scope` as `SetAddonScope` would: its `skills.scope`, then every matter's switch cleared and,
/// for a matter scope, that matter's set on. The host's rows change now; the rows to commit
/// come back, one per matter with its first `before`. With `switches` false (an add that
/// stopped, with no commit to carry rows) only the scopes are set: under a matter scope the
/// skill is then on in no matter until it is switched on.
pub(in crate::actor) fn place_added(
    actor: &mut Actor,
    ids: &[&str],
    scope: AddonScope,
    switches: bool,
) -> Result<Vec<(MatterRow, MatterRow)>, Error> {
    let (only, item) = match scope {
        AddonScope::All => (None, clauseo_settings::ItemScope::All),
        AddonScope::Matter(matter) => (Some(matter), clauseo_settings::ItemScope::Matter),
    };
    let mut merged: Vec<(MatterRow, MatterRow)> = Vec::new();
    // A matter scope under the firm's lock was refused before the add (`scope_ok`); `all` leaves
    // a scope the firm sets as it is.
    let firm_scopes = actor.effective.is_locked("skills.scope");
    for id in ids {
        if actor.effective.skill_scope(id) != item && !firm_scopes {
            actor
                .effective
                .set_skill_scope(&clauseo_settings::SystemFiles, id, item)?;
        }
        if !switches {
            continue;
        }
        let all = only.is_none();
        for (before, after) in changed_rows(actor, AddonKind::Skill, id, Want::Only(only), all) {
            if let Some(entry) = actor.matters.get_mut(&after.id) {
                entry.row = after.clone();
            }
            match merged.iter_mut().find(|(_, last)| last.id == after.id) {
                Some((_, last)) => *last = after,
                None => merged.push((before, after)),
            }
        }
    }
    // Live agents are compared again against the switches and scopes.
    actor.mcp.tools_rev += 1;
    Ok(merged)
}

/// The receipt for `ctx`, with `rows` (from [`place_added`], already the host's) in its commit.
pub(in crate::actor) fn receipt_with_rows(
    actor: &mut Actor,
    ctx: &CommandCtx,
    rows: Vec<(MatterRow, MatterRow)>,
) -> Result<(), Error> {
    if rows.is_empty() {
        super::settings::receipt(actor, ctx)
    } else {
        commit_rows(actor, ctx, rows)
    }
}

/// Sends the rows and the receipt in one commit, with the host's rows changed now and put
/// back if it fails.
fn commit_rows(
    actor: &mut Actor,
    ctx: &CommandCtx,
    rows: Vec<(MatterRow, MatterRow)>,
) -> Result<(), Error> {
    let mut draft = Draft::new();
    let mut old_rows = Vec::with_capacity(rows.len());
    let mut new_rows = Vec::with_capacity(rows.len());
    for (before, after) in rows {
        if let Some(entry) = actor.matters.get_mut(&after.id) {
            entry.row = after.clone();
        }
        old_rows.push(before);
        new_rows.push(after);
    }
    let mut new_rows = new_rows.into_iter();
    if let Some(first) = new_rows.next() {
        draft.matter_row(first);
    }
    draft.more_matter_rows(new_rows.collect());
    draft.receipt(ctx.receipt(None, CommandOutcome::Done, actor.now().wall));
    let tag = Tag::Command {
        id: ctx.id,
        outcome: CommandOutcome::Done,
        after: AfterCommand::MatterRows {
            before: old_rows.clone(),
        },
    };
    let sent = actor.send_commit(draft, tag).map(|_| ());
    if sent.is_err() {
        rows_failed(actor, old_rows);
    }
    sent
}

/// Puts the rows of a commit that failed back.
pub(in crate::actor) fn rows_failed(actor: &mut Actor, before: Vec<MatterRow>) {
    for row in before {
        super::matter_details::row_failed(actor, row);
    }
    // Live agents are compared again against the rows put back.
    actor.mcp.tools_rev += 1;
}

/// Sends the receipt of a Settings command that removed (`to` none) or renamed an item, with
/// every matter's switch for `from` cleared, or moved to `to`, in the same commit: a server
/// added again under its old name is not off somewhere unseen, and a renamed one keeps its
/// matters.
pub(in crate::actor) fn receipt_moving(
    actor: &mut Actor,
    ctx: &CommandCtx,
    kind: AddonKind,
    (from, to): (&str, Option<&str>),
) -> Result<(), Error> {
    let rows: Vec<(MatterRow, MatterRow)> = actor
        .matters
        .values()
        .filter_map(|entry| {
            let before = &entry.row;
            let mut switches = before.addon_switches.clone()?;
            let on = switches.get(kind, from);
            // A switch left for the new name (an earlier commit that failed) goes too.
            let stale = to.and_then(|to| switches.get(kind, to));
            if on.is_none() && stale.is_none() {
                return None;
            }
            switches.set(kind, from, None);
            if let Some(to) = to {
                switches.set(kind, to, on);
            }
            let after = MatterRow {
                addon_switches: (!switches.is_empty()).then_some(switches),
                ..before.clone()
            };
            Some((before.clone(), after))
        })
        .collect();
    if rows.is_empty() {
        super::settings::receipt(actor, ctx)
    } else {
        commit_rows(actor, ctx, rows)
    }
}

/// Sends the receipt of a `SaveServer` that gave a scope, with every matter's switch for the
/// server `name` set as `SetAddonScope` sets it (`only`: the one matter on, or none for
/// `all`) and those of its old name, on a rename, cleared, in the same commit.
pub(in crate::actor) fn receipt_placing(
    actor: &mut Actor,
    ctx: &CommandCtx,
    (name, old): (&str, Option<&str>),
    only: Option<MatterId>,
) -> Result<(), Error> {
    let kind = AddonKind::Tool;
    let rows: Vec<(MatterRow, MatterRow)> = actor
        .matters
        .values()
        .filter_map(|entry| {
            let before = &entry.row;
            let held = before.addon_switches.clone().unwrap_or_default();
            let mut switches = held.clone();
            if let Some(old) = old {
                switches.set(kind, old, None);
            }
            switches.set(kind, name, (only == Some(before.id)).then_some(true));
            (switches != held).then(|| {
                let after = MatterRow {
                    addon_switches: (!switches.is_empty()).then_some(switches),
                    ..before.clone()
                };
                (before.clone(), after)
            })
        })
        .collect();
    super::query::addons::changed(actor);
    if rows.is_empty() {
        super::settings::receipt(actor, ctx)
    } else {
        commit_rows(actor, ctx, rows)
    }
}

/// Answers the command with `error`, and logs the kind and the code.
fn refused(actor: &mut Actor, ctx: &CommandCtx, kind: AddonKind, error: Error) {
    tracing::warn!(
        kind = noun(kind),
        code = error.code().as_str(),
        "a skill's or tool's scope or switch was not changed"
    );
    actor.finish_command(ctx.id, Err(error));
}
