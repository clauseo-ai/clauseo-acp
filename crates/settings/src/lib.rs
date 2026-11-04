//! `clauseo-settings`: Clauseo's settings, the folders it uses and the policy layer.
//!
//! # Owns
//!
//! The settings schema ([`Settings`] and its sections) and its JSON Schema; where the data
//! folder, `settings.json` and new matters go on each OS ([`Locations`]); the folders a lawyer
//! may not pick as a matter ([`refused_folders`]); reading `settings.json` (JSON with comments)
//! and its runtime writes, [`Effective::apply_ui_patch`] of a [`UiPatch`],
//! [`Effective::apply_tools_patch`] of a [`ToolsPatch`] and [`Effective::apply_privacy_patch`]
//! of a [`PrivacyPatch`]; and the policy
//! layer a firm's IT pushes: the Windows registry, macOS managed preferences and a
//! `policy.json`. Decision record `docs/decisions/0009-policy-layer.md` fixes the policy
//! sources, paths, formats and the merge rule.
//!
//! # Interface
//!
//! At start-up, `host` and `cli` call [`Locations::resolve`] (or [`Locations::under_root`] in
//! tests and `cli`), then [`Effective::load`] with [`system_policy_sources`]. [`Effective`] has
//! one owner. [`Effective::apply_ui_patch`] takes `&mut self`, so the owner holds a
//! `Mutex<Effective>` (or the task that serialises settings writes) for the whole call, and
//! readers go through that owner. It answers [`Effective::settings`], [`Effective::agent`],
//! [`Effective::mcp_servers`], [`Effective::enabled_mcp_servers`],
//! [`Effective::default_matters_dir`], [`Effective::data_dir`], [`Effective::locations`],
//! [`Effective::policy`], [`Effective::policy_present`], [`Effective::is_locked`],
//! [`Effective::warnings`], [`Effective::apply_ui_patch`], [`Effective::apply_tools_patch`],
//! [`Effective::check_privacy_patch`], [`Effective::apply_privacy_patch`] and
//! [`Effective::apply_update_mode`]. Every input is a
//! trait:
//! [`PlatformDirs`], [`Env`], [`Files`] and [`PolicySource`], with [`SystemDirs`], [`SystemEnv`]
//! and [`SystemFiles`] over the OS. The
//! parsers of each policy store are in [`policy`]. The webview's `ui` changes are a [`UiPatch`],
//! applied with [`Effective::apply_ui_patch`]: the only write to `settings.json` and the only
//! change to settings after `load`. The `tools` switches are a [`ToolsPatch`], applied with
//! [`Effective::apply_tools_patch`] the same way; a session reads them at its start, so they
//! take effect at the next session start. The `privacy` switches are a [`PrivacyPatch`], applied
//! with [`Effective::apply_privacy_patch`] the same way, and the lawyer's `update.mode` is an
//! [`UpdateChoice`], applied with [`Effective::apply_update_mode`]. Everything else takes effect
//! on the next start. [`Locations::update_marker`] and [`Locations::updates_dir`] name the
//! updater's files, so the host joins no paths of its own.
//!
//! # Invariants
//!
//! - [`Effective::load`] never fails. A missing file is normal; a file or value that cannot be
//!   read is a [`Warning`] (with path, line and column where there is one) and defaults stand in.
//!   A policy source that cannot be read is dropped whole and listed in [`PolicyStatus`], and
//!   locks each security key (`mcp.servers`, `tools.web_search`, `tools.browser`,
//!   `privacy.counts`, `privacy.crash_reports`, `update.mode`) off: the list empty, each switch
//!   `false`, the update mode `none`.
//! - A policy may set `update.mode` to any of `auto`, `notify` and `none`, and locks it. The app
//!   writes only `auto` or `notify` ([`UpdateChoice`], [`Effective::apply_update_mode`]).
//! - A policy can only turn a `privacy` key off: a policy `true` is ignored with a warning and
//!   locks nothing; a policy `false` locks it (decision record 0036).
//! - Policy wins: a policy value replaces the user's for its key, and locks it. An agent's
//!   policy entry replaces the user's whole entry for that id. Earlier sources win over later.
//! - Policy fails closed per key: a policy value for a known key that does not type is a
//!   warning and still locks the key, at its default (an agent: with no entry), or off for a
//!   security key.
//! - The MCP server name `browser` ([`RESERVED_MCP_SERVER`]) is Clauseo's own: an entry of that
//!   name in `settings.json` or a policy is left out with a warning; the rest of the list loads.
//! - A key that `settings.json` or `policy.json` sets twice keeps its last value, valid or not:
//!   a last value that does not type puts the key back at its default, with a warning.
//! - A key this version does not know is a warning, never an error, so a firm's profile for a
//!   newer version still loads.
//! - No variable moves or hides a policy source: [`DATA_DIR_VAR`] moves the data folder and, with
//!   it, the default matters folder (`<data dir>-matters`, beside it, unless
//!   `matters.default_location` is set); nothing else.
//!   Only `HKEY_LOCAL_MACHINE` and forced managed preferences count as policy.
//! - [`UiPatch`] holds only the `ui` fields and refuses any other key.
//! - A [`UiPatch`] that names a locked key is refused with [`Error::Locked`], and nothing is
//!   written or changed in memory.
//! - [`ToolsPatch`] holds only the `tools` fields and refuses any other key; one that names a
//!   locked tool key is refused whole, as a [`UiPatch`] is. [`PrivacyPatch`] is the same for
//!   the `privacy` fields.
//! - An agent's `command` is an absolute path, or the agent is left out with a warning.
//! - `mcp.servers` is one key: a policy value replaces the user's whole list, with each
//!   server's `enabled`, and an invalid one locks the list empty. An entry with a bad name, a
//!   relative `command` or a value that does not type is left out with a warning; `enabled`
//!   is `false` unless set.
//! - Tests read and write no real folder: every input is injected.
//!
//! # Depends on
//!
//! `serde`, `serde_json`, `serde_json_lenient` (JSON with comments), `schemars`; `windows` on
//! Windows, `objc2-core-foundation` on macOS. No crate of ours.
//!
//! # Start reading
//!
//! `effective.rs` (`Effective::load`), then `leaf.rs` (how a key's value is typed), then
//! `policy.rs`.

mod dirs;
mod effective;
mod error;
mod files;
mod json;
mod leaf;
mod locations;
pub mod policy;
mod privacy;
mod schema;
mod system;
#[cfg(test)]
mod testing;
mod tools;
mod ui;
mod update;
mod user;
mod warning;

pub use dirs::refused_folders;
pub use effective::Effective;
pub use error::Error;
pub use files::{Files, SystemFiles};
pub use leaf::RESERVED_MCP_SERVER;
pub use locations::{
    DATA_DIR_VAR, Env, IDENTIFIER, KnownFolder, Locations, PlatformDirs, SETTINGS_FILE,
};
pub use policy::{
    FilePolicy, PolicyRead, PolicySource, PolicyStatus, PolicyValues, SourceError,
    system_policy_sources,
};
pub use privacy::PrivacyPatch;
pub use schema::{
    AgentLaunch, Density, FontSize, Language, Lite, MattersSettings, McpServerLaunch, McpSettings,
    Motion, PrivacySettings, Settings, Theme, ToolsSettings, UiSettings, UpdateMode,
    UpdateSettings, json_schema,
};
pub use system::{SystemDirs, SystemEnv};
pub use tools::ToolsPatch;
pub use ui::UiPatch;
pub use update::UpdateChoice;
pub use warning::{Origin, Position, Warning, WarningKind};
