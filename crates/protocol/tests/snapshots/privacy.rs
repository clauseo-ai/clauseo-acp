//! The settings examples of the golden rows `settings_changed` and `set_privacy_settings` in
//! `snapshots.rs`: the settings view with its privacy switches, and the privacy patch (decision
//! record 0036). They live here so `snapshots.rs` stays under the file-size limit; the rows that
//! use them stay in its `golden!` lists, whose match must name every variant, and their snapshot
//! files keep their names.

use std::collections::BTreeMap;

use clauseo_protocol::{
    Language, Lite, Motion, PrivacyPatch, PrivacySettings, SettingsView, Theme, ToolsSettingsView,
    UiSettings,
};

/// `HostEvent::SettingsChanged`'s example: every section, privacy included, and two locked keys.
pub(crate) fn settings_view() -> SettingsView {
    SettingsView {
        ui: UiSettings {
            theme: Theme::Dark,
            font_size: 18,
            language: Language::Hi,
            lite: Lite::Automatic,
            motion: Motion::Reduce,
        },
        tools: ToolsSettingsView {
            web_search: false,
            browser: true,
        },
        privacy: PrivacySettings {
            counts: true,
            crash_reports: false,
            counts_started: false,
        },
        locked: vec!["ui.font_size".to_owned(), "ui.theme".to_owned()],
        plans: BTreeMap::new(),
    }
}

/// `HostCommand::SetPrivacySettings`' example: counts off, crash reports left as they are.
pub(crate) fn privacy_patch() -> PrivacyPatch {
    PrivacyPatch {
        counts: Some(false),
        crash_reports: None,
    }
}
