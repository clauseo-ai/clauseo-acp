//! `PermissionRequested.always`: an `AlwaysOffer` when the host offers "Always for this matter"
//! (`clauseo.always`), left out otherwise; an entry written before the field existed decodes
//! with `None`, and a kind from a newer Clauseo reads as `unknown`. `PermissionResolved.always`:
//! the kept rule the host answered from, left out when the lawyer answered.

use clauseo_protocol::{
    AlwaysKind, AlwaysOffer, Deliverable, DomainEvent, PermissionChoice, PermissionKind,
    PermissionOption, PermissionRequested, PermissionResolved, Principal,
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

fn resolved(always: Option<AlwaysOffer>) -> DomainEvent {
    PermissionResolved {
        request: permission_request_id(10),
        choice: PermissionChoice::Selected {
            option_id: short("allow-once"),
            kind: PermissionKind::AllowOnce,
        },
        by: Principal::LocalDesktop,
        held_open: None,
        always: always.map(Box::new),
    }
    .into()
}

/// The host's own answer from a kept rule names it; the lawyer's answer leaves it out, and a
/// resolution written before the field existed decodes with `None`.
#[test]
fn a_resolution_names_the_kept_rule_only_when_the_host_answered_from_it() {
    let host = serde_json::to_value(resolved(Some(command_offer()))).unwrap();
    assert_eq!(
        host["permission_resolved"]["always"],
        json!({"kind": "command", "shown": "wc -l notes.txt", "cwd": "Drafts"})
    );
    let lawyer = serde_json::to_value(resolved(None)).unwrap();
    assert_eq!(
        lawyer["permission_resolved"].get("always"),
        None,
        "{lawyer}"
    );
    for always in [Some(command_offer()), None] {
        round_trips(&resolved(always));
    }
    let old = json!({"permission_resolved": {
        "request": serde_json::to_value(permission_request_id(10)).unwrap(),
        "choice": {"selected": {"option_id": "allow-once", "kind": "allow_once"}},
        "by": serde_json::to_value(Principal::LocalDesktop).unwrap(),
    }});
    let old: DomainEvent = serde_json::from_value(old).unwrap();
    assert_eq!(old, resolved(None));
}

/// Each `AlwaysKind` keeps its wire form, and one from a newer Clauseo reads as `unknown`.
#[test]
fn always_kind() {
    let cases = [
        (AlwaysKind::Command, "command"),
        (AlwaysKind::FileEdit, "file_edit"),
        (AlwaysKind::Site, "site"),
        (AlwaysKind::Tool, "tool"),
        (AlwaysKind::Read, "read"),
    ];
    crate::unknown_variants::unit_enum(&AlwaysKind::ALL, &cases, AlwaysKind::Unknown);
}
