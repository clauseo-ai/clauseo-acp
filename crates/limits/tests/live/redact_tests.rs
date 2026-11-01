//! The redactor's own tests: what it replaces, hostile samples, and that its output passes the
//! secret check.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::check::check_dir;
use crate::redact::Redactor;
use crate::write;

/// A UUID, built at run time so no source file holds one.
fn uuid() -> String {
    ["0f8fad5b", "d9cb", "469f", "a165", "70867728950e"].join("-")
}

/// A home path under `user`, built at run time.
fn mac_home(user: &str) -> String {
    ["", "Users", user, "logins", "codex"].join("/")
}

#[test]
fn redaction_replaces_emails_ids_uuids_tokens_hosts_and_paths() {
    let token = format!("sk-ant-{}", "x".repeat(30));
    let uuid = uuid();
    let line = serde_json::json!({
        "ok": true,
        "account": {"email": "Real.Person+x@corp.co.uk", "id": "acct_1", "subscriptionType": "max"},
        "organizationUuid": uuid,
        "user_id": 42,
        "note": format!("signed in as a@b.io with {token}"),
        "other": {"id": "kept", "ref": uuid},
        "limitId": "codex",
        "codexHome": mac_home("someone"),
        "params": {"serverName": "Someones-Mac.local", "status": "disabled"},
    })
    .to_string();
    let mut redactor = Redactor::default();
    let redacted: serde_json::Value = serde_json::from_str(&redactor.line(&line)).unwrap();
    assert_eq!(
        redacted,
        serde_json::json!({
            "ok": true,
            "account": {"email": "test@example.com", "id": "redacted-1", "subscriptionType": "max"},
            "organizationUuid": "redacted-2",
            "user_id": "redacted-3",
            "note": "signed in as test@example.com with redacted-token",
            "other": {"id": "kept", "ref": "redacted-2"},
            "limitId": "codex",
            "codexHome": "redacted-path",
            "params": {"serverName": "redacted-host", "status": "disabled"},
        })
    );
}

#[test]
fn hostile_samples_lose_every_email_uuid_path_and_key() {
    let uuid = uuid();
    let home = mac_home("someone");
    let message = format!(
        "failed for José.Pérez@corp.com, o'brien@corp.com and a@bücher.de (id {uuid}) reading {home}/auth, see C:\\Users\\someone\\x"
    );
    let mut by_email = serde_json::Map::new();
    by_email.insert("boss@corp.com".to_owned(), 1.into());
    by_email.insert(uuid, 2.into());
    let line =
        serde_json::json!({"ok": false, "message": message, "byEmail": by_email}).to_string();
    let mut redactor = Redactor::default();
    let redacted: serde_json::Value = serde_json::from_str(&redactor.line(&line)).unwrap();
    assert_eq!(
        redacted,
        serde_json::json!({
            "ok": false,
            "message": "failed for test@example.com, test@example.com and test@example.com (id redacted-1) reading \
                        redacted-path, see redacted-path",
            "byEmail": {"test@example.com": 1, "redacted-1": 2},
        })
    );
}

#[test]
fn the_runs_own_local_values_are_replaced_wherever_they_appear() {
    let login = ["", "srv", "g16", "logins", "claude-max"].join("/");
    let mut redactor = Redactor::new([login.clone(), String::new()]);
    assert_eq!(
        redactor.line(&format!("cannot read {login}/settings: denied")),
        "cannot read redacted-path/settings: denied"
    );
}

#[test]
fn a_line_that_is_not_json_gets_its_emails_and_tokens_replaced() {
    let token = format!("eyJ{}", "a".repeat(24));
    let mut redactor = Redactor::default();
    assert_eq!(
        redactor.line(&format!("x@y.dev said {token}; @scope/pkg and 50@ stay")),
        "test@example.com said redacted-token; @scope/pkg and 50@ stay"
    );
}

#[test]
fn a_redacted_recording_passes_the_secret_check() {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("clauseo-limits-redact-{}-{n}", std::process::id()));
    write::create_dir_all(&dir).unwrap();
    let token = format!("sk-{}", "k".repeat(40));
    let uuid = uuid();
    let home = mac_home("someone");
    let raw = format!(
        r#"{{"account":{{"email":"me@corp.test","accountId":"{token}"}},"x":"{token} {uuid} {home}","José.Pérez@corp.com":1}}"#
    );
    let mut redactor = Redactor::default();
    write::write(&dir.join("one.jsonl"), redactor.line(&raw)).unwrap();
    let report = check_dir(&dir);
    drop(write::remove_dir_all(&dir));
    assert_eq!(report.files, 1);
    assert!(report.findings.is_empty(), "{:?}", report.findings);
}
