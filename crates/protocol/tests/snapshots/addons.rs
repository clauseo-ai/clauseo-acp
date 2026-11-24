//! Goldens for the one list of skills and tools (`addons.rs`): `ListAddons` with and without a
//! matter, its answer, and the wire words the app reads. A child of `snapshots.rs`, so its
//! snapshots sit in `snapshots/`. `AddonsChanged` is a row of `host_event`.

use super::*;
use clauseo_protocol::{
    Addon, AddonFrom, AddonFromKind, AddonKind, AddonList, AddonScope, AddonSource, AddonStatus,
    HostQuery, McpSecretName, McpServerKind, McpServerView, QueryResult,
};

/// The contract's three rows, and a built-in skill and a matter-scoped one.
fn addon_list() -> AddonList {
    let row = |kind, id: &str, source, status| Addon {
        kind,
        id: id.to_owned(),
        name: id.to_owned(),
        description: String::new(),
        source,
        from: None,
        scope: AddonScope::All,
        on: true,
        enabled: true,
        has_scripts: None,
        status,
        can_remove: false,
        locked: false,
        switched: Vec::new(),
        server: None,
        icon: None,
    };
    AddonList {
        items: vec![
            Addon {
                description: "Drafting or revising a legal document, or marking up a draft"
                    .to_owned(),
                has_scripts: Some(false),
                ..row(
                    AddonKind::Skill,
                    "legal-drafting",
                    AddonSource::BuiltIn,
                    AddonStatus::Ready,
                )
            },
            Addon {
                name: "PDF".to_owned(),
                description: "Reads, fills and merges PDF files.".to_owned(),
                from: Some(AddonFrom {
                    kind: AddonFromKind::SkillsSh,
                    text: "anthropics/skills/pdf".to_owned(),
                }),
                scope: AddonScope::Matter(matter_id(1)),
                switched: vec![matter_id(1)],
                has_scripts: Some(true),
                can_remove: true,
                ..row(
                    AddonKind::Skill,
                    "pdf",
                    AddonSource::Added,
                    AddonStatus::Changed,
                )
            },
            Addon {
                name: "Case law (Indian Kanoon)".to_owned(),
                description: "Searches Indian judgments and statutes.".to_owned(),
                on: false,
                ..row(
                    AddonKind::Tool,
                    "case-law",
                    AddonSource::BuiltIn,
                    AddonStatus::NeedsSetup,
                )
            },
            Addon {
                from: Some(AddonFrom {
                    kind: AddonFromKind::Address,
                    text: "https://mcp.linear.app/mcp".to_owned(),
                }),
                locked: true,
                switched: vec![matter_id(2)],
                server: Some(linear()),
                ..row(
                    AddonKind::Tool,
                    "linear",
                    AddonSource::Added,
                    AddonStatus::NeedsSecret,
                )
            },
        ],
    }
}

/// The linear server as Settings › Tools shows it: a header's name and flag, no value.
fn linear() -> McpServerView {
    McpServerView {
        name: "linear".to_owned(),
        kind: McpServerKind::Remote,
        command: None,
        args: Vec::new(),
        url: Some("https://mcp.linear.app/mcp".to_owned()),
        env: Vec::new(),
        headers: vec![McpSecretName {
            name: "Authorization".to_owned(),
            saved: false,
        }],
        enabled: true,
    }
}

/// `switched` is always sent, `[]` when empty; `server` only on an added tool's row.
#[test]
fn switched_is_always_sent_and_server_only_on_added_tools() {
    let list = addon_list();
    let rows: Vec<serde_json::Value> = list
        .items
        .iter()
        .map(|row| serde_json::to_value(row).unwrap())
        .collect();
    assert_eq!(rows[0]["switched"], serde_json::json!([]));
    assert_eq!(
        rows[1]["switched"],
        serde_json::json!([matter_id(1).to_string()])
    );
    assert!(rows[0].get("server").is_none() && rows[2].get("server").is_none());
    assert_eq!(rows[3]["server"]["headers"][0]["name"], "Authorization");
    assert!(rows[3]["server"]["headers"][0].get("value").is_none());
    // A row from before `switched` reads with none.
    let mut old = rows[0].clone();
    old.as_object_mut().unwrap().remove("switched");
    let read: Addon = serde_json::from_value(old).unwrap();
    assert_eq!(read.switched, Vec::<clauseo_protocol::MatterId>::new());
}

#[test]
fn host_query_list_addons() {
    let query = HostQuery::ListAddons {
        matter: Some(matter_id(1)),
    };
    snapshot!("host_query_list_addons", &query);
    round_trips(&query);
    assert_eq!(
        serde_json::to_string(&query).unwrap(),
        format!(r#"{{"list_addons":{{"matter":"{}"}}}}"#, matter_id(1))
    );
}

#[test]
fn host_query_list_addons_without_a_matter() {
    let query = HostQuery::ListAddons { matter: None };
    round_trips(&query);
    assert_eq!(
        serde_json::to_string(&query).unwrap(),
        r#"{"list_addons":{}}"#
    );
    let decoded: HostQuery = serde_json::from_str(r#"{"list_addons":{"matter":null}}"#).unwrap();
    assert_eq!(decoded, query);
    assert!(serde_json::from_str::<HostQuery>(r#"{"list_addons":{"x":1}}"#).is_err());
}

#[test]
fn query_result_addons() {
    let answer = QueryResult::Addons(addon_list());
    snapshot!("query_result_addons", &answer);
    round_trips(&answer);
}

/// The words of each closed enum, as the app matches them.
#[test]
fn the_wire_words_are_the_contracts() {
    let words = |value: serde_json::Value| value.to_string();
    let statuses = [
        (AddonStatus::Ready, "ready"),
        (AddonStatus::Changed, "changed"),
        (AddonStatus::Invalid, "invalid"),
        (AddonStatus::Blocked, "blocked"),
        (AddonStatus::NeedsSecret, "needs_secret"),
        (AddonStatus::NeedsSetup, "needs_setup"),
        (AddonStatus::Unreachable, "unreachable"),
        (AddonStatus::SigningIn, "signing_in"),
        (AddonStatus::SignedOut, "signed_out"),
        (AddonStatus::NeedsCredit, "needs_credit"),
        (AddonStatus::NotOffered, "not_offered"),
    ];
    for (status, word) in statuses {
        assert_eq!(
            words(serde_json::to_value(status).unwrap()),
            format!("\"{word}\"")
        );
    }
    let kinds = [
        (AddonFromKind::SkillsSh, "skills_sh"),
        (AddonFromKind::Github, "github"),
        (AddonFromKind::Url, "url"),
        (AddonFromKind::Zip, "zip"),
        (AddonFromKind::Folder, "folder"),
        (AddonFromKind::Program, "program"),
        (AddonFromKind::Address, "address"),
        (AddonFromKind::Import, "import"),
    ];
    for (kind, word) in kinds {
        assert_eq!(
            words(serde_json::to_value(kind).unwrap()),
            format!("\"{word}\"")
        );
    }
    assert_eq!(
        words(serde_json::to_value(AddonScope::All).unwrap()),
        r#""all""#
    );
    assert_eq!(
        words(serde_json::to_value(AddonScope::Matter(matter_id(1))).unwrap()),
        format!(r#"{{"matter":"{}"}}"#, matter_id(1))
    );
    assert_eq!(
        words(serde_json::to_value(AddonSource::BuiltIn).unwrap()),
        r#""built_in""#
    );
    assert_eq!(
        words(serde_json::to_value(AddonKind::Tool).unwrap()),
        r#""tool""#
    );
    assert_eq!(
        serde_json::to_string(&HostEvent::AddonsChanged {}).unwrap(),
        r#"{"addons_changed":{}}"#
    );
}
