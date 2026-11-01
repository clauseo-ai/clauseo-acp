//! The redactor's own tests: what it replaces, and that its output passes the secret check.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::check::check_dir;
use crate::redact::{Redactor, text};
use crate::write;

#[test]
fn redaction_replaces_emails_ids_uuids_and_tokens() {
    // Built at run time, so no source file holds a token shape.
    let token = format!("sk-ant-{}", "x".repeat(30));
    let uuid = "0f8fad5b-d9cb-469f-a165-70867728950e";
    let line = serde_json::json!({
        "ok": true,
        "account": {"email": "Real.Person+x@corp.co.uk", "id": "acct_1", "subscriptionType": "max"},
        "organizationUuid": uuid,
        "user_id": 42,
        "note": format!("signed in as a@b.io with {token}"),
        "other": {"id": "kept", "ref": uuid},
        "limitId": "codex",
        "codexHome": "/Users/someone/logins/codex",
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
fn a_line_that_is_not_json_gets_its_emails_and_tokens_replaced() {
    let token = format!("eyJ{}", "a".repeat(24));
    assert_eq!(
        text(&format!("x@y.dev said {token}; @scope/pkg and 50@ stay")),
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
    let raw =
        format!(r#"{{"account":{{"email":"me@corp.test","accountId":"{token}"}},"x":"{token}"}}"#);
    let mut redactor = Redactor::default();
    write::write(&dir.join("one.jsonl"), redactor.line(&raw)).unwrap();
    let report = check_dir(&dir);
    drop(write::remove_dir_all(&dir));
    assert_eq!(report.files, 1);
    assert!(report.findings.is_empty(), "{:?}", report.findings);
}
