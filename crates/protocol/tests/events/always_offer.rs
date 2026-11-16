//! `PermissionRequested.always`: an `AlwaysOffer` when the host offers "Always for this matter"
//! (`clauseo.always`), left out otherwise; an entry written before the field existed decodes
//! with `None`, and a kind from a newer Clauseo reads as `unknown`.

use clauseo_protocol::{
    AlwaysKind, AlwaysOffer, Deliverable, DomainEvent, PermissionKind, PermissionOption,
    PermissionRequested,
};
use serde_json::{Value, json};

use crate::fixtures::{permission_request_id, round_trips, short};

fn command_offer() -> AlwaysOffer {
    AlwaysOffer {
        kind: AlwaysKind::Command,
        shown: Some("wc -l notes.txt".to_owned()),
        cwd: Some("Drafts".to_owned()),
    }
}

fn requested(always: Option<AlwaysOffer>) -> DomainEvent {
    PermissionRequested {
        request: permission_request_id(10),
        tool_call: None,
        options: vec![PermissionOption {
            option_id: short("allow-once"),
            kind: PermissionKind::AllowOnce,
            agent_label: "Yes".to_owned(),
        }],
        deliverable: Deliverable::Live,
        about: None,
        always,
    }
    .into()
}

/// The JSON of a `permission_requested` entry, with `always` when given.
fn entry(always: Option<Value>) -> Value {
    let mut payload = json!({
        "request": serde_json::to_value(permission_request_id(10)).unwrap(),
        "options": [{"option_id": "allow-once", "kind": "allow_once", "agent_label": "Yes"}],
        "deliverable": "live",
    });
    if let (Some(always), Value::Object(map)) = (always, &mut payload) {
        map.insert("always".to_owned(), always);
    }
    json!({ "permission_requested": payload })
}

#[test]
fn always_is_an_offer_or_left_out_and_round_trips() {
    let set = serde_json::to_value(requested(Some(command_offer()))).unwrap();
    assert_eq!(
        set["permission_requested"]["always"],
        json!({"kind": "command", "shown": "wc -l notes.txt", "cwd": "Drafts"})
    );
    let unset = serde_json::to_value(requested(None)).unwrap();
    assert_eq!(unset["permission_requested"].get("always"), None, "{unset}");
    for value in [set, unset] {
        let back: DomainEvent = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(back).unwrap(), value);
    }
    let file_edit = AlwaysOffer {
        kind: AlwaysKind::FileEdit,
        shown: None,
        cwd: None,
    };
    for always in [Some(command_offer()), Some(file_edit), None] {
        round_trips(&requested(always));
    }
}

#[test]
fn an_entry_without_always_decodes_with_none_and_a_newer_kind_reads_as_unknown() {
    let old: DomainEvent = serde_json::from_value(entry(None)).unwrap();
    assert_eq!(old, requested(None));
    let offer: DomainEvent =
        serde_json::from_value(entry(Some(json!({"kind": "site", "shown": "example.com"}))))
            .unwrap();
    assert_eq!(
        offer,
        requested(Some(AlwaysOffer {
            kind: AlwaysKind::Site,
            shown: Some("example.com".to_owned()),
            cwd: None,
        }))
    );
    let newer: DomainEvent =
        serde_json::from_value(entry(Some(json!({"kind": "zz_later_kind"})))).unwrap();
    assert_eq!(
        newer,
        requested(Some(AlwaysOffer {
            kind: AlwaysKind::Unknown,
            shown: None,
            cwd: None,
        }))
    );
}
