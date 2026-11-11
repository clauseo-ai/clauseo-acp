//! What an import never does: replace a server whose name the list holds, keep a value after a
//! settings write fails, or put a value or a path in the view. Split from `mcp_import.rs` for
//! its length.

use clauseo_connectors::SecretStore as _;

use super::*;

/// `view` as JSON, with each server's program left out (the list shows the stored program, as
/// A1's does), holds no value, no folder of the fake home, and no config file's name.
pub(super) fn assert_no_values_or_paths(view: &McpToolsView) {
    let mut json = serde_json::to_value(view).unwrap();
    if let Some(servers) = json["servers"].as_array_mut() {
        for server in servers {
            if let Some(server) = server.as_object_mut() {
                server.remove("command");
            }
        }
    }
    let text = json.to_string();
    let home = serde_json::to_string(home()).unwrap();
    let home = home.trim_matches('"');
    for needle in [
        ENV_VALUE,
        HEADER_VALUE,
        home,
        "Matters",
        ".mcp.json",
        "claude_desktop_config",
        "config.toml",
        ".claude.json",
        "tools/uvx-missing",
    ] {
        assert!(!text.contains(needle), "the view holds {needle:?}: {text}");
    }
}

/// A server whose name the list holds in another case is skipped (`already_added`), never
/// renamed or replaced: its entry stays as it was and the import writes no keychain item for it.
#[test]
fn mcp_import_never_replaces_or_renames_a_held_name() {
    let dir = Dir::new("mcp_import_collision", &[]);
    let mine = json!({"command": program(), "args": ["mine"], "enabled": true});
    dir.set_settings(&[], json!({"mcp": {"servers": {"GitHub": mine}}}));
    let store = Arc::new(MemoryStore::new());
    let host = dir.start_with(0, |config| wired(config, &store, apps()));
    let mut sub = subscribe(&host);
    let (view, last) = imported(&host, &mut sub, ClaudeDesktop);
    assert_eq!(last.imported, ["notes"]);
    assert!(
        last.skipped
            .contains(&skip("github", McpSkipReason::AlreadyAdded, None)),
        "{last:?}"
    );
    let names: Vec<&str> = view.servers.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["GitHub", "notes"]);
    let file: Value = serde_json::from_str(&dir.settings_json()).unwrap();
    assert_eq!(file["mcp"]["servers"]["GitHub"], mine);
    assert_eq!(file["mcp"]["servers"].get("github"), None);
    assert_eq!(store.peek("github/env/GITHUB_TOKEN"), None);
    assert_eq!(store.peek("GitHub/env/GITHUB_TOKEN"), None);
    host.shutdown();
}

/// A settings write that fails after the keychain writes saves no server and puts back every
/// value: the item it added is gone, the one it replaced holds its old value again.
#[test]
fn mcp_import_settings_write_failure_rolls_the_keychain_back() {
    let dir = Dir::new("mcp_import_write_fails", &[]);
    let store = Arc::new(MemoryStore::new());
    store
        .set("notes/header/Authorization", b"left-from-before")
        .unwrap();
    let host = dir.start_with(0, |config| wired(config, &store, apps()));
    let mut sub = subscribe(&host);
    let _ = view_until(&mut sub, |view| !view.importable.is_empty());
    // A file the settings cannot read back fails the write, before anything is written.
    let broken = "{ this is not json";
    write::write(&dir.data().join("settings.json"), broken.as_bytes());
    let error = host.dispatch(import(ClaudeDesktop)).unwrap_err();
    assert_ne!(error.code(), ErrorCode::McpSecretStoreFailed, "{error}");
    assert_eq!(store.peek("github/env/GITHUB_TOKEN"), None);
    assert_eq!(
        store.peek("notes/header/Authorization").as_deref(),
        Some(b"left-from-before".as_slice())
    );
    assert_eq!(dir.settings_json(), broken);
    // The put-back ran before the answer: a later command sees the same keychain.
    host.ok(HostCommand::Mcp(McpCommand::RemoveServer {
        name: "never-there".to_owned(),
    }));
    assert_eq!(store.peek("github/env/GITHUB_TOKEN"), None);
    host.shutdown();
}
