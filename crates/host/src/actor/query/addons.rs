//! `ListAddons`: every skill and tool, built in and added, in one list for the "Skills and
//! Connectors" place, and [`changed`], which sends `AddonsChanged` after a change to it.
//!
//! Skills first, then tools; within each, built-ins first, then added ones by id.
//! - Built-in skills: the three of `profile::skills::builtin::BUILTIN`, then `stetdoc` when the
//!   profile rendered it (it loaded) and `agent-browser` while `tools.browser` is on and the app
//!   gave the browser's shell tool. Each has its fixed `USES` line, is on and ready, and cannot
//!   be removed.
//! - Added skills: one row per pack `ListSkillPacks` lists (`skill_packs.rs`, read off the
//!   actor the same way), with its first skill's description cut as Codex's index cuts it.
//! - Tools (`addons/tools.rs`): `case-law` (always, whatever its liveness check says) and
//!   `browser`, then the lawyer's MCP servers as Settings › Tools shows them (`mcp/view.rs`).
//!
//! Each row's `scope`, `on` and `switched` follow `addon_rules.rs`: `on` is whether it is on in
//! the matter asked about, or without one in a matter with no switch for it. Clauseo's own
//! skills are always `all` and on; its tools are `all`, switched per matter.

pub(crate) mod tools;

use clauseo_profile::packs::index::use_text;
use clauseo_profile::packs::{Pack, PackSkill};
use clauseo_profile::skills::builtin::{AGENT_BROWSER, BUILTIN, use_of};
use clauseo_profile::skills::stetdoc::STETDOC;
use std::collections::BTreeSet;

use clauseo_protocol::{
    Addon, AddonFrom, AddonFromKind, AddonKind, AddonList, AddonScope, AddonSource, AddonStatus,
    AddonSwitches, AgentKind, HostEvent, MatterId, MatterKind, PackReason, PackRefusal, PackState,
    PackStatus, QueryResult,
};
use clauseo_settings::ItemScope;
use futures::task::LocalSpawnExt;

use super::skill_packs::{Listing, read_all};
use crate::actor::Actor;
use crate::actor::input::Reply;
use crate::actor::settings::packs::READ_JOB;
use crate::agent::codex::prepare::off_actor;
use crate::error::Error;

/// The policy key that locks every pack's switch.
const ENABLED: &str = "skills.enabled";

/// Answers `ListAddons` from a task of its own: the built-ins and the tools from the actor now,
/// the packs read off it. While case law is `unreachable` the read starts a fresh check in the
/// background (`connectors/recheck.rs`); the answer never waits for it.
pub(super) fn list(actor: &mut Actor, matter: Option<MatterId>, reply: Reply<QueryResult>) {
    crate::actor::connectors::recheck::on_listed(actor);
    let builtin = builtin_skills(actor);
    let tools = tools::rows(actor, matter);
    let locked = actor.effective.is_locked(ENABLED);
    let listing = Listing::of(actor);
    let places = Places::of(actor, matter);
    let clock = std::sync::Arc::clone(&actor.clock);
    let spawned = actor.spawner.spawn_local(async move {
        let packs = off_actor(clock, READ_JOB, move || Ok(read_all(&listing))).await;
        let answer = packs
            .map(|packs| {
                let mut items = builtin;
                items.extend(
                    packs
                        .iter()
                        .map(|(status, pack)| added_skill(status, pack.as_ref(), locked))
                        .map(|row| places.placed(row)),
                );
                items.extend(tools);
                QueryResult::Addons(AddonList { items })
            })
            .map_err(|_unanswered| {
                Error::Internal("the skills and tools list did not answer".into())
            });
        let _gone = reply.send(answer);
    });
    if let Err(error) = spawned {
        tracing::error!(%error, "the host pool refused a skills and tools list");
    }
}

/// Sends `AddonsChanged` to every app subscriber: the list may read differently now.
pub(in crate::actor) fn changed(actor: &mut Actor) {
    crate::actor::app::publish(actor, &HostEvent::AddonsChanged {});
}

/// What places a row: each item's scope, the matter asked about, and every matter's switches,
/// taken on the actor so the list can be finished off it.
pub(super) struct Places {
    matter: Option<MatterId>,
    /// The packs whose scope is `matter`.
    matter_scoped: BTreeSet<String>,
    switches: Vec<(MatterId, AddonSwitches)>,
    /// The matter a scope `matter` names when none is switched on: the Scratch matter, else the
    /// first.
    fallback: Option<MatterId>,
    /// The firm's locks: a locked item with scope `all` ignores every matter's switch.
    locks: crate::actor::addon_rules::Locks,
}

impl Places {
    pub(super) fn of(actor: &Actor, matter: Option<MatterId>) -> Self {
        let rows = actor.matters.values().map(|entry| &entry.row);
        let scratch = rows.clone().find(|row| row.kind == MatterKind::Scratch);
        Self {
            matter,
            matter_scoped: actor
                .effective
                .settings()
                .skills
                .scope
                .iter()
                .filter(|(_, scope)| **scope == ItemScope::Matter)
                .map(|(id, _)| id.clone())
                .collect(),
            switches: rows
                .clone()
                .filter_map(|row| Some((row.id, row.addon_switches.clone()?)))
                .collect(),
            fallback: scratch.or(rows.clone().next()).map(|row| row.id),
            locks: crate::actor::addon_rules::Locks::of(&actor.effective),
        }
    }

    /// `row` (on when on anywhere) with its scope, `on` and `switched`. A tool's scope is
    /// given by `matter_scoped` from the caller; a skill's is read here.
    pub(super) fn placed(&self, row: Addon) -> Addon {
        let scoped = row.kind == AddonKind::Skill && self.matter_scoped.contains(&row.id);
        self.placed_as(row, !scoped)
    }

    /// [`Self::placed`] with the scope given: `all` when `all`.
    pub(super) fn placed_as(&self, mut row: Addon, all: bool) -> Addon {
        let (kind, item) = (row.kind, row.id.clone());
        // Under the firm's lock an item with scope `all` is on everywhere, as launches have it.
        let ignored = all && self.locks.covers(kind, &item);
        let switch = |matter: &MatterId| {
            self.switches
                .iter()
                .find(|(id, _)| id == matter)
                .and_then(|(_, switches)| switches.get(kind, &item))
                .filter(|_| !ignored)
        };
        row.switched = self
            .switches
            .iter()
            .filter(|(id, _)| switch(id).is_some_and(|on| on != all))
            .map(|(id, _)| *id)
            .collect();
        let here = self
            .matter
            .map_or(all, |matter| switch(&matter).unwrap_or(all));
        row.on = row.on && here;
        row.scope = if all {
            AddonScope::All
        } else {
            let first = self
                .matter
                .filter(|matter| row.switched.contains(matter))
                .or_else(|| row.switched.first().copied())
                .or(self.matter)
                .or(self.fallback);
            first.map_or(AddonScope::All, AddonScope::Matter)
        };
        row
    }
}

/// A row with the fields every row of this version shares.
fn row(kind: AddonKind, id: &str, name: &str, source: AddonSource) -> Addon {
    Addon {
        kind,
        id: id.to_owned(),
        name: name.to_owned(),
        description: String::new(),
        source,
        from: None,
        scope: AddonScope::All,
        on: true,
        has_scripts: None,
        status: AddonStatus::Ready,
        can_remove: false,
        locked: false,
        switched: Vec::new(),
        server: None,
    }
}

/// The built-in skills, in the order the module comment gives.
fn builtin_skills(actor: &Actor) -> Vec<Addon> {
    let claude = clauseo_protocol::AgentId::new(AgentKind::Claude.id())
        .map(|agent| actor.profile.launch_writes(&agent))
        .unwrap_or_default();
    let rendered = |name: &str| {
        claude
            .iter()
            .any(|write| write.rendered().skills().iter().any(|s| s.name() == name))
    };
    let browser = actor.effective.settings().tools.browser
        && (actor.profile.browser_tool().is_some() || rendered(AGENT_BROWSER));
    let mut names: Vec<&str> = BUILTIN.to_vec();
    if rendered(STETDOC) {
        names.push(STETDOC);
    }
    if browser {
        names.push(AGENT_BROWSER);
    }
    names
        .into_iter()
        .map(|id| Addon {
            description: builtin_description(id),
            has_scripts: Some(false),
            ..row(AddonKind::Skill, id, builtin_name(id), AddonSource::BuiltIn)
        })
        .collect()
}

/// A built-in skill's name, for display.
fn builtin_name(id: &str) -> &'static str {
    match id {
        "legal-drafting" => "Legal drafting",
        "legal-research" => "Legal research",
        "matter-brief" => "Matter brief",
        STETDOC => "Word documents",
        AGENT_BROWSER => "Web browser",
        _ => "Clauseo skill",
    }
}

/// A built-in skill's description: its `USES` line, as a sentence starts.
fn builtin_description(id: &str) -> String {
    let line = use_of(id).unwrap_or("using a web page that needs a real browser");
    let mut chars = line.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// A pack's row, from its status and the pack when it read. `locked`: policy locks
/// `skills.enabled`.
fn added_skill(status: &PackStatus, pack: Option<&Pack>, locked: bool) -> Addon {
    let id = status.id.as_str();
    let name = status.name.as_deref().unwrap_or(id);
    let description = pack
        .and_then(|pack| pack.skills().first())
        .map(PackSkill::description)
        .and_then(use_text)
        .unwrap_or_default();
    let from = if status.added_here {
        Some(AddonFrom {
            kind: AddonFromKind::Folder,
            text: id.to_owned(),
        })
    } else {
        status.source_url.as_ref().map(|url| AddonFrom {
            kind: AddonFromKind::Url,
            text: url.clone(),
        })
    };
    let script_refusal = matches!(
        status.reason,
        Some(PackReason::Invalid {
            kind: PackRefusal::Script | PackRefusal::ScriptInInstructionOnly
        })
    );
    Addon {
        description,
        from,
        on: status.state == PackState::On,
        has_scripts: Some(pack.map_or(script_refusal, Pack::has_scripts)),
        status: match status.state {
            PackState::On | PackState::Off => AddonStatus::Ready,
            PackState::Changed => AddonStatus::Changed,
            PackState::Invalid => AddonStatus::Invalid,
            PackState::NotAdmitted => AddonStatus::Blocked,
        },
        can_remove: status.added_here,
        locked,
        ..row(AddonKind::Skill, id, name, AddonSource::Added)
    }
}
