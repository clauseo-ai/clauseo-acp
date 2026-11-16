//! A matter's kept "Always" rules: `kept_permissions` and its answer, and
//! `remove_kept_permission` with its shape checks, with the wire names the app reads.

use clauseo_protocol::{
    HostCommand, HostQuery, KeptKind, KeptPermission, MAX_KEPT_RULE_BYTES, QueryResult,
};

use super::fixtures::{matter_id, round_trips};
use super::{Validate, invalid, too_large};

fn remove(rule: &str) -> HostCommand {
    HostCommand::RemoveKeptPermission {
        matter: matter_id(1),
        rule: rule.to_owned(),
    }
}

#[test]
fn kept_permissions_and_its_answer_on_the_wire() {
    let query = HostQuery::KeptPermissions {
        matter: matter_id(1),
    };
    round_trips(&query);
    assert_eq!(
        serde_json::to_string(&query).unwrap(),
        r#"{"kept_permissions":{"matter":"01900000-0000-7000-8000-000000000001"}}"#
    );
    let answer = QueryResult::KeptPermissions(vec![
        KeptPermission {
            rule: "Bash(npm test)".to_owned(),
            kind: KeptKind::Run,
            subject: "npm test".to_owned(),
            server: None,
            cwd: None,
        },
        KeptPermission {
            rule: "mcp__court__find_case".to_owned(),
            kind: KeptKind::Tool,
            subject: "find_case".to_owned(),
            server: Some("court".to_owned()),
            cwd: None,
        },
    ]);
    round_trips(&answer);
    assert_eq!(
        serde_json::to_string(&answer).unwrap(),
        concat!(
            r#"{"kept_permissions":[{"rule":"Bash(npm test)","kind":"run","subject":"npm test"},"#,
            r#"{"rule":"mcp__court__find_case","kind":"tool","subject":"find_case","#,
            r#""server":"court"}]}"#
        )
    );
    for (kind, word) in [
        (KeptKind::Edit, "edit"),
        (KeptKind::Read, "read"),
        (KeptKind::Site, "site"),
    ] {
        assert_eq!(serde_json::to_string(&kind).unwrap(), format!("\"{word}\""));
    }
}

#[test]
fn remove_kept_permission_carries_the_exact_rule() {
    let command = remove("Bash(npm test)");
    round_trips(&command);
    assert_eq!(
        serde_json::to_string(&command).unwrap(),
        concat!(
            r#"{"remove_kept_permission":{"matter":"01900000-0000-7000-8000-000000000001","#,
            r#""rule":"Bash(npm test)"}}"#
        )
    );
    assert!(command.validate().is_ok());
    let longest = "x".repeat(MAX_KEPT_RULE_BYTES);
    assert!(remove(&longest).validate().is_ok());
    let over = "x".repeat(MAX_KEPT_RULE_BYTES + 1);
    assert!(too_large(
        &remove(&over).validate(),
        MAX_KEPT_RULE_BYTES + 1,
        MAX_KEPT_RULE_BYTES
    ));
    for rule in ["", "Bash(a\nb)", "Bash(\u{7})"] {
        assert!(invalid(&remove(rule).validate()), "{rule:?}");
    }
}
