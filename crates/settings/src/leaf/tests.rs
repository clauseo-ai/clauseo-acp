//! Typing one value: each fixed key, an agent, unknown keys, a repeated key, `Leaf::key` and
//! `Leaf::apply`, the samples every `KEYS` row needs, and the security keys' snapshot. The
//! `mcp.servers` list is in `mcp_tests.rs`, `refuse` and the `tools` switches in
//! `security_tests.rs`.

use super::{KEYS, Leaf, Raw, SECURITY_KEYS, agent_id, typed};
use crate::policy::kind_of;
use crate::schema::{
    AgentLaunch, Density, FontSize, Language, Lite, McpServerLaunch, Motion, PrivacySettings,
    Settings, Theme, ToolsSettings, UpdateMode,
};
use crate::testing::{abs, abs_str};
use crate::warning::{Origin, Warning, WarningKind};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) fn origin() -> Origin {
    Origin::Store {
        name: "test".to_owned(),
    }
}

/// Types one value: the leaf, or the warnings.
pub(super) fn one(key: &str, value: Value) -> Result<Leaf, Vec<WarningKind>> {
    let mut warnings = Vec::new();
    let raw = Raw {
        key: key.to_owned(),
        value,
        origin: origin(),
    };
    let mut leaves = typed(vec![raw], Vec::new(), &mut warnings);
    let kinds: Vec<WarningKind> = warnings
        .into_iter()
        .inspect(|warning| assert_eq!(warning.origin, origin()))
        .map(|warning| warning.kind)
        .collect();
    match leaves.pop() {
        Some(leaf) if kinds.is_empty() => Ok(leaf),
        _ => Err(kinds),
    }
}

pub(super) fn invalid(key: &str, message: &str) -> Vec<WarningKind> {
    vec![WarningKind::InvalidValue {
        key: key.to_owned(),
        message: message.to_owned(),
    }]
}

#[test]
fn each_ui_key_is_typed() {
    assert_eq!(one("ui.theme", json!("dark")), Ok(Leaf::Theme(Theme::Dark)));
    assert_eq!(
        one("ui.font_size", json!(18)),
        Ok(Leaf::FontSize(FontSize::try_from(18).unwrap()))
    );
    assert_eq!(
        one("ui.density", json!("compact")),
        Ok(Leaf::Density(Density::Compact))
    );
    assert_eq!(
        one("ui.language", json!("hi")),
        Ok(Leaf::Language(Language::Hi))
    );
    assert_eq!(one("ui.lite", json!("on")), Ok(Leaf::Lite(Lite::On)));
    assert_eq!(
        one("ui.motion", json!("reduce")),
        Ok(Leaf::Motion(Motion::Reduce))
    );
}

#[test]
fn a_value_of_the_wrong_type_is_left_out_with_its_key() {
    assert_eq!(
        one("ui.theme", json!("purple")),
        Err(invalid(
            "ui.theme",
            "unknown variant `purple`, expected one of `system`, `light`, `dark`"
        ))
    );
    // 12 and 13 were sizes before the range moved to 14 to 20: a stored one now warns and the
    // key goes back to its default, as any value out of range does.
    assert_eq!(
        one("ui.font_size", json!(12)),
        Err(invalid(
            "ui.font_size",
            "a font size is from 14 to 20, not 12"
        ))
    );
    // A tag the app has no words for, BCP 47 or not, takes the same path.
    assert_eq!(
        one("ui.language", json!("en-GB")),
        Err(invalid(
            "ui.language",
            "unknown variant `en-GB`, expected `en` or `hi`"
        ))
    );
    assert_eq!(
        one("ui.lite", json!("sometimes")),
        Err(invalid(
            "ui.lite",
            "unknown variant `sometimes`, expected one of `automatic`, `on`, `off`"
        ))
    );
    assert_eq!(
        one("ui.motion", json!(true)),
        Err(invalid(
            "ui.motion",
            "invalid type: boolean `true`, expected string or map"
        ))
    );
    assert_eq!(
        one("ui.font_size", json!(30)),
        Err(invalid(
            "ui.font_size",
            "a font size is from 14 to 20, not 30"
        ))
    );
}

#[test]
fn the_default_location_is_absolute_or_null() {
    assert_eq!(
        one("matters.default_location", json!(abs_str("cases"))),
        Ok(Leaf::DefaultLocation(Some(abs("cases"))))
    );
    assert_eq!(
        one("matters.default_location", Value::Null),
        Ok(Leaf::DefaultLocation(None))
    );
    assert_eq!(
        one("matters.default_location", json!("cases")),
        Err(invalid(
            "matters.default_location",
            "cases is not an absolute path"
        ))
    );
}

#[test]
fn an_agent_needs_an_absolute_command_and_warns_on_fields_it_does_not_know() {
    let command = abs_str("bin/agent");
    let leaf = one(
        "agents.claude",
        json!({"command": command, "args": ["--acp"], "env": {"A": "1"}}),
    );
    let launch = AgentLaunch {
        command: command.clone(),
        args: vec!["--acp".to_owned()],
        env: BTreeMap::from([("A".to_owned(), "1".to_owned())]),
    };
    assert_eq!(leaf, Ok(Leaf::Agent("claude".to_owned(), launch.clone())));

    let mut warnings = Vec::new();
    let raw = Raw {
        key: "agents.claude".to_owned(),
        value: json!({"command": command, "args": ["--acp"], "env": {"A": "1"}, "cwd": "/"}),
        origin: origin(),
    };
    let leaves = typed(vec![raw], Vec::new(), &mut warnings);
    assert_eq!(leaves, vec![Leaf::Agent("claude".to_owned(), launch)]);
    assert_eq!(
        warnings,
        vec![Warning {
            origin: origin(),
            kind: WarningKind::UnknownKey {
                key: "agents.claude.cwd".to_owned()
            },
        }]
    );

    assert_eq!(
        one("agents.claude", json!({"command": "agent"})),
        Err(vec![WarningKind::RelativeAgentCommand {
            id: "claude".to_owned(),
            command: "agent".to_owned(),
        }])
    );
    assert_eq!(
        one("agents.claude", json!({"args": []})),
        Err(invalid("agents.claude", "missing field `command`"))
    );
}

#[test]
fn unknown_keys_are_warnings() {
    for key in [
        "ui.colour",
        "agents.",
        "agents",
        "matters",
        "update.channel",
    ] {
        assert_eq!(
            one(key, json!(1)),
            Err(vec![WarningKind::UnknownKey {
                key: key.to_owned()
            }]),
            "{key}"
        );
    }
}

#[test]
fn the_update_mode_is_auto_notify_or_none() {
    for (text, mode) in [
        ("auto", UpdateMode::Auto),
        ("notify", UpdateMode::Notify),
        ("none", UpdateMode::None),
    ] {
        assert_eq!(one("update.mode", json!(text)), Ok(Leaf::UpdateMode(mode)));
    }
    assert_eq!(
        one("update.mode", json!("off")),
        Err(invalid(
            "update.mode",
            "unknown variant `off`, expected one of `auto`, `notify`, `none`"
        ))
    );
}

fn theme_raws(themes: &[&str]) -> Vec<Raw> {
    themes
        .iter()
        .map(|theme| Raw {
            key: "ui.theme".to_owned(),
            value: json!(theme),
            origin: origin(),
        })
        .collect()
}

#[test]
fn a_repeated_key_keeps_its_last_value_valid_or_not() {
    // A valid last value wins over an earlier one, and an invalid one in the middle is only a
    // warning: the last value is what stands.
    let mut warnings = Vec::new();
    let leaves = typed(
        theme_raws(&["dark", "nope", "light"]),
        Vec::new(),
        &mut warnings,
    );
    assert_eq!(leaves, vec![Leaf::Theme(Theme::Light)]);
    assert_eq!(warnings.len(), 1);

    // An invalid last value refuses the key, so it goes back to its default and the earlier
    // valid value does not stand.
    let mut warnings = Vec::new();
    let leaves = typed(theme_raws(&["dark", "purple"]), Vec::new(), &mut warnings);
    assert_eq!(leaves, vec![Leaf::Refused("ui.theme".to_owned())]);
    assert_eq!(warnings.len(), 1);
}

#[test]
fn each_key_comes_once_in_the_order_of_its_last_occurrence() {
    let raw = |key: &str, value: Value| Raw {
        key: key.to_owned(),
        value,
        origin: origin(),
    };
    let raws = vec![
        raw("ui.theme", json!("light")),
        raw("ui.density", json!("compact")),
        raw("ui.theme", json!("dark")),
    ];
    let mut warnings = Vec::new();
    let leaves = typed(raws, Vec::new(), &mut warnings);
    assert_eq!(
        leaves,
        vec![Leaf::Density(Density::Compact), Leaf::Theme(Theme::Dark)]
    );
    assert_eq!(warnings, Vec::new());
}

#[test]
fn key_names_each_leaf_and_apply_sets_it() {
    let command = abs_str("bin/agent");
    let launch = AgentLaunch {
        command,
        args: Vec::new(),
        env: BTreeMap::new(),
    };
    let server = McpServerLaunch {
        command: abs_str("bin/s"),
        args: Vec::new(),
        env: BTreeMap::new(),
        enabled: true,
    };
    let leaves = [
        (
            Leaf::DefaultLocation(Some(abs("m"))),
            "matters.default_location",
        ),
        (Leaf::Agent("x".to_owned(), launch.clone()), "agents.x"),
        (
            Leaf::McpServers(BTreeMap::from([("s".to_owned(), server.clone())])),
            "mcp.servers",
        ),
        (Leaf::Theme(Theme::Light), "ui.theme"),
        (
            Leaf::FontSize(FontSize::try_from(20).unwrap()),
            "ui.font_size",
        ),
        (Leaf::Density(Density::Compact), "ui.density"),
        (Leaf::Language(Language::Hi), "ui.language"),
        (Leaf::Lite(Lite::On), "ui.lite"),
        (Leaf::Motion(Motion::Reduce), "ui.motion"),
        (Leaf::WebSearch(false), "tools.web_search"),
        (Leaf::Browser(true), "tools.browser"),
        (Leaf::UpdateMode(UpdateMode::Notify), "update.mode"),
    ];
    let mut settings = Settings::default();
    for (leaf, key) in leaves {
        assert_eq!(leaf.key(), key);
        leaf.apply(&mut settings);
    }
    assert_eq!(settings.matters.default_location, Some(abs("m")));
    assert_eq!(settings.agents.get("x"), Some(&launch));
    assert_eq!(settings.mcp.servers.get("s"), Some(&server));
    assert_eq!(settings.ui.theme, Theme::Light);
    assert_eq!(settings.ui.font_size.get(), 20);
    assert_eq!(settings.ui.density, Density::Compact);
    assert_eq!(settings.ui.language, Language::Hi);
    assert_eq!(settings.ui.lite, Lite::On);
    assert_eq!(settings.ui.motion, Motion::Reduce);
    assert!(!settings.tools.web_search);
    assert!(settings.tools.browser);
    assert_eq!(settings.update.mode, UpdateMode::Notify);
}

#[test]
fn every_fixed_key_types_and_the_kinds_agree() {
    let samples = [
        json!(abs_str("m")),
        json!({"a": {"command": abs_str("bin/a")}}),
        json!("dark"),
        json!(16),
        json!("compact"),
        json!("en"),
        json!("automatic"),
        json!("system"),
        json!(true),
        json!(false),
        json!(false),
        json!(true),
        json!("notify"),
    ];
    // One sample per row, so a new row cannot go unchecked.
    assert_eq!(samples.len(), KEYS.len());
    for ((key, kind), sample) in KEYS.iter().zip(samples) {
        assert!(one(key, sample).is_ok(), "{key}");
        assert_eq!(kind_of(key), Some(*kind), "{key}");
    }
    assert_eq!(agent_id("agents.x"), Some("x"));
    assert_eq!(agent_id("agents."), None);
    assert_eq!(agent_id("agentsx"), None);
}

#[test]
fn a_refused_key_goes_back_to_its_default() {
    // One value per KEYS row that is not its default, so a row with no `reset` arm leaves its
    // value in force and fails below.
    let samples = [
        json!(abs_str("m")),
        json!({"a": {"command": abs_str("bin/a"), "enabled": true}}),
        json!("dark"),
        json!(20),
        json!("compact"),
        json!("hi"),
        json!("off"),
        json!("full"),
        json!(!ToolsSettings::default().web_search),
        json!(!ToolsSettings::default().browser),
        json!(!PrivacySettings::default().counts),
        json!(!PrivacySettings::default().crash_reports),
        json!("none"),
    ];
    assert_eq!(samples.len(), KEYS.len());
    for ((key, _), sample) in KEYS.iter().zip(samples) {
        let mut settings = Settings::default();
        one(key, sample).unwrap().apply(&mut settings);
        assert_ne!(
            settings,
            Settings::default(),
            "{key}: the sample is the default"
        );
        let refused = Leaf::Refused((*key).to_owned());
        assert_eq!(refused.key(), *key);
        assert!(!refused.is_value());
        refused.apply(&mut settings);
        assert_eq!(
            settings,
            Settings::default(),
            "{key} did not go back to its default"
        );
    }
    // An agent's refused key drops that agent and no other.
    let launch = AgentLaunch {
        command: abs_str("bin/x"),
        args: Vec::new(),
        env: BTreeMap::new(),
    };
    let mut settings = Settings::default();
    settings.agents.insert("x".to_owned(), launch.clone());
    settings.agents.insert("y".to_owned(), launch);
    Leaf::Refused("agents.x".to_owned()).apply(&mut settings);
    assert_eq!(settings.agents.keys().collect::<Vec<_>>(), ["y"]);
}

#[test]
fn the_security_keys() {
    insta::assert_json_snapshot!("security_keys", SECURITY_KEYS);
}
