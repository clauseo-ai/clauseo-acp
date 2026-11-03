//! The display settings' wire form: `get_settings`, `set_ui_settings` and its patch, and the
//! values each setting takes. That the values match `clauseo-settings` is checked in
//! `crates/host`, where both crates are visible.

use clauseo_protocol::{
    CommandEnvelope, HostCommand, HostQuery, Language, Lite, Motion, QueryResult, SettingsView,
    Theme, ToolsPatch, ToolsSettingsView, UiPatch, UiSettings,
};

use super::Validate;
use super::fixtures::{command_id, round_trips};

#[test]
fn get_settings_is_a_plain_string_and_answers_settings() {
    assert_eq!(
        serde_json::to_string(&HostQuery::GetSettings).unwrap(),
        r#""get_settings""#
    );
    round_trips(&HostQuery::GetSettings);
    let answer = QueryResult::Settings(SettingsView {
        ui: UiSettings {
            theme: Theme::System,
            font_size: 16,
            language: Language::En,
            lite: Lite::Automatic,
            motion: Motion::System,
        },
        tools: ToolsSettingsView {
            web_search: true,
            browser: false,
        },
        locked: vec!["ui.theme".to_owned()],
    });
    assert_eq!(
        serde_json::to_string(&answer).unwrap(),
        r#"{"settings":{"ui":{"theme":"system","font_size":16,"language":"en","lite":"automatic","motion":"system"},"tools":{"web_search":true,"browser":false},"locked":["ui.theme"]}}"#
    );
    round_trips(&answer);
}

#[test]
fn a_patch_writes_only_the_fields_it_sets_and_passes_validate() {
    let command = HostCommand::SetUiSettings {
        patch: UiPatch {
            motion: Some(Motion::Reduce),
            ..UiPatch::default()
        },
    };
    assert_eq!(
        serde_json::to_string(&command).unwrap(),
        r#"{"set_ui_settings":{"patch":{"motion":"reduce"}}}"#
    );
    round_trips(&command);
    // validate() leaves the font size alone: its range is the settings crate's rule.
    let any_size = HostCommand::SetUiSettings {
        patch: UiPatch {
            font_size: Some(99),
            ..UiPatch::default()
        },
    };
    assert!(any_size.validate().is_ok());
}

#[test]
fn a_patch_refuses_unknown_fields_and_values() {
    for patch in [
        r#"{"density":"compact"}"#,
        r#"{"agents":{}}"#,
        r#"{"theme":"purple"}"#,
        r#"{"language":"fr"}"#,
        r#"{"lite":"sometimes"}"#,
        r#"{"motion":"none"}"#,
        r#"{"font_size":300}"#,
    ] {
        let json = patch_command(patch);
        assert!(
            serde_json::from_str::<CommandEnvelope>(&json).is_err(),
            "{patch} decoded"
        );
    }
    // The same command with a known field decodes, so the refusals above are the patch's.
    let json = patch_command(r#"{"font_size":20,"language":"hi"}"#);
    assert!(serde_json::from_str::<CommandEnvelope>(&json).is_ok());
}

/// A `set_ui_settings` command's JSON around `patch`.
fn patch_command(patch: &str) -> String {
    format!(
        r#"{{"command_id":"{}","command":{{"set_ui_settings":{{"patch":{patch}}}}}}}"#,
        command_id(1)
    )
}

/// A `set_tool_settings` command's JSON around `patch`.
fn tool_patch_command(patch: &str) -> String {
    format!(
        r#"{{"command_id":"{}","command":{{"set_tool_settings":{{"patch":{patch}}}}}}}"#,
        command_id(1)
    )
}

#[test]
fn a_tool_patch_writes_only_the_switch_it_sets_and_passes_validate() {
    let command = HostCommand::SetToolSettings {
        patch: ToolsPatch {
            browser: Some(false),
            ..ToolsPatch::default()
        },
    };
    assert_eq!(
        serde_json::to_string(&command).unwrap(),
        r#"{"set_tool_settings":{"patch":{"browser":false}}}"#
    );
    round_trips(&command);
    assert!(command.validate().is_ok());
}

#[test]
fn a_tool_patch_refuses_unknown_fields_and_values() {
    for patch in [
        r#"{"web_search":"no"}"#,
        r#"{"browser":1}"#,
        r#"{"density":"compact"}"#,
        r#"{"tools":{}}"#,
        r#"{"ui":{"theme":"dark"}}"#,
    ] {
        let json = tool_patch_command(patch);
        assert!(
            serde_json::from_str::<CommandEnvelope>(&json).is_err(),
            "{patch} decoded"
        );
    }
    let json = tool_patch_command(r#"{"web_search":false}"#);
    assert!(serde_json::from_str::<CommandEnvelope>(&json).is_ok());
}

/// Each value's JSON string, in order.
fn names<T: serde::Serialize>(values: &[T]) -> Vec<String> {
    values
        .iter()
        .map(|value| {
            serde_json::to_value(value)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

#[test]
fn every_value_has_its_snake_case_name() {
    assert_eq!(names(&Theme::ALL), ["system", "light", "dark"]);
    assert_eq!(names(&Language::ALL), ["en", "hi"]);
    assert_eq!(names(&Lite::ALL), ["automatic", "on", "off"]);
    assert_eq!(names(&Motion::ALL), ["system", "reduce", "full"]);
    assert_eq!(Theme::default(), Theme::System);
    assert_eq!(Language::default(), Language::En);
    assert_eq!(Lite::default(), Lite::Automatic);
    assert_eq!(Motion::default(), Motion::System);
}
