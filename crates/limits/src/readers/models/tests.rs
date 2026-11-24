//! The probe answers mapped to `AgentModels`.

use clauseo_codex_appserver::ModelListResponse;
use serde_json::json;

use super::{claude, codex};

/// A `session/options` answer as claude-agent-acp 0.110.1 gives an AIR client.
fn claude_answer() -> serde_json::Value {
    let air = |values: serde_json::Value| json!({"jetbrains": {"air": {"reasoningEfforts": {"values": values, "recommended": "medium"}}}});
    json!({"configOptions": [
        {"id": "model", "name": "Model", "category": "model", "type": "select",
         "currentValue": "opus", "options": [
            {"value": "opus", "name": "Opus 5.5", "description": "Most capable",
             "_meta": air(json!(["low", "medium", "high", "x_high", "max"]))},
            {"value": "fable", "name": "Fable 5.2", "description": "",
             "_meta": air(json!(["low", "medium", "high"]))},
            {"value": "haiku", "name": "Haiku 5.5"}]},
        {"id": "effort", "name": "Effort", "category": "thought_level", "type": "select",
         "currentValue": "medium", "options": [
            {"value": "low", "name": "Low"}, {"value": "medium", "name": "Medium"},
            {"value": "high", "name": "High"}, {"value": "x_high", "name": "Xhigh"},
            {"value": "max", "name": "Max"}]},
        {"id": "fast", "name": "Fast", "category": "other", "type": "boolean", "currentValue": false}
    ]})
}

#[test]
fn claude_options_give_every_model_its_efforts_and_the_defaults() {
    let models = claude::models(&claude_answer()).unwrap();
    assert_eq!(models.model_option, "model");
    assert_eq!(models.effort_option.as_deref(), Some("effort"));
    assert_eq!(models.current_model, "opus");
    assert_eq!(models.current_effort.as_deref(), Some("medium"));
    let names: Vec<&str> = models.models.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Opus 5.5", "Fable 5.2", "Haiku 5.5"]);
    let opus = models.model("opus").unwrap();
    assert_eq!(opus.efforts.len(), 5);
    assert_eq!(
        opus.efforts[3].name, "Xhigh",
        "the effort option's own name"
    );
    assert_eq!(opus.default_effort.as_deref(), Some("medium"));
    assert_eq!(opus.description.as_deref(), Some("Most capable"));
    let fable = models.model("fable").unwrap();
    assert_eq!(fable.efforts.len(), 3);
    assert_eq!(fable.description, None, "an empty description is none");
    assert_eq!(models.model("haiku").unwrap().efforts, []);
}

#[test]
fn claude_options_without_a_model_option_give_none() {
    assert_eq!(claude::models(&json!({"configOptions": []})), None);
    assert_eq!(claude::models(&json!({})), None);
}

#[test]
fn codex_models_are_named_as_codex_acp_names_them() {
    let page: ModelListResponse = serde_json::from_value(json!({"data": [
        {"id": "gpt-6-luna", "model": "gpt-6-luna", "displayName": "gpt-6-luna",
         "description": "Small", "hidden": false, "isDefault": false,
         "supportedReasoningEfforts": [{"reasoningEffort": "low", "description": "Fast"}],
         "defaultReasoningEffort": "low"},
        {"id": "gpt-6.2-sol", "model": "gpt-6.2-sol", "displayName": "gpt-6.2-sol",
         "description": "Frontier", "hidden": false, "isDefault": true,
         "supportedReasoningEfforts": [
            {"reasoningEffort": "low", "description": "Fast"},
            {"reasoningEffort": "medium", "description": "Balanced"},
            {"reasoningEffort": "xhigh", "description": "Deepest"}],
         "defaultReasoningEffort": "medium"},
        {"id": "secret", "model": "secret", "displayName": "secret", "description": "",
         "hidden": true, "isDefault": false, "supportedReasoningEfforts": [],
         "defaultReasoningEffort": "low"}
    ]}))
    .unwrap();
    let models = codex::models(&page.data).unwrap();
    assert_eq!(models.model_option, "model");
    assert_eq!(models.effort_option.as_deref(), Some("reasoning_effort"));
    assert_eq!(models.current_model, "gpt-6.2-sol");
    assert_eq!(models.current_effort.as_deref(), Some("medium"));
    let names: Vec<&str> = models.models.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["6 Luna", "6.2 Sol"], "hidden models are left out");
    let sol = models.model("gpt-6.2-sol").unwrap();
    let efforts: Vec<&str> = sol.efforts.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(efforts, ["Low", "Medium", "Xhigh"]);
}

#[test]
fn an_empty_model_list_gives_none() {
    assert_eq!(codex::models(&[]), None);
}
