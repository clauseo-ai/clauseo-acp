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
//! Every row's scope is `all` in this version, and the query's matter changes nothing yet.

mod tools;

use clauseo_profile::packs::index::use_text;
use clauseo_profile::packs::{Pack, PackSkill};
use clauseo_profile::skills::builtin::{AGENT_BROWSER, BUILTIN, use_of};
use clauseo_profile::skills::stetdoc::STETDOC;
use clauseo_protocol::{
    Addon, AddonFrom, AddonFromKind, AddonKind, AddonList, AddonScope, AddonSource, AddonStatus,
    AgentKind, HostEvent, MatterId, PackReason, PackRefusal, PackState, PackStatus, QueryResult,
};
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
/// background (`connectors/recheck.rs`); the answer never waits for it. `_matter` is accepted
/// for per-matter switches (D2) and changes nothing yet.
pub(super) fn list(actor: &mut Actor, _matter: Option<MatterId>, reply: Reply<QueryResult>) {
    crate::actor::connectors::recheck::on_listed(actor);
    let builtin = builtin_skills(actor);
    let tools = tools::rows(actor);
    let locked = actor.effective.is_locked(ENABLED);
    let listing = Listing::of(actor);
    let clock = std::sync::Arc::clone(&actor.clock);
    let spawned = actor.spawner.spawn_local(async move {
        let packs = off_actor(clock, READ_JOB, move || Ok(read_all(&listing))).await;
        let answer = packs
            .map(|packs| {
                let mut items = builtin;
                items.extend(
                    packs
                        .iter()
                        .map(|(status, pack)| added_skill(status, pack.as_ref(), locked)),
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
