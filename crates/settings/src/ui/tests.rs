use super::UiPatch;
use crate::schema::{Density, FontSize, Language, Settings, Theme};

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
fn a_patch_takes_the_four_ui_fields() {
    let patch: UiPatch = serde_json::from_str(
        r#"{"theme": "dark", "font_size": 12, "density": "compact", "language": "de-AT"}"#,
    )
    .unwrap();
    let mut settings = Settings::default();
    settings.apply_ui_patch(&patch);
    assert_eq!(settings.ui.theme, Theme::Dark);
    assert_eq!(settings.ui.font_size, FontSize::try_from(12).unwrap());
    assert_eq!(settings.ui.density, Density::Compact);
    assert_eq!(
        settings.ui.language,
        Language::try_from("de-AT".to_owned()).unwrap()
    );
}

#[test]
fn an_empty_patch_changes_nothing_and_serialises_empty() {
    let mut settings = Settings::default();
    settings.ui.theme = Theme::Light;
    settings.apply_ui_patch(&UiPatch::default());
    assert_eq!(settings.ui.theme, Theme::Light);
    assert_eq!(serde_json::to_string(&UiPatch::default()).unwrap(), "{}");
}
