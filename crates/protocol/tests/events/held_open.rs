//! `PermissionResolved.held_open` (decision record 0015): a matter path when the host refused
//! the request because a program holds that file open, left out otherwise, and an entry
//! written before the field existed decodes with `None`.

use clauseo_protocol::{
    DomainEvent, MatterPath, PermissionChoice, PermissionKind, PermissionResolved, Principal,
};
use serde_json::{Value, json};

use crate::fixtures::{permission_request_id, short};

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
