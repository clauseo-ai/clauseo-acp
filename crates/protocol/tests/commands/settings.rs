//! The display settings' wire form: `get_settings`, `set_ui_settings` and its patch, and the
//! values each setting takes. That the values match `clauseo-settings` is checked in
//! `crates/host`, where both crates are visible.

use std::collections::BTreeMap;

use clauseo_protocol::{
    AgentId, CommandEnvelope, HostCommand, HostQuery, Language, Lite, Motion, PlanProvider,
    PlanView, PrivacyPatch, PrivacySettings, QueryResult, SettingsView, Theme, ToolsPatch,
    ToolsSettingsView, UiPatch, UiSettings,
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
        privacy: PrivacySettings {
            counts: true,
            crash_reports: false,
            counts_started: false,
        },
        locked: vec!["ui.theme".to_owned()],
        plans: BTreeMap::new(),
    });
    assert_eq!(
        serde_json::to_string(&answer).unwrap(),
        r#"{"settings":{"ui":{"theme":"system","font_size":16,"language":"en","lite":"automatic","motion":"system"},"tools":{"web_search":true,"browser":false},"privacy":{"counts":true,"crash_reports":false,"counts_started":false},"locked":["ui.theme"]}}"#
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

/// A `set_privacy_settings` command's JSON around `patch`.
fn privacy_patch_command(patch: &str) -> String {
    format!(
        r#"{{"command_id":"{}","command":{{"set_privacy_settings":{{"patch":{patch}}}}}}}"#,
        command_id(1)
    )
}

#[test]
fn a_privacy_patch_writes_only_the_switch_it_sets_and_passes_validate() {
    let command = HostCommand::SetPrivacySettings {
        patch: PrivacyPatch {
            counts: Some(false),
            ..PrivacyPatch::default()
        },
    };
    assert_eq!(
        serde_json::to_string(&command).unwrap(),
        r#"{"set_privacy_settings":{"patch":{"counts":false}}}"#
    );
    round_trips(&command);
    assert!(command.validate().is_ok());
}

#[test]
fn a_privacy_patch_refuses_unknown_fields_and_values() {
    for patch in [
        r#"{"counts":"no"}"#,
        r#"{"crash_reports":1}"#,
        r#"{"counts_started":true}"#,
        r#"{"web_search":false}"#,
        r#"{"privacy":{}}"#,
    ] {
        let json = privacy_patch_command(patch);
        assert!(
            serde_json::from_str::<CommandEnvelope>(&json).is_err(),
            "{patch} decoded"
        );
    }
    let json = privacy_patch_command(r#"{"crash_reports":true}"#);
    assert!(serde_json::from_str::<CommandEnvelope>(&json).is_ok());
}

#[test]
fn first_act_carries_the_switch_and_passes_validate() {
    let command = HostCommand::FirstAct { counts: false };
    assert_eq!(
        serde_json::to_string(&command).unwrap(),
        r#"{"first_act":{"counts":false}}"#
    );
    round_trips(&command);
    assert!(command.validate().is_ok());
    for body in ["{}", r#"{"counts":"yes"}"#, r#"{"counts":true,"id":"x"}"#] {
        let json = format!(
            r#"{{"command_id":"{}","command":{{"first_act":{body}}}}}"#,
            command_id(1)
        );
        assert!(
            serde_json::from_str::<CommandEnvelope>(&json).is_err(),
            "{body} decoded"
        );
    }
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

/// A view with the firm plans: each agent's endpoint, provider and variable names, and no
/// variable's value, which the type has nowhere to hold.
#[test]
fn plans_carry_names_and_never_values() {
    let mut view = SettingsView {
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
        privacy: PrivacySettings {
            counts: true,
            crash_reports: false,
            counts_started: false,
        },
        locked: Vec::new(),
        plans: BTreeMap::new(),
    };
    let without = serde_json::to_string(&view).unwrap();
    assert!(
        !without.contains("plans"),
        "no plans are left out: {without}"
    );
    round_trips(&view);

    let plan = PlanView {
        endpoint: Some("https://gw.firm.example".to_owned()),
        provider: Some(PlanProvider::Anthropic),
        env_names: vec!["ANTHROPIC_CUSTOM_HEADERS_FILE".to_owned()],
    };
    view.plans.insert(AgentId::new("claude-acp").unwrap(), plan);
    view.plans
        .insert(AgentId::new("codex-acp").unwrap(), PlanView::default());
    let json = serde_json::to_string(&view.plans).unwrap();
    assert_eq!(
        json,
        r#"{"claude-acp":{"endpoint":"https://gw.firm.example","provider":"anthropic","env_names":["ANTHROPIC_CUSTOM_HEADERS_FILE"]},"codex-acp":{}}"#
    );
    round_trips(&view);
    // A value, or any field the view does not have, is not part of the form: the keys of a plan
    // are exactly these three.
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let mut keys: Vec<&String> = value["claude-acp"].as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["endpoint", "env_names", "provider"]);
    for provider in PlanProvider::ALL {
        round_trips(&provider);
    }
}
