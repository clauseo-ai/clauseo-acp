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
//! [`Effective::check_privacy_patch`], [`Effective::apply_privacy_patch`],
//! [`Effective::apply_update_mode`], [`Effective::set_agent_launch`], [`Effective::set_plan`],
//! [`Effective::set_skill_pack`] and [`Effective::plan`]. Every input is a trait:
//! [`PlatformDirs`], [`Env`], [`Files`] and [`PolicySource`], with [`SystemDirs`], [`SystemEnv`]
//! and [`SystemFiles`] over the OS. The
//! parsers of each policy store are in [`policy`]. The webview's `ui` changes are a [`UiPatch`],
//! applied with [`Effective::apply_ui_patch`]: the only write to `settings.json` and the only
//! change to settings after `load`. The `tools` switches are a [`ToolsPatch`], applied with
//! [`Effective::apply_tools_patch`] the same way; a session reads them at its start, so they
//! take effect at the next session start. The `privacy` switches are a [`PrivacyPatch`], applied
//! with [`Effective::apply_privacy_patch`] the same way, and the lawyer's `update.mode` is an
//! [`UpdateChoice`], applied with [`Effective::apply_update_mode`]. The host writes an
//! installed agent's launch entry with [`Effective::set_agent_launch`] and a firm plan with
//! [`Effective::set_plan`] (decision record 0048), never from a webview's choice, and turns a
//! skill pack on or off with [`Effective::set_skill_pack`] (decision record 0040). Everything
//! else takes effect on the next start. [`Locations::update_marker`] and
//! [`Locations::updates_dir`] name the updater's files, so the host joins no paths of its own.
//!
//! # Invariants
//!
//! - [`Effective::load`] never fails. A missing file is normal; a file or value that cannot be
//!   read is a [`Warning`] (with path, line and column where there is one) and defaults stand in.
//!   A policy source that cannot be read is dropped whole and listed in [`PolicyStatus`], and
//!   locks each security key (`mcp.servers`, `tools.web_search`, `tools.browser`,
//!   `privacy.counts`, `privacy.crash_reports`, `update.mode`, `install.bundle`,
//!   `install.registry`, every `plans.<id>`, `skills.extra`, `skills.allow_scripts` and
//!   `skills.curated`) off: the list empty, each switch `false`, the update mode `none`, each
//!   install key unset, no plan, so no endpoint the lawyer chose stands in for the firm's
//!   (decision record 0048), no extra skill pack and none curated (decision record 0040).
//! - Only a policy sets `skills.curated`: a `settings.json` value is ignored with a warning.
//!   A `skills.enabled` entry whose id is no pack id or whose value is not
//!   `sha256:<64 lowercase hex>` is left out with a warning that quotes the id only when it is
//!   id-shaped, else gives its place; no message quotes a hash.
//! - A policy may set `update.mode` to any of `auto`, `notify` and `none`, and locks it. The app
//!   writes only `auto` or `notify` ([`UpdateChoice`], [`Effective::apply_update_mode`]).
//! - A plan variable's name holding `KEY`, `TOKEN`, `SECRET` or `PASSWORD`, in any case, is
//!   refused in `settings.json`, in every policy source and in [`Effective::set_plan`]; a plan's
//!   endpoint and `install.registry` are `https` URLs. No message quotes a plan's or an install
//!   key's value.
//! - [`Effective::set_agent_launch`] writes only Clauseo's own installs (a command or script
//!   under `<data_dir>/agents/`), refuses a locked id, and never replaces an entry that is not
//!   Clauseo's own.
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
mod lock_reason;
mod plans;
pub mod policy;
mod privacy;
mod schema;
mod skills;
mod system;
#[cfg(test)]
mod testing;
mod tools;
mod ui;
mod update;
mod user;
mod warning;

pub use dirs::refused_folders;
pub use effective::{Effective, EntryWrite};
pub use error::Error;
pub use files::{Files, SystemFiles};
pub use leaf::RESERVED_MCP_SERVER;
pub use locations::{
    DATA_DIR_VAR, Env, IDENTIFIER, KnownFolder, Locations, PlatformDirs, SETTINGS_FILE,
};
pub use lock_reason::{LockCause, LockReason, SourceKind};
pub use plans::{PLAN_ENV_NAMES, PLAN_PROXY_NAMES, SECRET_WORDS, secret_shaped};
pub use policy::{
    FilePolicy, PolicyRead, PolicySource, PolicyStatus, PolicyValues, SourceError,
    system_policy_sources,
};
pub use privacy::PrivacyPatch;
pub use schema::{
    AgentLaunch, Density, FontSize, InstallSettings, Language, Lite, MattersSettings,
    McpServerLaunch, McpSettings, Motion, Plan, PlanProvider, PrivacySettings, Settings,
    SkillsExtra, SkillsSettings, Theme, ToolsSettings, UiSettings, UpdateMode, UpdateSettings,
    json_schema,
};
pub use system::{SystemDirs, SystemEnv};
pub use tools::ToolsPatch;
pub use ui::UiPatch;
pub use update::UpdateChoice;
pub use warning::{Origin, Position, Warning, WarningKind};
