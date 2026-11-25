//! The commands that change an add-on (a skill or a tool), and where a matter's switches are
//! kept: `HostCommand::SetAddonScope`, `SetAddonOn` and `RemoveAddon`, and [`AddonSwitches`],
//! the store's `matters.addon_switches` (schema 8).
//!
//! What the words mean (decision records 0040 and 0009): an item's scope is `all` (on in every
//! matter unless a matter switches it off) or `matter` (off except in the matters that switch it
//! on); a matter's switch, when it has one, decides the item for that matter. An item on
//! nowhere (a skill not turned on, a tool not enabled) is on in no matter, switch or not.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{AddonKind, AddonScope, Error, MatterId};

/// Most bytes in an add-on id a command carries: a skill pack's id is at most 30 and a tool's
/// server name at most 64, so this bound only keeps a command small.
pub const MAX_ADDON_ID_BYTES: usize = 256;

/// `HostCommand::SetAddonScope`: where the item is on. JSON: `{"set_addon_scope": {"kind":
/// "skill", "id": "pdf", "scope": {"matter": "…"}}}` or `"scope": "all"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SetAddonScope {
    /// A skill or a tool.
    pub kind: AddonKind,
    /// The skill pack's id or the tool's server name.
    pub id: String,
    /// `all`: every matter's switch for it is cleared. `{"matter": X}`: they are cleared, then
    /// X's is set on. Either way the item is turned on if it was not.
    pub scope: AddonScope,
}

/// `HostCommand::SetAddonOn`: one matter's switch for the item, or with no matter the item on
/// or off in every matter at once. JSON: `{"set_addon_on": {"kind": "tool", "id": "linear",
/// "matter": "…", "on": false}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SetAddonOn {
    /// A skill or a tool.
    pub kind: AddonKind,
    /// The skill pack's id or the tool's server name.
    pub id: String,
    /// The matter. Left out, the change is for every matter in one commit: `on: true` is
    /// `SetAddonScope` `all` (every switch cleared, turned on if it was not); `on: false` makes
    /// its scope `matter` and clears every switch, so it is on in no matter. Clauseo's own
    /// tools take only a matter's switch (`addon.built_in` without one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matter: Option<MatterId>,
    /// Whether it is on in that matter. On for an item that is on nowhere turns it on and makes
    /// its scope `matter` first, so it is on in this matter only.
    pub on: bool,
}

/// `HostCommand::RemoveAddon`: removes an added item and clears every matter's switch for it.
/// JSON: `{"remove_addon": {"kind": "skill", "id": "pdf"}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct RemoveAddon {
    /// A skill or a tool.
    pub kind: AddonKind,
    /// The skill pack's id or the tool's server name.
    pub id: String,
}

/// The shape every add-on command shares: an id of 1 to [`MAX_ADDON_ID_BYTES`] bytes. Whether
/// it names an item is the host's check (`addon.unknown`).
pub(crate) fn check_id(kind: &'static str, id: &str) -> Result<(), Error> {
    if id.is_empty() || id.len() > MAX_ADDON_ID_BYTES {
        return Err(Error::Format {
            kind,
            rule: "an add-on id is 1 to 256 bytes",
        });
    }
    Ok(())
}

/// A matter's switches, by kind and id: `{"skill": {"pdf": true}, "tool": {"linear": false}}`.
/// An item with no entry follows its scope. The store keeps it as JSON in
/// `matters.addon_switches`, NULL when empty.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddonSwitches {
    /// The skills' switches, by pack id or built-in name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub skill: BTreeMap<String, bool>,
    /// The tools' switches, by server name (`case-law`, `browser` for Clauseo's own).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tool: BTreeMap<String, bool>,
}

impl AddonSwitches {
    /// The switch for the item, if the matter has one.
    pub fn get(&self, kind: AddonKind, id: &str) -> Option<bool> {
        self.of(kind).get(id).copied()
    }

    /// Sets the switch for the item, or with `None` clears it.
    pub fn set(&mut self, kind: AddonKind, id: &str, on: Option<bool>) {
        let map = match kind {
            AddonKind::Skill => &mut self.skill,
            AddonKind::Tool => &mut self.tool,
        };
        match on {
            Some(on) => {
                map.insert(id.to_owned(), on);
            }
            None => {
                map.remove(id);
            }
        }
    }

    /// Whether no item has a switch.
    pub fn is_empty(&self) -> bool {
        self.skill.is_empty() && self.tool.is_empty()
    }

    /// One kind's switches.
    pub fn of(&self, kind: AddonKind) -> &BTreeMap<String, bool> {
        match kind {
            AddonKind::Skill => &self.skill,
            AddonKind::Tool => &self.tool,
        }
    }
}
