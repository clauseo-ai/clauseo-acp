use super::UiPatch;
use crate::leaf::KEYS;
use crate::schema::{Density, FontSize, Language, Lite, Motion, Settings, Theme};

#[test]
fn patch_rejects_agents_key() {
    let error = serde_json::from_str::<UiPatch>(r#"{"agents": {"x": {"command": "/x"}}}"#)
        .unwrap_err()
        .to_string();
    assert!(error.starts_with("unknown field `agents`"), "{error}");
    for key in ["matters", "default_location", "command"] {
        let text = format!(r#"{{"{key}": "/x"}}"#);
        assert!(serde_json::from_str::<UiPatch>(&text).is_err(), "{key}");
    }
}

#[test]
fn a_patch_takes_every_ui_field() {
    let patch: UiPatch = serde_json::from_str(
        r#"{"theme": "dark", "font_size": 14, "density": "compact", "language": "hi",
            "lite": "on", "motion": "reduce"}"#,
    )
    .unwrap();
    let mut settings = Settings::default();
    settings.apply_ui_patch(&patch);
    assert_eq!(settings.ui.theme, Theme::Dark);
    assert_eq!(settings.ui.font_size, FontSize::try_from(14).unwrap());
    assert_eq!(settings.ui.density, Density::Compact);
    assert_eq!(settings.ui.language, Language::Hi);
    assert_eq!(settings.ui.lite, Lite::On);
    assert_eq!(settings.ui.motion, Motion::Reduce);
}

#[test]
fn an_empty_patch_changes_nothing_and_serialises_empty() {
    let mut settings = Settings::default();
    settings.ui.theme = Theme::Light;
    settings.apply_ui_patch(&UiPatch::default());
    assert_eq!(settings.ui.theme, Theme::Light);
    assert_eq!(serde_json::to_string(&UiPatch::default()).unwrap(), "{}");
}

/// A patch that sets every field, to values that are not the defaults.
fn every_field() -> UiPatch {
    UiPatch {
        theme: Some(Theme::Dark),
        font_size: Some(FontSize::try_from(18).unwrap()),
        density: Some(Density::Compact),
        language: Some(Language::Hi),
        lite: Some(Lite::Off),
        motion: Some(Motion::Full),
    }
}

#[test]
fn the_keys_are_the_names_the_writer_saves() {
    // The writer saves `to_value(patch)` (user.rs); the lock check reads `keys()`. They must agree.
    let saved = serde_json_lenient::to_value(every_field()).unwrap();
    let names: Vec<String> = saved
        .as_object()
        .unwrap()
        .keys()
        .map(|name| format!("ui.{name}"))
        .collect();
    assert_eq!(every_field().keys(), names);
}

#[test]
fn the_keys_a_patch_sets_are_the_ui_policy_keys_in_field_order() {
    let all = every_field();
    let ui_keys: Vec<&str> = KEYS
        .iter()
        .map(|(key, _)| *key)
        .filter(|key| key.starts_with("ui."))
        .collect();
    assert_eq!(all.keys(), ui_keys);
    assert_eq!(
        UiPatch {
            density: Some(Density::Compact),
            ..UiPatch::default()
        }
        .keys(),
        ["ui.density"]
    );
    assert_eq!(UiPatch::default().keys(), Vec::<&str>::new());
}
