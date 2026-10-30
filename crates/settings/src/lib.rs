//! `clauseo-settings`: Clauseo's settings, the folders it uses and the policy layer.
//!
//! # Owns
//!
//! The settings schema ([`Settings`] and its sections) and its JSON Schema; where the data
//! folder, `settings.json` and new matters go on each OS ([`Locations`]); the folders a lawyer
//! may not pick as a matter ([`refused_folders`]); reading `settings.json` (JSON with comments)
//! and the one write to it, a [`UiPatch`]; and the policy layer a firm's IT pushes: the Windows
//! registry, macOS managed preferences and a `policy.json`. Decision record
//! `docs/decisions/0009-policy-layer.md` fixes the policy sources, paths, formats and the
//! merge rule.
//!
//! # Interface
//!
//! At start-up, `host` and `cli` call [`Locations::resolve`] (or [`Locations::under_root`] in
//! tests and `cli`), then [`Effective::load`] with [`system_policy_sources`], and keep the
//! [`Effective`] in an `Arc`. It answers [`Effective::settings`], [`Effective::agent`],
//! [`Effective::default_matters_dir`], [`Effective::data_dir`], [`Effective::locations`],
//! [`Effective::policy`], [`Effective::is_locked`] and [`Effective::warnings`]. Every input is a
//! trait: [`PlatformDirs`], [`Env`], [`Files`] and [`PolicySource`], with [`SystemDirs`],
//! [`SystemEnv`] and [`SystemFiles`] over the OS. The parsers of each policy store are in
//! [`policy`]. The webview's settings are a [`UiPatch`], applied with
//! [`Settings::apply_ui_patch`] and saved with [`write_user_settings`].
//!
//! # Invariants
//!
//! - [`Effective::load`] never fails. A missing file is normal; a file or value that cannot be
//!   read is a [`Warning`] (with path, line and column where there is one) and defaults stand in.
//!   A policy source that cannot be read is dropped whole and listed in [`PolicyStatus`].
//! - Policy wins: a policy value replaces the user's for its key, and locks it. An agent's
//!   policy entry replaces the user's whole entry for that id. Earlier sources win over later.
//! - A key this version does not know is a warning, never an error, so a firm's profile for a
//!   newer version still loads.
//! - No variable moves or hides a policy source: [`DATA_DIR_VAR`] moves only the data folder.
//!   Only `HKEY_LOCAL_MACHINE` and forced managed preferences count as policy.
//! - [`UiPatch`] holds only the four `ui` fields and refuses any other key.
//! - An agent's `command` is an absolute path, or the agent is left out with a warning.
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
mod schema;
mod system;
#[cfg(test)]
mod testing;
mod ui;
mod user;
mod warning;

pub use dirs::refused_folders;
pub use effective::Effective;
pub use error::Error;
pub use files::{Files, SystemFiles};
pub use locations::{
    DATA_DIR_VAR, Env, IDENTIFIER, KnownFolder, Locations, PlatformDirs, SETTINGS_FILE,
};
pub use policy::{
    FilePolicy, PolicyRead, PolicySource, PolicyStatus, PolicyValues, SourceError,
    system_policy_sources,
};
pub use schema::{
    AgentLaunch, Density, FontSize, Language, MattersSettings, Settings, Theme, UiSettings,
    json_schema,
};
pub use system::{SystemDirs, SystemEnv};
pub use ui::UiPatch;
pub use user::write_user_settings;
pub use warning::{Origin, Position, Warning, WarningKind};
