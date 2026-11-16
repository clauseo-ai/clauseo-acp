//! `PermissionResolved.held_open` (decision record 0021): a matter path when the host refused
//! the request because a program holds that file open, left out otherwise, and an entry
//! written before the field existed decodes with `None`. And the notice kinds of a step
//! Clauseo's pre-write guard stopped: a paper held open, the matter's `.codex` folder, or a file
//! Clauseo keeps no copy of.

use clauseo_protocol::{
    DomainEvent, MatterPath, NoticeKind, PermissionChoice, PermissionKind, PermissionResolved,
    Principal,
};
use serde_json::{Value, json};

use crate::fixtures::{decodes_in_both, permission_request_id, round_trips, short};

fn refused(held_open: Option<MatterPath>) -> DomainEvent {
    PermissionResolved {
        request: permission_request_id(10),
        choice: PermissionChoice::Selected {
            option_id: short("reject"),
            kind: PermissionKind::RejectOnce,
        },
        by: Principal::LocalDesktop,
        held_open,
    }
    .into()
}

#[test]
fn held_open_is_a_matter_path_or_left_out() {
    let path = MatterPath::new("sub/Brief.docx").unwrap();
    let set = serde_json::to_value(refused(Some(path))).unwrap();
    assert_eq!(
        set["permission_resolved"]["held_open"],
        json!("sub/Brief.docx")
    );
    let unset = serde_json::to_value(refused(None)).unwrap();
    assert_eq!(
        unset["permission_resolved"].get("held_open"),
        None,
        "{unset}"
    );
    for value in [set, unset] {
        let back: DomainEvent = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(back).unwrap(), value);
    }
}

#[test]
fn an_entry_without_held_open_decodes_with_none_and_a_bad_path_is_refused() {
    let entry = |held_open: Option<&str>| {
        let mut payload = json!({
            "request": serde_json::to_value(permission_request_id(10)).unwrap(),
            "choice": {"selected": {"option_id": "reject", "kind": "reject_once"}},
            "by": "local_desktop",
        });
        if let (Some(path), Value::Object(map)) = (held_open, &mut payload) {
            map.insert("held_open".to_owned(), json!(path));
        }
        json!({ "permission_resolved": payload })
    };
    let old: DomainEvent = serde_json::from_value(entry(None)).unwrap();
    assert_eq!(old, refused(None));
    assert!(serde_json::from_value::<DomainEvent>(entry(Some("../x"))).is_err());
}

#[test]
fn the_guards_notice_kinds_decode_and_round_trip() {
    decodes_in_both(r#""codex_folder_blocked""#, &NoticeKind::CodexFolderBlocked);
    let held = NoticeKind::HeldOpenBlocked {
        file: MatterPath::new("Drafts/Lease.docx").unwrap(),
    };
    decodes_in_both(
        r#"{"held_open_blocked":{"file":"Drafts/Lease.docx"}}"#,
        &held,
    );
    round_trips(&held);
    decodes_in_both(r#""guard_step_blocked""#, &NoticeKind::GuardStepBlocked);
    let more = NoticeKind::GuardStepsMore { more: 3 };
    decodes_in_both(r#"{"guard_steps_more":{"more":3}}"#, &more);
    round_trips(&more);
    let no_copy = NoticeKind::NoCopyBlocked {
        file: MatterPath::new("Scan.pdf").unwrap(),
    };
    decodes_in_both(r#"{"no_copy_blocked":{"file":"Scan.pdf"}}"#, &no_copy);
    round_trips(&no_copy);
}
