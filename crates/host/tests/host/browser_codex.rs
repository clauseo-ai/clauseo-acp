//! Codex's browser is the `browser` MCP entry, Claude's the shell tool, on one host wired
//! with both (as the app is): the Codex session gets the entry filled with no proxy, no
//! `AGENT_BROWSER_*` variable, no tools folder on its `PATH` and the entry's line in its
//! instructions; the Claude session gets no entry and the shell tool. Neither opens an egress
//! proxy: the plain client's entry asks for none (it holds no `{proxy_port}`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_acp::McpServer;
use clauseo_profile::{BrowserSidecars, ClauseoProfile, ProfilePaths};
use clauseo_protocol::{ClauseoTool, ToolAvailability};
use clauseo_settings::Locations;
use serde_json::json;

use crate::egress::{Recording, calls};
use crate::profile_rig::{
    CLAUDE, CODEX, Hook, Wired, agent_lines, agents, browser_server, codex_lines,
    first_frame_tools, path_text, settings, sidecar, template, turn,
};
use crate::support::Dir;
use crate::write;

/// Codex's entry as `clauseo_profile::servers::browser` builds it, but for the tokens one OS
/// alone holds (as `browser_server`): no `AGENT_BROWSER_PROXY`, so no `{proxy_port}`.
fn codex_browser() -> McpServer {
    let McpServer::Stdio {
        name,
        command,
        args,
        env,
    } = browser_server();
    let env = env
        .into_iter()
        .filter(|(name, _)| name != "AGENT_BROWSER_PROXY")
        .collect();
    McpServer::Stdio {
        name,
        command,
        args,
        env,
    }
}

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

fn var(env: &std::collections::BTreeMap<OsString, OsString>, name: &str) -> bool {
    env.contains_key(&OsString::from(name))
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

#[test]
fn codex_gets_the_entry_with_no_proxy_and_claude_the_shell_tool() {
    let dir = Dir::new("browser_codex", &[]);
    let session = if cfg!(windows) {
        "{{any}}"
    } else {
        "@SESSION@"
    };
    let entry = json!({"name": "browser", "command": "{{any}}",
    "args": ["browser-guard", "--agent-browser", path_text(&sidecar()), "--cwd", "@BROWSER@",
        "--cdp-port", "{{any}}", "--", "mcp", "--tools", "core"],
    "env": [
        {"name": "AGENT_BROWSER_CONFIG", "value": "/cfg/agent-browser.json"},
        {"name": "AGENT_BROWSER_SESSION", "value": session},
        {"name": "AGENT_BROWSER_EXECUTABLE_PATH", "value": "/chrome"},
        {"name": "AGENT_BROWSER_STREAM_PORT", "value": "{{any}}"},
    ]});
    let codex = template(
        &dir,
        "bc-codex",
        &codex_lines("bc-codex", &json!({"mcpServers": [entry]})),
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
        codex_servers: vec![codex_browser()],
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
        .find(|state| state.tool == ClauseoTool::Browser)
        .expect("the header shows the browser");
    assert_eq!(browser.availability, ToolAvailability::Available);
    // No egress proxy for Codex's entry: none opened while its agent ran.
    assert_eq!(calls(&log), Vec::<String>::new());

    let claude_thread = host.create_thread(matter, CLAUDE);
    turn(&host, claude_thread);
    host.shutdown();
    // Neither session opened a proxy (the recording logs each open and close).
    assert_eq!(calls(&log), Vec::<String>::new());

    let seen = launcher.seen();
    assert_eq!(seen.len(), 2);
    let (codex_seen, claude_seen) = (&seen[0], &seen[1]);
    // Codex: the entry only, no shell browser.
    assert!(
        codex_seen
            .env
            .keys()
            .all(|name| !name.to_string_lossy().starts_with("AGENT_BROWSER_")),
        "{:?}",
        codex_seen.env.keys().collect::<Vec<_>>()
    );
    assert!(!path_parts(&codex_seen.env).contains(&tools));
    let config = codex_seen.codex_config().expect("a CODEX_CONFIG");
    let instructions = config["developer_instructions"].as_str().unwrap();
    assert!(
        instructions.contains("use your browser tools, the ones from the `browser` MCP server"),
        "{instructions}"
    );
    assert!(!instructions.contains("agent-browser"), "{instructions}");
    assert!(
        !instructions.contains("require_escalated"),
        "{instructions}"
    );
    // Claude: the shell tool, no entry (its `session/new` carried none).
    for name in ["AGENT_BROWSER_SESSION", "AGENT_BROWSER_EXECUTABLE_PATH"] {
        assert!(var(&claude_seen.env, name), "{name}");
    }
    assert!(!var(&claude_seen.env, "AGENT_BROWSER_PROXY"));
    assert!(path_parts(&claude_seen.env).contains(&tools));
}
