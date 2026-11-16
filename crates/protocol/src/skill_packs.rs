//! The skill packs as the app lists them (decision record 0040, "Turning packs on (PR-E)"):
//! one [`PackStatus`] per pack folder, and per pack `skills.enabled` names, answering
//! `HostQuery::ListSkillPacks`. `HostCommand::SetSkillPack` turns one on or off.
//!
//! These are view types: the app and the host ship together, so every enum here is closed
//! (decision record 0002): a new state or reason is added on both sides at once.

use serde::{Deserialize, Serialize};

use crate::{Date, PackId};

/// One pack as `ListSkillPacks` lists it. The manifest's fields are there when the pack read
/// and checked (`state` `on`, `off` or `changed` from a stale hash, and `not_admitted` for a
/// valid pack); each is left out when it failed the listable filter (a control or format
/// character), and for a pack that did not read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PackStatus {
    /// The pack's id: its folder's name.
    pub id: PackId,
    /// Its name, for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Its version, for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Its author, for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Its licence, for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licence: Option<String>,
    /// Where it came from: an `https` URL, never fetched by Clauseo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    /// When its author last reviewed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reviewed: Option<Date>,
    /// Whether it promises to hold no script.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_only: Option<bool>,
    /// Whether it loads, and if not, why not in one word.
    pub state: PackState,
    /// Why it is `not_admitted` or `invalid`; left out for the other states.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<PackReason>,
    /// The hash its files have now (`sha256:<64 hex>`), what `SetSkillPack` would record, and
    /// what a firm that locks `skills.enabled` puts in its policy. Left out when it did not
    /// read whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

/// Whether a pack loads in the next launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum PackState {
    /// Turned on, and its files hash to the hash recorded then: it loads.
    On,
    /// Valid and admitted, but not turned on.
    Off,
    /// `skills.extra` (or the firm's `skills.curated` list) does not admit it: it cannot load
    /// or be turned on, whatever else holds. `reason` says whether a firm decided.
    NotAdmitted,
    /// Turned on, but its files no longer hash to the hash recorded, or they changed while
    /// they were read: it does not load until it is turned on again, which the app offers.
    Changed,
    /// It breaks a pack rule, or a pack turned on has no folder: `reason` gives the rule.
    Invalid,
}

/// Why a pack is `not_admitted` or `invalid`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum PackReason {
    /// Not admitted. `by_policy`: the firm's policy decided (`skills.extra` locked, or the
    /// firm's `skills.curated` list leaves it out); else the lawyer's own `skills.extra`.
    NotAdmitted {
        /// Whether a firm decided.
        by_policy: bool,
    },
    /// Invalid, by the rule `kind`.
    Invalid {
        /// The rule it breaks.
        kind: PackRefusal,
    },
}

/// The pack rule a pack breaks: `profile::packs::Refusal`, one for one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum PackRefusal {
    /// No folder named exactly the id: also a pack turned on whose folder is gone.
    Missing,
    /// A symlink, a junction or another reparse point inside the pack, or at it.
    Link,
    /// A file with more than one hard link.
    HardLink,
    /// Something that is neither a file nor a folder.
    NotAFile,
    /// A name that is not UTF-8, or a path that is not plain, too long, too deep, or given
    /// twice in different cases.
    BadPath,
    /// A file or folder the layout does not allow, or a listed skill with no `SKILL.md`.
    Unlisted,
    /// Past a size or count cap.
    TooBig,
    /// `clauseo-pack.json` does not parse, has an unknown key or a bad field.
    Manifest,
    /// The manifest's id is not a pack id, or is reserved.
    BadId,
    /// The folder's name is not the manifest's id.
    IdMismatch,
    /// A script or executable, and `skills.allow_scripts` is off.
    Script,
    /// A script or executable in a pack whose manifest says `instruction_only`.
    ScriptInInstructionOnly,
    /// A skill's `SKILL.md` breaks a rule, or a file holds a shell injection.
    Skill,
    /// A read failed.
    Io,
}
