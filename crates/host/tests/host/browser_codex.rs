//! Codex's browser is the `browser` MCP entry, Claude's the shell tool, on one host wired
//! with both (as the app is): the Codex session (every OS) opens one egress proxy, since
//! its entry asks for one (`{proxy_port}`, ADR 0053), and gets the entry filled with the proxy's
//! address and its browser listener's password (in a variable, in no argument), no
//! `AGENT_BROWSER_*` variable of its own, no tools folder on its `PATH` and the entry's line in
//! its instructions; the Claude session gets no entry, no shell tool and no proxy (no browser at
//! all on any OS, `servers::no_browser`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_host::TestProxy;
use clauseo_profile::{BrowserSidecars, ClauseoProfile, ProfilePaths};
use clauseo_protocol::{ClauseoTool, ToolAvailability};
use clauseo_settings::Locations;
use serde_json::json;

use crate::egress::{Recording, calls};
use crate::profile_rig::{
    CLAUDE, CODEX, Hook, Seen, Wired, agent_lines, agents, browser_server, codex_lines,
    filled_browser, first_frame_tools, settings, template, turn,
};
use crate::support::Dir;
use crate::write;

/// The launch profile under `root`, with the launch instructions (`{browser}` among them).
fn profile(root: &Path) -> ClauseoProfile {
    let paths = ProfilePaths::new(
        &Locations::under_root(root),
        root.join("app").join("clauseo"),
        root.join("stetdoc").join("stetdoc"),
        BrowserSidecars {
            clauseo_mcp: root.join("app").join("clauseo-mcp"),
            agent_browser: root.join("app").join("clauseo-agent-browser"),
        },
    )
    .unwrap();
    ClauseoProfile::new(paths, None)
}

/// `PATH`'s folders in `env`.
fn path_parts(env: &std::collections::BTreeMap<OsString, OsString>) -> Vec<PathBuf> {
    let (_, path) = env
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
        .expect("a PATH");
    let separator = if cfg!(windows) { ';' } else { ':' };
    path.to_str()
        .unwrap()
        .split(separator)
        .map(PathBuf::from)
        .collect()
}

/// The guard's file for Codex's session, read while the session runs: the password and a line
/// end, private to the user (no guard runs here to read and delete it).
fn proxy_secret_file_holds_the_password(codex_seen: &Seen) {
    let file = codex_seen.run.as_ref().unwrap().join("proxy-secret");
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(text, format!("{}\n", TestProxy::PASSWORD));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

/// Codex's launch: the entry only, no shell browser (no `AGENT_BROWSER_*` variable, no tools
/// folder on its `PATH`), and the proxy's password reaches the guard in its file only: not in
/// the agent's environment, and its stderr tail hides it where the proxy opened.
fn codex_env_has_no_shell_browser_and_no_password(codex_seen: &Seen, tools: &Path) {
    assert!(
        codex_seen
            .env
            .keys()
            .all(|name| !name.to_string_lossy().starts_with("AGENT_BROWSER_")),
        "{:?}",
        codex_seen.env.keys().collect::<Vec<_>>()
    );
    assert!(!path_parts(&codex_seen.env).contains(&tools.to_path_buf()));
    assert!(
        codex_seen
            .env
            .values()
            .all(|value| !value.to_string_lossy().contains(TestProxy::PASSWORD)),
        "the proxy password is in Codex's environment"
    );
    assert!(
        codex_seen
            .redacted
            .iter()
            .any(|secret| secret == TestProxy::PASSWORD)
    );
}

#[test]
fn codex_gets_the_entry_with_its_proxy_and_claude_no_browser() {
    let dir = Dir::new("browser_codex", &[]);
    // Exactly these arguments and variables (the password in none of them), with
    // `AGENT_BROWSER_PROXY=http://127.0.0.1:8123` (the recording's port) and
    // `CLAUSEO_PROXY_SECRET_FILE` the run folder's `proxy-secret`, holding `TestProxy::PASSWORD`.
    let entry = filled_browser();
    // Codex has the entry on every OS, Windows included (the guard's session key, #1091).
    let servers = json!([entry]);
    let codex = template(
        &dir,
        "bc-codex",
        &codex_lines("bc-codex", &json!({"mcpServers": servers})),
    );
    let claude = template(
        &dir,
        "bc-claude",
        &agent_lines("bc-claude", "session/new", &json!({"mcpServers": []}), &[]),
    );
    settings(
        &dir,
        agents(&[(CODEX, &codex, json!({})), (CLAUDE, &claude, json!({}))]),
        json!({"tools": {"browser": true, "web_search": false}}),
    );
    let tools = dir.data().join("tools");
    let command = format!("agent-browser{}", std::env::consts::EXE_SUFFIX);
    write::write(&tools.join(command), b"stand-in");
    let hook = Hook {
        profile: Some(Arc::new(profile(dir.data()))),
        browser_tool: Some(tools.clone()),
        // Codex's entry as `clauseo_profile::servers::browser` builds it, but for the tokens one
        // OS alone holds (as `browser_server`): `{proxy_port}` and `{proxy_secret_file}` among
        // them.
        codex_servers: vec![browser_server()],
        ..Hook::default()
    };
    let (proxy, log, _closes) = Recording::new(false);
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), move |config| {
        config
            .with_profile_hook(Box::new(hook))
            .with_egress_proxy(proxy)
            .with_browser_executable(Some(PathBuf::from("/chrome")))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));

    let codex_thread = host.create_thread(matter, CODEX);
    turn(&host, codex_thread);
    let header = first_frame_tools(&host, codex_thread);
    let browser = header
        .iter()
        .find(|state| state.tool == ClauseoTool::Browser);
    let browser = browser.expect("the header shows the browser");
    assert_eq!(browser.availability, ToolAvailability::Available);
    // Codex's entry asks for the session's proxy: exactly one opened, and it is open while its
    // agent runs.
    let opened = vec!["open 1".to_owned()];
    assert_eq!(calls(&log), opened);
    proxy_secret_file_holds_the_password(&launcher.seen()[0]);

    let claude_thread = host.create_thread(matter, CLAUDE);
    turn(&host, claude_thread);
    let claude_header = first_frame_tools(&host, claude_thread);
    // Claude's session opened none.
    assert_eq!(calls(&log), opened);
    host.shutdown();
    // Codex's proxy closed once, after its agent; nothing else (the recording logs each open
    // and close).
    let closed = vec!["open 1".to_owned(), "close 1".to_owned()];
    assert_eq!(calls(&log), closed);

    let seen = launcher.seen();
    assert_eq!(seen.len(), 2);
    let (codex_seen, claude_seen) = (&seen[0], &seen[1]);
    codex_env_has_no_shell_browser_and_no_password(codex_seen, &tools);
    let config = codex_seen.codex_config().expect("a CODEX_CONFIG");
    let instructions = config["developer_instructions"].as_str().unwrap();
    let line = "use the `browser` MCP server's tools";
    assert!(instructions.contains(line), "{instructions}");
    assert!(!instructions.contains("agent-browser"), "{instructions}");
    assert!(
        !instructions.contains("require_escalated"),
        "{instructions}"
    );
    // Claude never gets the entry (its `session/new` carried none).
    let claude_browser = claude_header
        .iter()
        .find(|state| state.tool == ClauseoTool::Browser);
    // On every OS, no browser at all (`servers::no_browser`): no `AGENT_BROWSER_*` variable,
    // no `agent-browser` on its `PATH`, and no browser in its header.
    assert!(
        claude_seen
            .env
            .keys()
            .all(|name| !name.to_string_lossy().starts_with("AGENT_BROWSER_")),
        "{:?}",
        claude_seen.env.keys().collect::<Vec<_>>()
    );
    assert!(!path_parts(&claude_seen.env).contains(&tools));
    assert_eq!(claude_browser, None);
    assert!(
        claude_seen
            .redacted
            .iter()
            .all(|secret| secret != TestProxy::PASSWORD)
    );
}
