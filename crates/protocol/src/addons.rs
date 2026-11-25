//! Skills and tools (MCP servers) in one list, for the "Skills and Connectors" place: one
//! [`Addon`] row per skill or tool, built in or added, answering `HostQuery::ListAddons`
//! (`QueryResult::Addons`). `HostEvent::AddonsChanged` tells app subscribers to read it again.
//!
//! These are view types: the app and the host ship together, so every enum here is closed
//! (decision record 0002): a new kind, source or status is added on both sides at once.

use serde::{Deserialize, Serialize};

use crate::{MatterId, McpServerView};

mod change;

pub(crate) use change::check_id;
pub use change::{AddonSwitches, MAX_ADDON_ID_BYTES, RemoveAddon, SetAddonOn, SetAddonScope};

/// `ListAddons`' answer: skills first, then tools; within each, built-ins first, then added
/// ones, each group sorted by id.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AddonList {
    /// The rows.
    pub items: Vec<Addon>,
}

/// One skill or tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent flags of a list row (on, enabled, can_remove, locked), each its own wire field"
)]
pub struct Addon {
    /// A skill or a tool.
    pub kind: AddonKind,
    /// Its id: a built-in's fixed name, a skill pack's id, or a tool's server name.
    pub id: String,
    /// Its name, for display.
    pub name: String,
    /// One line, at most 200 characters: a fixed line for a built-in, the first skill's
    /// `SKILL.md` description for an added skill, and empty for an added tool (the app shows
    /// `from` instead).
    pub description: String,
    /// Built in, or added.
    pub source: AddonSource,
    /// Where an added item came from; left out for a built-in, and for an added skill with no
    /// known source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<AddonFrom>,
    /// Where it is on: every matter, or only the matters it is switched on in.
    pub scope: AddonScope,
    /// Whether it is on: in the matter asked about, or without one, in a matter with no switch
    /// for it (scope `all` and on).
    pub on: bool,
    /// The item's own on, apart from scope and switches; a matter's `on` is false whenever this
    /// is false. A pack turned on with its hash, a server `enabled`, `tools.browser` for the
    /// browser and `tools.connectors` for case law; Clauseo's own skills always. Left out (an
    /// older host), it reads as true.
    #[serde(default = "own_on")]
    pub enabled: bool,
    /// Skills only: whether its folder holds a file an agent could run (anything but Markdown,
    /// text and images, or a file marked executable). Left out for a tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_scripts: Option<bool>,
    /// Whether it can be used as it is, and if not, why not in one word.
    pub status: AddonStatus,
    /// Whether the lawyer can remove it: added items the lawyer controls.
    pub can_remove: bool,
    /// Whether the firm decides it: its scope and its removal answer `settings.locked`. A row
    /// that is `locked` with scope `matter` still takes a matter's switch; with scope `all` it
    /// takes none.
    pub locked: bool,
    /// The matters whose switch differs from the scope's default: under `all` those it is off
    /// in, under `matter` those it is on in. Always sent; `[]` when none.
    #[serde(default)]
    pub switched: Vec<MatterId>,
    /// An added tool's server as Settings › Tools shows it: names and `saved` flags, never a
    /// value. Left out for a skill and a built-in tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<McpServerView>,
}

/// A skill, or a tool (an MCP server).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum AddonKind {
    /// A skill.
    Skill,
    /// A tool: an MCP server.
    Tool,
}

/// Whether Clauseo ships the item or someone added it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum AddonSource {
    /// Shipped with Clauseo: it cannot be removed.
    BuiltIn,
    /// Added by the lawyer, the firm or a pack installer.
    Added,
}

/// Where an added item came from. JSON: `{"kind": "url", "text": "https://…"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AddonFrom {
    /// The kind of place.
    pub kind: AddonFromKind,
    /// The place: a skills.sh or GitHub name, a URL, a file or folder name, a program, an
    /// `https` address, or the app imported from.
    pub text: String,
}

/// The kind of place an added item came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum AddonFromKind {
    /// A skill from skills.sh.
    SkillsSh,
    /// A skill from a GitHub repository.
    Github,
    /// A skill from a web address.
    Url,
    /// A skill from a zip file.
    Zip,
    /// A skill from a folder on this computer.
    Folder,
    /// A tool that runs a program on this computer.
    Program,
    /// A tool at an `https` address.
    Address,
    /// A tool imported from another app.
    Import,
}

/// Where an item is on. JSON: `"all"` or `{"matter": "<id>"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum AddonScope {
    /// On in every matter unless switched off in one.
    All,
    /// Off everywhere except the matters it is switched on in, starting with this one.
    Matter(MatterId),
}

/// Whether an item can be used as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum AddonStatus {
    /// It can be used.
    Ready,
    /// A skill's files changed since it was turned on: `SetSkillPack {id, on: true}` turns it
    /// on again.
    Changed,
    /// It cannot be used as it is.
    Invalid,
    /// The firm's policy, or the lawyer's own `skills.extra`, does not allow it.
    Blocked,
    /// A tool's saved key or header value is missing.
    NeedsSecret,
    /// It needs setting up first: case law is offered (or not checked yet) but not connected,
    /// or its last sign-in failed.
    NeedsSetup,
    /// Case law is not set up, and Clauseo had no answer from it (offline, a proxy, a timeout or
    /// a server error). Reading the list again starts a new check, at most every 30 s.
    Unreachable,
    /// Case law's sign-in is under way in the browser.
    SigningIn,
    /// Case law was signed out, or its key was revoked: it needs signing in again.
    SignedOut,
    /// Case law is signed in but out of credit (the figures are on `connectors_changed`).
    NeedsCredit,
    /// Case law answered but is not offered yet (a preview price list), or this install has no
    /// data connectors. Reading the list again checks again, at most every 30 s.
    NotOffered,
}

/// [`Addon::enabled`] when a message leaves it out.
const fn own_on() -> bool {
    true
}
