//! What a refused key is set to: [`refuse`] locks a security key at its safe value, and
//! [`reset`] puts any key back at its default.

use super::{SECURITY_KEYS, agent_id};
use crate::plans::plan_id;
use crate::schema::{
    Density, FontSize, Language, Lite, Motion, PrivacySettings, Settings, SkillsExtra, Theme,
    ToolsSettings, UpdateMode,
};

/// Locks each security key `key` names at its refused value: `mcp.servers` empty, each
/// `tools` and `privacy` key `false`, `update.mode` `none`, each `install` key unset, no
/// plan at all (`plans.`), `skills.extra` `off`, `skills.allow_scripts` `false` and
/// `skills.curated` empty. Kept apart from [`reset`], which restores the default, since a
/// security key's default can be on. `key` is one of [`SECURITY_KEYS`], or a prefix ending in
/// `.` (`tools.`, `install.`) that names every security key under it. A single `plans.<id>`
/// names none: [`reset`] drops that one plan.
/// Returns whether `key` named any security key; when it did not, nothing changes.
pub(crate) fn refuse(key: &str, settings: &mut Settings) -> bool {
    let named = |security: &str| match key.strip_suffix('.') {
        Some(_) => security.starts_with(key),
        None => security == key,
    };
    let mut any = false;
    for security in SECURITY_KEYS.into_iter().filter(|security| named(security)) {
        any = true;
        match security {
            "mcp.servers" => settings.mcp.servers.clear(),
            "tools.web_search" => settings.tools.web_search = false,
            "tools.browser" => settings.tools.browser = false,
            "tools.connectors" => settings.tools.connectors = false,
            "privacy.counts" => settings.privacy.counts = false,
            "privacy.crash_reports" => settings.privacy.crash_reports = false,
            "update.mode" => settings.update.mode = UpdateMode::None,
            "install.bundle" => settings.install.bundle = None,
            "install.registry" => settings.install.registry = None,
            "plans." => settings.plans.clear(),
            "skills.extra" => settings.skills.extra = SkillsExtra::Off,
            "skills.allow_scripts" => settings.skills.allow_scripts = false,
            "skills.curated" => settings.skills.curated.clear(),
            other => unreachable!("security key {other} has no refused value"),
        }
    }
    any
}

/// Puts the setting `key` back to its default; for `agents.<id>` or `plans.<id>`, drops that
/// entry.
pub(super) fn reset(key: &str, settings: &mut Settings) {
    match key {
        "matters.default_location" => settings.matters.default_location = None,
        "mcp.servers" => settings.mcp.servers.clear(),
        "ui.theme" => settings.ui.theme = Theme::default(),
        "ui.font_size" => settings.ui.font_size = FontSize::default(),
        "ui.density" => settings.ui.density = Density::default(),
        "ui.language" => settings.ui.language = Language::default(),
        "ui.lite" => settings.ui.lite = Lite::default(),
        "ui.motion" => settings.ui.motion = Motion::default(),
        "tools.web_search" => settings.tools.web_search = ToolsSettings::default().web_search,
        "tools.browser" => settings.tools.browser = ToolsSettings::default().browser,
        "tools.connectors" => settings.tools.connectors = ToolsSettings::default().connectors,
        "privacy.counts" => settings.privacy.counts = PrivacySettings::default().counts,
        "privacy.crash_reports" => {
            settings.privacy.crash_reports = PrivacySettings::default().crash_reports;
        }
        "update.mode" => settings.update.mode = UpdateMode::default(),
        "install.bundle" => settings.install.bundle = None,
        "install.registry" => settings.install.registry = None,
        "skills.extra" => settings.skills.extra = SkillsExtra::default(),
        "skills.curated" => settings.skills.curated.clear(),
        "skills.allow_scripts" => settings.skills.allow_scripts = false,
        "skills.enabled" => settings.skills.enabled.clear(),
        _ => {
            if let Some(id) = agent_id(key) {
                settings.agents.remove(id);
            } else if let Some(id) = plan_id(key) {
                settings.plans.remove(id);
            }
        }
    }
}
