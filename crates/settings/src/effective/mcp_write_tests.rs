//! The writes of one MCP server (Settings › Tools): add, replace, rename, remove, on and off,
//! and an import's servers in one write;
//! the firm's lock, each rule's refusal, a failed write, and the file a fresh load agrees with.

use super::Effective;
use crate::error::Error;
use crate::mcp::McpRefusal;
use crate::schema::{KeychainRef, McpServerLaunch, McpTransport, McpValue};
use crate::testing::{MemFiles, abs_json, abs_str, locations, policy, user};
use std::collections::BTreeMap;

fn file_text(files: &MemFiles) -> Option<String> {
    files.text(locations().settings_file())
}

fn saved() -> McpValue {
    McpValue::Keychain(KeychainRef::default())
}

/// A local server at `bin/<name>`, on, with one variable in the keychain.
fn local(name: &str) -> McpServerLaunch {
    McpServerLaunch::local(
        abs_str(&format!("bin/{name}")),
        vec!["--stdio".to_owned()],
        BTreeMap::from([("API_TOKEN".to_owned(), saved())]),
        true,
    )
}

fn remote(url: &str, header: &str) -> McpServerLaunch {
    McpServerLaunch {
        transport: McpTransport::Remote {
            url: url.to_owned(),
            headers: BTreeMap::from([(header.to_owned(), saved())]),
        },
        enabled: false,
    }
}

fn refusal(error: Error) -> McpRefusal {
    match error {
        Error::Mcp { refusal, .. } => refusal,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_saved_server_writes_refs_never_values_and_a_fresh_load_agrees() {
    let files = user(r#"{"ui": {"theme": "dark"}}"#);
    let mut effective = Effective::load(&locations(), &files, &[]);
    effective
        .save_mcp_server(&files, None, "github", &local("github"))
        .unwrap();
    effective
        .save_mcp_server(
            &files,
            None,
            "linear",
            &remote("https://mcp.linear.app/mcp", "Authorization"),
        )
        .unwrap();
    let text = file_text(&files).unwrap();
    assert!(text.contains(r#""API_TOKEN": {"#), "{text}");
    assert!(text.contains(r#""keychain": true"#), "{text}");
    assert!(
        text.contains(r#""theme": "dark""#),
        "the other keys stay: {text}"
    );
    let fresh = Effective::load(&locations(), &files, &[]);
    assert_eq!(fresh.warnings(), []);
    assert_eq!(fresh.mcp_servers(), effective.mcp_servers());
    assert_eq!(fresh.mcp_servers().get("github"), Some(&local("github")));
}

#[test]
fn a_rename_replaces_the_entry_in_one_write_and_keeps_the_others() {
    let files = MemFiles::default();
    let mut effective = Effective::load(&locations(), &files, &[]);
    effective
        .save_mcp_server(&files, None, "gh", &local("gh"))
        .unwrap();
    effective
        .save_mcp_server(&files, None, "other", &local("other"))
        .unwrap();
    effective
        .save_mcp_server(&files, Some("gh"), "github", &local("github"))
        .unwrap();
    assert_eq!(
        effective.mcp_servers().keys().collect::<Vec<_>>(),
        ["github", "other"]
    );
    let fresh = Effective::load(&locations(), &files, &[]);
    assert_eq!(fresh.mcp_servers(), effective.mcp_servers());
    // A rename that only changes the case is the same server's.
    effective
        .save_mcp_server(&files, Some("github"), "GitHub", &local("github"))
        .unwrap();
    assert!(effective.mcp_servers().contains_key("GitHub"));
}

#[test]
fn remove_and_turn_on_or_off() {
    let files = MemFiles::default();
    let mut effective = Effective::load(&locations(), &files, &[]);
    effective
        .save_mcp_server(&files, None, "github", &local("github"))
        .unwrap();
    effective
        .set_mcp_server_enabled(&files, "github", false)
        .unwrap();
    assert_eq!(
        Effective::load(&locations(), &files, &[])
            .enabled_mcp_servers()
            .count(),
        0
    );
    assert_eq!(
        refusal(
            effective
                .set_mcp_server_enabled(&files, "nope", true)
                .unwrap_err()
        ),
        McpRefusal::Unknown
    );
    assert_eq!(
        effective.remove_mcp_server(&files, "github").unwrap(),
        Some(McpServerLaunch {
            enabled: false,
            ..local("github")
        })
    );
    assert_eq!(effective.remove_mcp_server(&files, "github").unwrap(), None);
    assert!(
        Effective::load(&locations(), &files, &[])
            .mcp_servers()
            .is_empty()
    );
}

#[test]
fn each_rule_refuses_with_its_own_refusal_and_writes_nothing() {
    let files = MemFiles::default();
    let mut effective = Effective::load(&locations(), &files, &[]);
    effective
        .save_mcp_server(&files, None, "github", &local("github"))
        .unwrap();
    let before = file_text(&files);
    let relative = McpServerLaunch::local("bin/npx".to_owned(), Vec::new(), BTreeMap::new(), true);
    let bad_env = McpServerLaunch::local(
        abs_str("bin/x"),
        Vec::new(),
        BTreeMap::from([("ProgramFiles(x86)".to_owned(), saved())]),
        true,
    );
    let twice_env = McpServerLaunch::local(
        abs_str("bin/x"),
        Vec::new(),
        BTreeMap::from([("TOKEN".to_owned(), saved()), ("token".to_owned(), saved())]),
        true,
    );
    let cases: [(Option<&str>, &str, McpServerLaunch, McpRefusal); 17] = [
        (None, "", local("x"), McpRefusal::NameEmpty),
        (None, "GitHub", local("x"), McpRefusal::NameTaken),
        (None, "case-law", local("x"), McpRefusal::NameReserved),
        (None, "Browser", local("x"), McpRefusal::NameReserved),
        (None, "ide", local("x"), McpRefusal::NameReserved),
        (
            None,
            "Claude-In-Chrome",
            local("x"),
            McpRefusal::NameReserved,
        ),
        (None, "computer-use", local("x"), McpRefusal::NameReserved),
        (None, "a__b", local("x"), McpRefusal::NameInvalid),
        (None, "a_", local("x"), McpRefusal::NameInvalid),
        (None, "has space", local("x"), McpRefusal::NameInvalid),
        (None, "x", relative, McpRefusal::CommandInvalid),
        (None, "x", bad_env, McpRefusal::EnvNameInvalid),
        (None, "x", twice_env, McpRefusal::EnvNameInvalid),
        (
            None,
            "x",
            remote("http://example.com/mcp", "A"),
            McpRefusal::UrlInvalid,
        ),
        (
            None,
            "x",
            remote("https://example.com", "Bad Header"),
            McpRefusal::HeaderNameInvalid,
        ),
        (Some("nope"), "x", local("x"), McpRefusal::Unknown),
        (
            None,
            "x",
            remote("https://user:pw@example.com", "A"),
            McpRefusal::UrlInvalid,
        ),
    ];
    // A bare name is kept as typed: the host finds it at each save and launch.
    let bare = McpServerLaunch::local("npx".to_owned(), Vec::new(), BTreeMap::new(), true);
    assert!(effective.check_mcp_server(None, "bare", &bare).is_ok());
    for (previous, name, server, expected) in cases {
        let error = effective
            .save_mcp_server(&files, previous, name, &server)
            .unwrap_err();
        assert_eq!(refusal(error), expected, "{name}");
    }
    assert_eq!(file_text(&files), before);
    assert_eq!(effective.mcp_servers().len(), 1);
}

#[test]
fn the_url_rule() {
    for ok in [
        "https://mcp.linear.app/mcp",
        "HTTPS://example.com:8443/a?b=c",
        "http://localhost:3000/mcp",
        "http://127.0.0.1/mcp",
        "https://[::1]:9/",
    ] {
        assert!(crate::mcp::url_ok(ok), "{ok}");
    }
    for bad in [
        "",
        "ftp://example.com",
        "http://example.com",
        "http://127.0.0.2/",
        "http://localhost.evil.com/",
        "https://",
        "https://a@b.com",
        "https://example.com/#x",
        "https://exa mple.com",
        "https://example.com:99999",
        "https://éxample.com",
    ] {
        assert!(!crate::mcp::url_ok(bad), "{bad}");
    }
}

#[test]
fn a_policy_list_locks_every_write() {
    let firm = policy(
        "firm",
        format!(
            r#"{{"mcp": {{"servers": {{"firm": {{"command": "{}"}}}}}}}}"#,
            abs_json("bin/firm")
        ),
    );
    let files = MemFiles::default();
    let mut effective = Effective::load(&locations(), &files, &[&firm]);
    assert!(effective.mcp_locked());
    let locked = |error: Error| matches!(error, Error::Locked { key } if key == "mcp.servers");
    assert!(locked(
        effective
            .save_mcp_server(&files, None, "mine", &local("mine"))
            .unwrap_err()
    ));
    assert!(locked(
        effective.remove_mcp_server(&files, "firm").unwrap_err()
    ));
    assert!(locked(
        effective
            .set_mcp_server_enabled(&files, "firm", true)
            .unwrap_err()
    ));
    assert_eq!(file_text(&files), None);
}

#[test]
fn a_failed_write_leaves_memory_as_it_was() {
    let mut files = MemFiles::default();
    let mut effective = Effective::load(&locations(), &files, &[]);
    effective
        .save_mcp_server(&files, None, "github", &local("github"))
        .unwrap();
    files.fail_writes = true;
    assert!(matches!(
        effective.save_mcp_server(&files, Some("github"), "gh", &local("gh")),
        Err(Error::Io { .. })
    ));
    assert!(matches!(
        effective.remove_mcp_server(&files, "github"),
        Err(Error::Io { .. })
    ));
    assert!(matches!(
        effective.set_mcp_server_enabled(&files, "github", false),
        Err(Error::Io { .. })
    ));
    assert_eq!(
        effective.mcp_servers(),
        &BTreeMap::from([("github".to_owned(), local("github"))])
    );
}

/// The file's `mcp.servers.<name>` entry, as JSON.
fn entry(files: &MemFiles, name: &str) -> serde_json::Value {
    let text = file_text(files).unwrap();
    let file: serde_json::Value = serde_json::from_str(&text).unwrap();
    file["mcp"]["servers"][name].clone()
}

/// Turning a server on changes only its `enabled` in the file: a stray `url` beside its
/// `command`, a key this version does not read and a plain variable stay as they were. Saving
/// another server leaves this entry as it was too, and neither write moves another key.
#[test]
fn writes_leave_every_other_key_as_the_file_has_it() {
    let odd = serde_json::json!({
        "command": abs_str("bin/odd"),
        "url": "https://stray.example/mcp",
        "future_key": {"kept": [1, 2]},
        "env": {"PLAIN": "value"},
    });
    let text = serde_json::json!({
        "ui": {"theme": "dark"},
        "mcp": {"servers": {"odd": &odd}, "future_section_key": true},
    })
    .to_string();
    let files = user(&text);
    let mut effective = Effective::load(&locations(), &files, &[]);
    assert!(effective.mcp_servers().contains_key("odd"));
    effective
        .set_mcp_server_enabled(&files, "odd", true)
        .unwrap();
    let mut on = odd;
    on["enabled"] = serde_json::json!(true);
    assert_eq!(entry(&files, "odd"), on);
    effective
        .save_mcp_server(&files, None, "github", &local("github"))
        .unwrap();
    assert_eq!(entry(&files, "odd"), on);
    let file: serde_json::Value = serde_json::from_str(&file_text(&files).unwrap()).unwrap();
    assert_eq!(file["ui"], serde_json::json!({"theme": "dark"}));
    assert_eq!(file["mcp"]["future_section_key"], serde_json::json!(true));
    // A minimal entry the lawyer saves is written minimal: no empty lists.
    let bare = McpServerLaunch::local(abs_str("bin/bare"), Vec::new(), BTreeMap::new(), false);
    effective
        .save_mcp_server(&files, None, "bare", &bare)
        .unwrap();
    assert_eq!(
        entry(&files, "bare"),
        serde_json::json!({"command": abs_str("bin/bare"), "enabled": false})
    );
}

#[test]
fn an_import_adds_all_in_one_write_or_none() {
    let mut files = user(r#"{"ui": {"theme": "dark"}}"#);
    let mut effective = Effective::load(&locations(), &files, &[]);
    effective
        .save_mcp_server(&files, None, "github", &local("github"))
        .unwrap();
    let before = file_text(&files);
    // A name held in another case, or twice in the batch, refuses the whole batch.
    for batch in [
        vec![
            ("a".to_owned(), local("a")),
            ("GitHub".to_owned(), local("b")),
        ],
        vec![("a".to_owned(), local("a")), ("A".to_owned(), local("b"))],
    ] {
        let error = effective.add_mcp_servers(&files, &batch).unwrap_err();
        assert_eq!(refusal(error), McpRefusal::NameTaken);
        assert_eq!(file_text(&files), before);
    }
    files.fail_writes = true;
    let batch = [("a".to_owned(), local("a")), ("b".to_owned(), local("b"))];
    assert!(matches!(
        effective.add_mcp_servers(&files, &batch),
        Err(Error::Io { .. })
    ));
    assert_eq!(effective.mcp_servers().len(), 1, "a failed write adds none");
    files.fail_writes = false;
    effective.add_mcp_servers(&files, &batch).unwrap();
    let fresh = Effective::load(&locations(), &files, &[]);
    let names: Vec<&str> = fresh.mcp_servers().keys().map(String::as_str).collect();
    assert_eq!(names, ["a", "b", "github"]);
    assert_eq!(fresh.mcp_servers(), effective.mcp_servers());
}
