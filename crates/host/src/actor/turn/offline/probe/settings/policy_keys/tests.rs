use serde_json::json;

use super::{
    AdminPolicy, Doc, Finding, USER_TIER, admin_docs, finding, plist_doc, sourced_admin_docs,
    user_policy_touches,
};
use crate::agent::loads::pinned::security_pins;

fn doc(value: &serde_json::Value) -> Doc {
    Doc::from_text(&value.to_string())
}

#[test]
fn an_admin_policy_that_turns_hooks_off_is_named_by_its_key() {
    let none = AdminPolicy::of(&[doc(&json!({"model": "opus"})), Doc::Absent]);
    assert_eq!(none.hooks_off, None);
    assert!(none.content);
    let off = AdminPolicy::of(&[doc(&json!({"disableAllHooks": true}))]);
    assert_eq!(off.hooks_off, Some("disableAllHooks"));
    let managed_only =
        AdminPolicy::of(&[Doc::Absent, doc(&json!({"allowManagedHooksOnly": true}))]);
    assert_eq!(managed_only.hooks_off, Some("allowManagedHooksOnly"));
    // `false`, or a string, turns nothing off.
    let on = AdminPolicy::of(&[doc(
        &json!({"disableAllHooks": false, "allowManagedHooksOnly": "true"}),
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
    let empty = AdminPolicy::of(&[Doc::Absent, doc(&json!({}))]);
    assert!(!empty.content);
    let unreadable = AdminPolicy::of(&[Doc::Unreadable]);
    assert_eq!((unreadable.content, unreadable.hooks_off), (true, None));
}

/// What the user tier refuses, for a launch on Windows (no sandbox, out of forced ask), as
/// Claude's per-user policy only ever applies there.
fn user(admin: AdminPolicy, user: &Doc) -> Option<String> {
    let pins = security_pins(false, false);
    user_policy_touches(admin, user, &pins, false).map(|found| found.setting())
}

/// An unreadable or oversized admin document refuses, naming it (fails closed); a weakening
/// setting is named with its key and source; one that tightens passes.
#[test]
fn an_admin_policy_that_weakens_or_cannot_be_read_is_found_with_its_source() {
    let pins = security_pins(true, false);
    let weak = [
        ("managed-settings.json", doc(&json!({"model": "opus"}))),
        (
            "managed-settings.d",
            doc(&json!({"sandbox": {"enabled": false}})),
        ),
    ];
    let found = finding(&weak, &pins, true, false).unwrap();
    assert_eq!(
        (found.setting(), found.source()),
        ("sandbox.enabled".to_owned(), "managed-settings.d")
    );
    let unreadable = [
        ("HKLM", Doc::Absent),
        ("managed-settings.json", Doc::Unreadable),
    ];
    assert_eq!(
        finding(&unreadable, &pins, true, false),
        Some(Finding::Unreadable("managed-settings.json"))
    );
    let tight = [(
        "remote-settings.json",
        doc(&json!({"sandbox": {"enabled": true}})),
    )];
    assert_eq!(finding(&tight, &pins, true, false), None);
}

/// The user tier refuses only a weakening value (the hooks keys included), when it applies; a
/// firm's per-user policy that only tightens passes (no false refusal).
#[test]
fn the_user_tier_refuses_only_a_weakening_value_when_it_applies() {
    let none = AdminPolicy::default();
    for (weakening, key) in [
        (json!({"disableAllHooks": true}), "disableAllHooks"),
        (
            json!({"allowManagedHooksOnly": true}),
            "allowManagedHooksOnly",
        ),
        (
            json!({"allowManagedPermissionRulesOnly": true}),
            "allowManagedPermissionRulesOnly",
        ),
        (
            json!({"permissions": {"defaultMode": "bypassPermissions"}}),
            "permissions.defaultMode",
        ),
        (
            json!({"sandbox": {"enabled": true, "allowUnsandboxedCommands": true}}),
            "sandbox.allowUnsandboxedCommands",
        ),
    ] {
        assert_eq!(
            user(none, &doc(&weakening)).as_deref(),
            Some(key),
            "{weakening}"
        );
    }
    for tightening in [
        json!({"model": "opus", "env": {"X": "1"}}),
        json!({"permissions": {"deny": ["Bash(curl:*)"], "defaultMode": "default"}}),
        json!({"sandbox": {"enabled": false}}),
        json!({"allowManagedHooksOnly": false}),
        json!({"hooks": {}}),
    ] {
        assert_eq!(user(none, &doc(&tightening)), None, "{tightening}");
    }
    assert_eq!(user(none, &Doc::Absent), None);
    assert_eq!(
        user(none, &Doc::Unreadable).as_deref(),
        Some(USER_TIER),
        "unsure is refused"
    );
    // An administrator's policy outranks it: Claude Code does not read it then.
    let admin = AdminPolicy::of(&[doc(&json!({"model": "opus"}))]);
    assert_eq!(user(admin, &doc(&json!({"disableAllHooks": true}))), None);
}

#[test]
fn admin_files_are_read_from_the_managed_folder_with_their_drop_ins() {
    let dir = super::super::tests::folder("policy_keys_admin_files");
    assert!(!AdminPolicy::of(&admin_docs(dir.as_path(), false)).content);
    crate::write::replace_private_file(
        &dir.as_path().join("managed-settings.json"),
        br#"{"model":"opus"}"#,
    )
    .unwrap();
    let drop_ins = dir.as_path().join("managed-settings.d");
    crate::write::create_dir_all(&drop_ins).unwrap();
    crate::write::replace_private_file(
        &drop_ins.join("10-hooks.json"),
        br#"{"disableAllHooks":true}"#,
    )
    .unwrap();
    let policy = AdminPolicy::of(&admin_docs(dir.as_path(), false));
    assert_eq!(policy.hooks_off, Some("disableAllHooks"));
    assert!(policy.content);
    let sources: Vec<&str> = sourced_admin_docs(dir.as_path(), false)
        .into_iter()
        .map(|(source, _)| source)
        .collect();
    assert_eq!(sources, ["managed-settings.json", "managed-settings.d"]);
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
    // A nested dictionary is read as an object, so a weakening sandbox key in it is found.
    let mut sandbox = plist::Dictionary::new();
    sandbox.insert("enabled".into(), plist::Value::Boolean(false));
    let mut nested = plist::Dictionary::new();
    nested.insert("sandbox".into(), plist::Value::Dictionary(sandbox));
    let mut bytes = Vec::new();
    plist::Value::Dictionary(nested)
        .to_writer_xml(&mut bytes)
        .unwrap();
    assert_eq!(
        plist_doc(&bytes),
        doc(&json!({"sandbox": {"enabled": false}}))
    );
    assert_eq!(plist_doc(b""), Doc::Absent);
    assert_eq!(plist_doc(b"not a plist"), Doc::Unreadable);
}

#[test]
fn server_managed_settings_turn_hooks_off_but_leave_the_user_tier_checked() {
    let remote = doc(&json!({"disableAllHooks": true}));
    let policy = AdminPolicy::of(&[Doc::Absent]).with_remote(&remote);
    assert_eq!(policy.hooks_off, Some("disableAllHooks"));
    assert!(policy.remote);
    // The cache sits in a folder the user can write, so it never switches the user tier off.
    assert!(!policy.content);
    assert!(user(policy, &doc(&json!({"disableAllHooks": true}))).is_some());
    let plain = AdminPolicy::default().with_remote(&Doc::Absent);
    assert_eq!(plain, AdminPolicy::default());
}
