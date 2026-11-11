//! Settings › Tools' import at the host's interface (PR A3): the counts per app in the view, an
//! import per app with its skipped servers and why, the values in the keychain double and never
//! in the settings file or a log, and the firm's lock.
//!
//! The finder is desktop's (`find_mcp_imports_in`, the settings crate's `find_candidates` with
//! the bare-name rule) over a fake filesystem: Claude Desktop, Claude Code (user and project
//! scopes) and Codex config under a fake home, on this machine's OS rules, so each program's
//! path is absolute here. The import log test installs the log
//! file, the process's global subscriber: nextest runs each test in a process of its own.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_connectors::MemoryStore;
use clauseo_host::{
    AppSubscription, HostConfig, McpFileStamp, McpFound, McpImportFinder, log_file,
};
use clauseo_protocol::{
    ErrorCode, HostCommand, HostEvent, McpCommand, McpImportResult, McpImportSource, McpImportable,
    McpSkipReason, McpSkipped, McpToolsView,
};
use clauseo_settings::Locations;
use clauseo_settings::mcp_import::{Env, ImportFs, Os};
use futures::StreamExt as _;
use serde_json::{Value, json};

use McpImportSource::{ClaudeCode, ClaudeDesktop, Codex};

use crate::protection_rig::subscribe;
use crate::support::{Dir, wait};
use crate::write;

/// An env value in the apps' config: plain words, distinctive enough that finding it in a log,
/// the settings file or the view means a leak.
const ENV_VALUE: &str = "juniper-almanac-tidewater-kestrel";

/// A header value in the apps' config, as `ENV_VALUE`.
const HEADER_VALUE: &str = "Bearer copper-lighthouse-meadow-ferry";

/// A fake filesystem: files by path, with `\` read as `/` so one key fits both OSes.
#[derive(Default, Clone)]
struct FakeFs {
    files: BTreeMap<String, String>,
}

fn key(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

impl FakeFs {
    fn file(mut self, path: &str, text: &str) -> Self {
        self.files.insert(path.replace('\\', "/"), text.to_owned());
        self
    }
}

impl ImportFs for FakeFs {
    fn read_to_string(&self, p: &Path, max: u64) -> Option<String> {
        let text = self.files.get(&key(p))?;
        (u64::try_from(text.len()).ok()? <= max).then(|| text.clone())
    }

    fn is_file(&self, p: &Path) -> bool {
        self.files.contains_key(&key(p))
    }

    fn list_dir(&self, _: &Path) -> Vec<PathBuf> {
        Vec::new()
    }
}

/// This machine's OS rules, and the fake home under them.
fn os() -> Os {
    if cfg!(windows) { Os::Windows } else { Os::Mac }
}

fn home() -> &'static str {
    if cfg!(windows) {
        "C:\\Users\\ana"
    } else {
        "/Users/ana"
    }
}

/// `rel` under the fake home, with this OS's separator.
fn at(rel: &str) -> String {
    let sep = if cfg!(windows) { "\\" } else { "/" };
    format!("{}{sep}{}", home(), rel.replace('/', sep))
}

/// The fake `APPDATA` (Windows' Claude Desktop folder), `CODEX_HOME`, nvm-windows'
/// `NVM_HOME`, and a `PATH` holding [`program`]'s folder only.
fn vars(name: &str) -> Option<String> {
    match name {
        "PATH" | "Path" => Some(nvm_bin()),
        "NVM_HOME" => Some(at("nvm")),
        "APPDATA" => Some(at("AppData/Roaming")),
        "CODEX_HOME" => Some(at("codex")),
        _ => None,
    }
}

/// nvm's folder of the current Node, on this OS.
fn nvm_bin() -> String {
    if cfg!(windows) {
        at("nvm/v22.1.0")
    } else {
        at(".nvm/versions/node/v22.1.0/bin")
    }
}

/// The program most importable servers run: under nvm and on the fake `PATH`, so the settings
/// keep its bare name.
fn program() -> String {
    format!("{}{}{}", nvm_bin(), std::path::MAIN_SEPARATOR, npx_name())
}

/// `npx`'s file name on this OS.
fn npx_name() -> &'static str {
    if cfg!(windows) { "npx.cmd" } else { "npx" }
}

/// A program off the search path: the settings keep its absolute path.
fn off_path() -> String {
    at(&format!("opt/drafts/{}", npx_name()))
}

/// Claude Desktop's config file under the fake home.
fn desktop_config() -> String {
    if cfg!(windows) {
        at("AppData/Roaming/Claude/claude_desktop_config.json")
    } else {
        at("Library/Application Support/Claude/claude_desktop_config.json")
    }
}

/// The three apps' config:
/// - Claude Desktop: `github` (local, with `ENV_VALUE`) and `notes` (remote http, with
///   `HEADER_VALUE`) import; `files` runs a program not found (written as a path); `weather`
///   is `sse`.
/// - Claude Code: the user-wide `search` imports; a project's `.mcp.json` `repo-tool` is not
///   approved and its `repo-off` is disabled there.
/// - Codex: `drafts` (a program off the search path) imports; `billing` names a variable not
///   set; `paused` is `enabled = false`.
fn apps() -> FakeFs {
    let npx = program();
    let desktop = json!({"mcpServers": {
        "github": {"command": npx, "args": ["-y", "server-github"],
            "env": {"GITHUB_TOKEN": ENV_VALUE}},
        "notes": {"type": "http", "url": "https://notes.example.test/mcp",
            "headers": {"Authorization": HEADER_VALUE}},
        "files": {"command": at("tools/uvx-missing"), "args": ["files"]},
        "weather": {"type": "sse", "url": "https://weather.example.test/sse"},
    }});
    let project = at("Matters/Jones");
    let claude = json!({
        "mcpServers": {"search": {"command": npx}},
        "projects": {project.clone(): {"hasTrustDialogAccepted": true,
            "enabledMcpjsonServers": [], "disabledMcpjsonServers": ["repo-off"]}},
    });
    let mcp_json = json!({"mcpServers": {
        "repo-tool": {"command": npx}, "repo-off": {"command": npx},
    }});
    let drafts = off_path();
    let codex = format!(
        "[mcp_servers.drafts]\ncommand = '{drafts}'\n\n\
         [mcp_servers.billing]\ncommand = '{npx}'\nenv_vars = [\"NOT_SET_ANYWHERE\"]\n\n\
         [mcp_servers.paused]\ncommand = '{npx}'\nenabled = false\n"
    );
    let desktop_path = desktop_config();
    FakeFs::default()
        .file(&npx, "")
        .file(&off_path(), "")
        .file(&desktop_path, &desktop.to_string())
        .file(&at(".claude.json"), &claude.to_string())
        .file(&format!("{project}/.mcp.json"), &mcp_json.to_string())
        .file(&at("codex/config.toml"), &codex)
}

/// The finder over a fake filesystem: desktop's rule (`find_mcp_imports_in`) on this OS's rules
/// under the fake home, each fake path its own canonical form. It knows no files, so no scan is
/// ever skipped as unchanged.
struct FakeFinder(Arc<FakeFs>);

impl McpImportFinder for FakeFinder {
    fn find(&self, existing: &[String]) -> McpFound {
        let env = Env {
            os: os(),
            home: Path::new(home()),
            vars: &vars,
            fs: self.0.as_ref(),
        };
        let canonical = |path: &Path| Some(path.to_path_buf());
        clauseo_host::find_mcp_imports_in(&env, &canonical, existing)
    }

    fn stamps(&self, _: &[PathBuf]) -> Vec<McpFileStamp> {
        Vec::new()
    }
}

/// The config with the keychain double, a finder over `fs`, and a confirm that allows every
/// import (`confirm::Answers`).
fn wired(config: HostConfig, store: &Arc<MemoryStore>, fs: FakeFs) -> HostConfig {
    config
        .with_mcp_secrets(Arc::<MemoryStore>::clone(store))
        .with_mcp_confirm(Arc::new(confirm::Answers::allowing(true)))
        .with_mcp_import(Arc::new(FakeFinder(Arc::new(fs))))
}

/// The next `McpToolsChanged` on `sub` that `wanted` accepts, skipping every other event.
fn view_until(sub: &mut AppSubscription, wanted: impl Fn(&McpToolsView) -> bool) -> McpToolsView {
    wait("an McpToolsChanged", async {
        loop {
            let item = sub.next().await.expect("the app stream ended");
            if let HostEvent::McpToolsChanged(view) = crate::settings::decode(&item)
                && wanted(&view)
            {
                return view;
            }
        }
    })
}

fn import(source: McpImportSource) -> HostCommand {
    HostCommand::Mcp(McpCommand::Import { source })
}

fn counts(pairs: &[(McpImportSource, u32)]) -> Vec<McpImportable> {
    pairs
        .iter()
        .map(|&(source, count)| McpImportable { source, count })
        .collect()
}

fn skip(name: &str, reason: McpSkipReason, program: Option<&str>) -> McpSkipped {
    McpSkipped {
        name: name.to_owned(),
        reason,
        program: program.map(str::to_owned),
    }
}

/// Imports from `source` and waits for the view naming that import.
fn imported(
    host: &crate::support::Running,
    sub: &mut AppSubscription,
    source: McpImportSource,
) -> (McpToolsView, McpImportResult) {
    host.ok(import(source));
    // Each import's result differs from the one before in this test, so the view is sent.
    let view = view_until(sub, |view| {
        view.last_import
            .as_ref()
            .is_some_and(|last| last.source == source)
    });
    let last = view.last_import.clone().unwrap();
    (view, last)
}

#[path = "mcp_import_confirm.rs"]
mod confirm;
#[path = "mcp_import_safety.rs"]
mod safety;

/// The log files under `dir`, oldest first.
fn read_logs(dir: &Path) -> String {
    ["host.2.log", "host.1.log", "host.log"]
        .iter()
        .map(|name| std::fs::read_to_string(dir.join(name)).unwrap_or_default())
        .collect()
}

/// The counts per app, then an import from each with its skipped servers; values go to the
/// keychain double only, never to the settings file, the view or a log.
#[test]
fn mcp_import_counts_imports_and_skips_and_keeps_values_in_the_keychain() {
    let dir = Dir::new("mcp_import", &[]);
    let logs = dir.data().join("logs-under-test");
    write::create_dir_all(&logs);
    let locations = Locations::under_root(&logs);
    let guard = log_file::install(&locations);
    assert!(guard.is_active(), "the log did not start");

    let store = Arc::new(MemoryStore::new());
    let host = dir.start_with(0, |config| wired(config, &store, apps()));
    let mut sub = subscribe(&host);
    let view = view_until(&mut sub, |view| !view.importable.is_empty());
    assert_eq!(
        view.importable,
        counts(&[(ClaudeDesktop, 2), (ClaudeCode, 1), (Codex, 1)]),
        "{view:?}"
    );
    assert_eq!(view.last_import, None);

    // Claude Desktop: two in, two skipped with why.
    let (view, last) = imported(&host, &mut sub, ClaudeDesktop);
    assert_eq!(last.imported, ["github", "notes"]);
    assert_eq!(
        last.skipped,
        [
            skip("files", McpSkipReason::CommandNotFound, Some("uvx-missing")),
            skip("weather", McpSkipReason::UnsupportedTransport, None),
        ]
    );
    let names: Vec<&str> = view.servers.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["github", "notes"]);
    assert!(
        view.servers.iter().all(|server| server.enabled),
        "the confirm allowed them: they arrive turned on"
    );
    // On the search path: the bare name, which a launch finds again after an upgrade.
    assert_eq!(view.servers[0].command.as_deref(), Some(npx_name()));
    assert_eq!(view.servers[0].args, ["-y", "server-github"]);
    assert!(view.servers[0].env.iter().all(|var| var.saved));
    assert_eq!(
        view.servers[1].url.as_deref(),
        Some("https://notes.example.test/mcp")
    );
    assert_eq!(
        view.importable,
        counts(&[(ClaudeCode, 1), (Codex, 1)]),
        "the imported ones no longer count"
    );
    assert_eq!(
        store.peek("github/env/GITHUB_TOKEN").as_deref(),
        Some(ENV_VALUE.as_bytes())
    );
    assert_eq!(
        store.peek("notes/header/Authorization").as_deref(),
        Some(HEADER_VALUE.as_bytes())
    );
    let text = dir.settings_json();
    for value in [ENV_VALUE, HEADER_VALUE] {
        assert!(!text.contains(value), "settings.json holds a value");
        assert!(!serde_json::to_string(&view).unwrap().contains(value));
    }
    let file: Value = serde_json::from_str(&text).unwrap();
    let servers = &file["mcp"]["servers"];
    assert_eq!(
        servers["github"]["env"]["GITHUB_TOKEN"],
        json!({"keychain": true})
    );
    assert_eq!(
        servers["notes"]["headers"]["Authorization"],
        json!({"keychain": true})
    );
    assert_eq!(servers["github"]["enabled"], true);

    // Claude Code: the user's server in, the project's unapproved one skipped.
    let (view, last) = imported(&host, &mut sub, ClaudeCode);
    assert_eq!(last.imported, ["search"]);
    assert!(view.servers.iter().all(|server| server.name != "repo-off"));
    assert_eq!(
        last.skipped,
        [skip("repo-tool", McpSkipReason::NotApproved, None)]
    );

    // Codex: one in, one with a variable not set.
    let (view, last) = imported(&host, &mut sub, Codex);
    assert_eq!(last.imported, ["drafts"]);
    // Off the search path: the absolute path found.
    let drafts = view.servers.iter().find(|server| server.name == "drafts");
    assert_eq!(
        drafts.and_then(|server| server.command.as_deref()),
        Some(off_path().as_str()),
        "{view:?}"
    );
    assert_eq!(
        last.skipped,
        [skip("billing", McpSkipReason::UnresolvedVariable, None)]
    );
    assert!(view.importable.is_empty(), "{view:?}");
    // What the apps turned off never comes in, and is not offered as a skip either.
    for off in ["repo-off", "paused"] {
        assert!(
            view.servers.iter().all(|server| server.name != off),
            "{off}"
        );
        assert!(last.skipped.iter().all(|one| one.name != off), "{off}");
    }

    // Claude Desktop again: everything already added or still not importable.
    let (view, last) = imported(&host, &mut sub, ClaudeDesktop);
    safety::assert_no_values_or_paths(&view);
    assert_eq!(last.imported, Vec::<String>::new());
    assert_eq!(
        last.skipped,
        [
            skip("files", McpSkipReason::CommandNotFound, Some("uvx-missing")),
            skip("github", McpSkipReason::AlreadyAdded, None),
            skip("notes", McpSkipReason::AlreadyAdded, None),
            skip("weather", McpSkipReason::UnsupportedTransport, None),
        ]
    );
    assert_eq!(
        store.peek("github/env/GITHUB_TOKEN").as_deref(),
        Some(ENV_VALUE.as_bytes()),
        "a skipped server touched the held value"
    );
    host.shutdown();
    drop(guard);

    assert_logged_without_values(&locations.log_dir());
}

/// The log files under `dir` name an import and hold no value.
fn assert_logged_without_values(dir: &Path) {
    let log = read_logs(dir);
    assert!(
        log.contains("MCP servers imported"),
        "the log holds no import:\n{log}"
    );
    for value in [ENV_VALUE, HEADER_VALUE, "copper-lighthouse"] {
        assert!(!log.contains(value), "the log holds a value");
    }
}

/// A keychain failure imports nothing: no item left, the settings file unchanged.
#[test]
fn mcp_import_keychain_failure_imports_nothing() {
    let dir = Dir::new("mcp_import_keychain", &[]);
    let store = Arc::new(MemoryStore::new());
    let host = dir.start_with(0, |config| wired(config, &store, apps()));
    let before = dir.settings_json();
    store.fail_next_writes(1);
    host.refused(
        import(McpImportSource::ClaudeDesktop),
        ErrorCode::McpSecretStoreFailed,
    );
    host.ok(HostCommand::Mcp(McpCommand::RemoveServer {
        name: "never-there".to_owned(),
    }));
    assert_eq!(store.peek("github/env/GITHUB_TOKEN"), None);
    assert_eq!(store.peek("notes/header/Authorization"), None);
    assert_eq!(dir.settings_json(), before);
    host.shutdown();
}

/// The firm's list (policy) and its `managed-mcp.json` refuse an import with `settings.locked`,
/// and the view offers nothing to import.
#[test]
fn mcp_import_is_locked_by_the_firm() {
    let dir = Dir::new("mcp_import_firm_list", &[]);
    dir.set_policy(&json!({"mcp": {"servers": {"firm": {"command": program()}}}}).to_string());
    let store = Arc::new(MemoryStore::new());
    let host = dir.start_with(0, |config| wired(config, &store, apps()));
    let mut sub = subscribe(&host);
    let view = view_until(&mut sub, |_| true);
    assert!(view.importable.is_empty(), "{view:?}");
    host.refused(
        import(McpImportSource::ClaudeDesktop),
        ErrorCode::SettingsLocked,
    );
    assert_eq!(store.peek("github/env/GITHUB_TOKEN"), None);
    host.shutdown();

    let dir = Dir::new("mcp_import_firm_managed", &[]);
    dir.set_settings(&[], json!({}));
    dir.set_managed_mcp();
    let before = dir.settings_json();
    let host = dir.start_with(0, |config| wired(config, &store, apps()));
    let mut sub = subscribe(&host);
    let view = view_until(&mut sub, |_| true);
    assert!(view.importable.is_empty(), "{view:?}");
    host.refused(import(McpImportSource::Codex), ErrorCode::SettingsLocked);
    assert_eq!(dir.settings_json(), before);
    assert_eq!(store.peek("github/env/GITHUB_TOKEN"), None);
    host.shutdown();
}
