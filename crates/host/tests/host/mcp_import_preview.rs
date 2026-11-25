//! The import's preview in `McpToolsView::importable`: each server an app offers, how it is
//! reached, its program or its address's origin only, and whether Clauseo has its name already.

use clauseo_protocol::{McpImportServer, McpServerKind};

use super::*;

/// A token in the remote server's path and query: the preview shows the origin only.
const URL_TOKEN: &str = "otter-quarry-lantern";

/// Claude Desktop offers `github` (local, its program on the search path) and `notes`
/// (remote, on a port of its own, with a token in its path and query); the lawyer has
/// `github` already.
#[test]
fn mcp_import_preview_names_kind_text_and_already_added() {
    let dir = Dir::new("mcp_import_preview", &[]);
    let mine = json!({"command": program(), "enabled": true});
    dir.set_settings(&[], json!({"mcp": {"servers": {"github": mine}}}));
    let url = format!("https://notes.example.test:8443/{URL_TOKEN}/mcp?key={URL_TOKEN}");
    let desktop = json!({"mcpServers": {
        "github": {"command": program(), "args": ["-y", "server-github"]},
        "notes": {"type": "http", "url": url},
    }});
    let fs = FakeFs::default()
        .file(&program(), "")
        .file(&desktop_config(), &desktop.to_string());
    let store = Arc::new(MemoryStore::new());
    let host = dir.start_with(0, |config| wired(config, &store, fs));
    let mut sub = subscribe(&host);
    let view = view_until(&mut sub, |view| !view.importable.is_empty());
    assert_eq!(counts(&view.importable), [(ClaudeDesktop, 1)], "{view:?}");
    assert_eq!(
        view.importable[0].servers,
        [
            McpImportServer {
                name: "github".to_owned(),
                kind: McpServerKind::Local,
                text: npx_name().to_owned(),
                already_added: true,
            },
            McpImportServer {
                name: "notes".to_owned(),
                kind: McpServerKind::Remote,
                text: "https://notes.example.test:8443".to_owned(),
                already_added: false,
            },
        ]
    );
    let text = serde_json::to_string(&view).unwrap();
    assert!(
        !text.contains(URL_TOKEN),
        "the view holds the address's path or query"
    );
    host.shutdown();
}
