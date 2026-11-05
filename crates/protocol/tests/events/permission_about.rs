//! `PermissionRequested.about`: `agent_plan` when the host can tell the request is the agent
//! saving its own plan note, left out otherwise; an entry written before the field existed
//! decodes with `None`, and a value from a newer Clauseo reads as `unknown`.

use clauseo_protocol::{
    Deliverable, DomainEvent, PermissionAbout, PermissionKind, PermissionOption,
    PermissionRequested,
};
use serde_json::{Value, json};

use crate::fixtures::{permission_request_id, round_trips, short};

fn requested(about: Option<PermissionAbout>) -> DomainEvent {
    PermissionRequested {
        request: permission_request_id(10),
        tool_call: None,
        options: vec![PermissionOption {
            option_id: short("allow-once"),
            kind: PermissionKind::AllowOnce,
            agent_label: "Yes".to_owned(),
        }],
        deliverable: Deliverable::Live,
        about,
    }
    .into()
}

/// The JSON of a `permission_requested` entry, with `about` when given.
fn entry(about: Option<&str>) -> Value {
    let mut payload = json!({
        "request": serde_json::to_value(permission_request_id(10)).unwrap(),
        "options": [{"option_id": "allow-once", "kind": "allow_once", "agent_label": "Yes"}],
        "deliverable": "live",
    });
    if let (Some(about), Value::Object(map)) = (about, &mut payload) {
        map.insert("about".to_owned(), json!(about));
    }
    json!({ "permission_requested": payload })
}

#[test]
fn about_is_agent_plan_or_left_out_and_round_trips() {
    let set = serde_json::to_value(requested(Some(PermissionAbout::AgentPlan))).unwrap();
    assert_eq!(set["permission_requested"]["about"], json!("agent_plan"));
    let unset = serde_json::to_value(requested(None)).unwrap();
    assert_eq!(unset["permission_requested"].get("about"), None, "{unset}");
    for value in [set, unset] {
        let back: DomainEvent = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(back).unwrap(), value);
    }
    for about in [Some(PermissionAbout::AgentPlan), None] {
        round_trips(&requested(about));
    }
}

#[test]
fn an_entry_without_about_decodes_with_none_and_a_newer_value_reads_as_unknown() {
    let old: DomainEvent = serde_json::from_value(entry(None)).unwrap();
    assert_eq!(old, requested(None));
    let plan: DomainEvent = serde_json::from_value(entry(Some("agent_plan"))).unwrap();
    assert_eq!(plan, requested(Some(PermissionAbout::AgentPlan)));
    let newer: DomainEvent = serde_json::from_value(entry(Some("zz_later_about"))).unwrap();
    assert_eq!(newer, requested(Some(PermissionAbout::Unknown)));
}
