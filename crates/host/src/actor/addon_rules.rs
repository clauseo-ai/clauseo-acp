//! Where a skill or tool is on (decision record 0040, Scope and per-matter switches): its
//! scope from the settings (`skills.scope`, `mcp.servers.<name>.scope`; Clauseo's own tools are
//! always `all`) and a matter's own switch (`matters.addon_switches`). An item applies in a
//! matter when the matter's switch says so, else when its scope is `all`. Whether the item is on
//! at all (a pack turned on, a server enabled, `tools.browser`) is the caller's other half.

use std::collections::{BTreeMap, BTreeSet};

use clauseo_profile::skills::builtin::{AGENT_BROWSER, BUILTIN};
use clauseo_profile::skills::stetdoc::STETDOC;
use clauseo_protocol::{AddonKind, AddonSwitches, MatterId};
use clauseo_settings::{Effective, ItemScope, McpServerLaunch};

use super::Actor;

/// Clauseo's own tools, as `ListAddons` names them.
pub(crate) const CASE_LAW: &str = "case-law";
/// Clauseo's own browser tool.
pub(crate) const BROWSER: &str = "browser";

/// Whether `id` is one of Clauseo's own skills.
pub(in crate::actor) fn builtin_skill(id: &str) -> bool {
    BUILTIN.contains(&id) || id == STETDOC || id == AGENT_BROWSER
}

/// Whether `id` is one of Clauseo's own tools.
pub(in crate::actor) fn builtin_tool(id: &str) -> bool {
    id == CASE_LAW || id == BROWSER
}

/// Whether the item's scope is `all`: the default a matter with no switch follows.
pub(in crate::actor) fn scope_all(effective: &Effective, kind: AddonKind, id: &str) -> bool {
    match kind {
        AddonKind::Skill => effective.skill_scope(id) == ItemScope::All,
        AddonKind::Tool => effective
            .mcp_servers()
            .get(id)
            .is_none_or(|server| !server.matter_scoped()),
    }
}

/// Whether the item applies under `switches` (a matter's), given its scope.
pub(in crate::actor) fn applies(
    switches: Option<&AddonSwitches>,
    kind: AddonKind,
    id: &str,
    scope_all: bool,
) -> bool {
    switches
        .and_then(|switches| switches.get(kind, id))
        .unwrap_or(scope_all)
}

/// The switches of `matter`, when the host knows it and it has any.
pub(in crate::actor) fn switches_of(actor: &Actor, matter: MatterId) -> Option<&AddonSwitches> {
    actor
        .matters
        .get(&matter)
        .and_then(|entry| entry.row.addon_switches.as_ref())
}

/// What one matter gets (a launch's view): the settings, and the matter's switches.
#[derive(Clone, Copy)]
pub(crate) struct Placed<'a> {
    effective: &'a Effective,
    switches: Option<&'a AddonSwitches>,
}

impl<'a> Placed<'a> {
    /// The matter `matter` (a matter the host does not know has no switch).
    pub(crate) fn of(actor: &'a Actor, matter: MatterId) -> Self {
        Self {
            effective: &actor.effective,
            switches: switches_of(actor, matter),
        }
    }

    /// Whether the item's scope and the matter's switch put it in the matter.
    fn applies(&self, kind: AddonKind, id: &str) -> bool {
        applies(self.switches, kind, id, scope_all(self.effective, kind, id))
    }

    /// The enabled servers on in the matter, by name.
    pub(crate) fn servers(&self) -> impl Iterator<Item = (&'a str, &'a McpServerLaunch)> + '_ {
        self.effective
            .enabled_mcp_servers()
            .filter(|(name, _)| self.applies(AddonKind::Tool, name))
    }

    /// The enabled servers the matter leaves out, by name.
    pub(crate) fn servers_off(&self) -> BTreeSet<String> {
        self.effective
            .enabled_mcp_servers()
            .filter(|(name, _)| !self.applies(AddonKind::Tool, name))
            .map(|(name, _)| name.to_owned())
            .collect()
    }

    /// Whether the pack `id` (if turned on) is on in the matter.
    pub(crate) fn skill_on(&self, id: &str) -> bool {
        self.applies(AddonKind::Skill, id)
    }

    /// The packs turned on and on in the matter, with their recorded hashes.
    pub(crate) fn skills(&self) -> BTreeMap<String, String> {
        self.effective
            .settings()
            .skills
            .enabled
            .iter()
            .filter(|(id, _)| self.skill_on(id))
            .map(|(id, hash)| (id.clone(), hash.clone()))
            .collect()
    }

    /// Whether the matter leaves Clauseo's tool `name` (`case-law`, `browser`) on: its
    /// switch, else on.
    pub(crate) fn builtin_on(&self, name: &str) -> bool {
        applies(self.switches, AddonKind::Tool, name, true)
    }

    /// Clauseo's own tools the matter switches off.
    pub(crate) fn builtin_off(&self) -> BTreeSet<&'static str> {
        [CASE_LAW, BROWSER]
            .into_iter()
            .filter(|name| !self.builtin_on(name))
            .collect()
    }
}
