use serde_json::json;

use super::{AdminPolicy, Doc, admin_docs, plist_doc, user_policy_touches};

fn doc(value: serde_json::Value) -> Doc {
    Doc::from_text(&value.to_string())
}

#[test]
fn an_admin_policy_that_turns_hooks_off_is_named_by_its_key() {
    let none = AdminPolicy::of(&[doc(json!({"model": "opus"})), Doc::Absent]);
    assert_eq!(none.hooks_off, None);
    assert!(none.content);
    let off = AdminPolicy::of(&[doc(json!({"disableAllHooks": true}))]);
    assert_eq!(off.hooks_off, Some("disableAllHooks"));
    let managed_only = AdminPolicy::of(&[Doc::Absent, doc(json!({"allowManagedHooksOnly": true}))]);
    assert_eq!(managed_only.hooks_off, Some("allowManagedHooksOnly"));
    // `false`, or a string, turns nothing off.
    let on = AdminPolicy::of(&[doc(
        json!({"disableAllHooks": false, "allowManagedHooksOnly": "true"}),
    )]);
    assert_eq!(on.hooks_off, None);
}

#[test]
fn absent_empty_and_unreadable_documents() {
    assert_eq!(Doc::from_text(""), Doc::Absent);
    assert_eq!(Doc::from_text("  \u{feff}\n"), Doc::Absent);
    assert_eq!(Doc::from_text("{"), Doc::Unreadable);
    assert_eq!(Doc::from_read(&Err(())), Doc::Unreadable);
    assert_eq!(Doc::from_read(&Ok(None)), Doc::Absent);
    let empty = AdminPolicy::of(&[Doc::Absent, doc(json!({}))]);
    assert!(!empty.content);
    let unreadable = AdminPolicy::of(&[Doc::Unreadable]);
    assert_eq!(
        (
            unreadable.content,
            unreadable.unreadable,
            unreadable.hooks_off
        ),
        (true, true, None)
    );
}

#[test]
fn the_user_tier_refuses_only_when_it_applies_and_touches_a_guarded_area() {
    let none = AdminPolicy::default();
    for touching in [
        json!({"permissions": {"allow": ["Bash"]}}),
        json!({"sandbox": {"enabled": false}}),
        json!({"hooks": {}}),
        json!({"disableAllHooks": true}),
        json!({"allowManagedHooksOnly": false}),
        json!({"allowManagedPermissionRulesOnly": true}),
    ] {
        assert!(
            user_policy_touches(none, &doc(touching.clone())),
            "{touching}"
        );
    }
    assert!(!user_policy_touches(
        none,
        &doc(json!({"model": "opus", "env": {"X": "1"}}))
    ));
    assert!(!user_policy_touches(none, &Doc::Absent));
    assert!(
        user_policy_touches(none, &Doc::Unreadable),
        "unsure is refused"
    );
    // An administrator's policy outranks it: Claude Code does not read it then.
    let admin = AdminPolicy::of(&[doc(json!({"model": "opus"}))]);
    assert!(!user_policy_touches(
        admin,
        &doc(json!({"sandbox": {"enabled": false}}))
    ));
}

#[test]
fn admin_files_are_read_from_the_managed_folder_with_their_drop_ins() {
    let dir = super::super::tests::folder("policy_keys_admin_files");
    assert!(!AdminPolicy::of(&admin_docs(dir.as_path(), false)).content);
    std::fs::write(
        dir.as_path().join("managed-settings.json"),
        r#"{"model":"opus"}"#,
    )
    .unwrap();
    let drop_ins = dir.as_path().join("managed-settings.d");
    std::fs::create_dir(&drop_ins).unwrap();
    std::fs::write(
        drop_ins.join("10-hooks.json"),
        r#"{"disableAllHooks":true}"#,
    )
    .unwrap();
    let policy = AdminPolicy::of(&admin_docs(dir.as_path(), false));
    assert_eq!(policy.hooks_off, Some("disableAllHooks"));
    assert!(policy.content);
    crate::write::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_managed_preferences_list_turning_hooks_off_is_read() {
    let mut settings = plist::Dictionary::new();
    settings.insert("allowManagedHooksOnly".into(), plist::Value::Boolean(true));
    let mut bytes = Vec::new();
    plist::Value::Dictionary(settings)
        .to_writer_xml(&mut bytes)
        .unwrap();
    let policy = AdminPolicy::of(&[plist_doc(&bytes)]);
    assert_eq!(policy.hooks_off, Some("allowManagedHooksOnly"));
    assert_eq!(plist_doc(b""), Doc::Absent);
    assert_eq!(plist_doc(b"not a plist"), Doc::Unreadable);
}

#[test]
fn server_managed_settings_turn_hooks_off_but_leave_the_user_tier_checked() {
    let remote = doc(json!({"disableAllHooks": true}));
    let policy = AdminPolicy::of(&[Doc::Absent]).with_remote(&remote);
    assert_eq!(policy.hooks_off, Some("disableAllHooks"));
    assert!(policy.remote);
    // The cache sits in a folder the user can write, so it never switches the user tier off.
    assert!(!policy.content);
    assert!(user_policy_touches(
        policy,
        &doc(json!({"sandbox": {"enabled": false}}))
    ));
    let plain = AdminPolicy::default().with_remote(&Doc::Absent);
    assert_eq!(plain, AdminPolicy::default());
}
