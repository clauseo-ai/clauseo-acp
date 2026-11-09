//! The browser as a shell tool (`ProfileHook::browser_tool`): a session whose launch prep
//! finds a Chrome gets agent-browser's variables, the tools folder on its agent's `PATH`, the
//! instructions' browser part and `Available` in its header; a session with no Chrome, or with
//! `tools.browser` off, gets none of them, a no-browser line and the reason in its header.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_host::browser::chrome::{ChromeRoots, Platform};
use clauseo_profile::{BrowserSidecars, ClauseoProfile, ProfilePaths};
use clauseo_protocol::{ClauseoTool, ToolsUnavailableReason};
use clauseo_settings::Locations;
use serde_json::{Value, json};

use crate::profile_rig::{
    CODEX, Hook, Seen, Wired, agents, codex_lines, first_frame_tools, settings, template, tool,
    turn,
};
use crate::support::Dir;
use crate::write;

/// `tools.browser` on: the default on macOS and Windows, not on Linux.
const ON: &str = r#"{"tools": {"browser": true}}"#;

/// The variables the shell tool sets, by name.
const NAMES: [&str; 5] = [
    "AGENT_BROWSER_SESSION",
    "AGENT_BROWSER_CONFIG",
    "AGENT_BROWSER_DOWNLOAD_PATH",
    "AGENT_BROWSER_PROFILE",
    "AGENT_BROWSER_EXECUTABLE_PATH",
];

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

/// One Codex turn on a host wired with the shell tool (`<data>/tools`, holding an
/// `agent-browser` file), `chrome` as the session's Chrome (none: a search of empty roots) and
/// `extra` settings; the launch seen, its developer instructions and its header's tools.
fn launch(
    name: &str,
    chrome: Option<&Path>,
    extra: &str,
) -> (Dir, Seen, String, Vec<clauseo_protocol::ToolState>) {
    let dir = Dir::new(name, &[]);
    let codex = template(&dir, name, &codex_lines(name, &json!({})));
    let extra: Value = serde_json::from_str(extra).unwrap();
    settings(&dir, agents(&[(CODEX, &codex, json!({}))]), extra);
    let tools = dir.data().join("tools");
    let command = format!("agent-browser{}", std::env::consts::EXE_SUFFIX);
    write::write(&tools.join(command), b"stand-in");
    let hook = Hook {
        profile: Some(Arc::new(profile(dir.data()))),
        browser_tool: Some(tools),
        ..Hook::default()
    };
    let empty = ChromeRoots {
        platform: Platform::MacArm64,
        root: dir.data().join("no-root"),
        home: None,
        program_files: None,
        program_files_x86: None,
        local_app_data: None,
        app_data: dir.data().join("no-app"),
    };
    let stetdoc = dir.data().join("stetdoc");
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        config
            .with_profile_hook(Box::new(hook))
            .with_agent_path_prepend(stetdoc.clone())
            .with_browser_executable(chrome.map(Path::to_path_buf))
            .with_chrome_roots(empty)
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CODEX);
    turn(&host, thread);
    let header = first_frame_tools(&host, thread);
    host.shutdown();
    let [seen]: [Seen; 1] = launcher.seen().try_into().unwrap();
    let config = seen.codex_config().expect("a CODEX_CONFIG");
    let instructions = config["developer_instructions"]
        .as_str()
        .unwrap()
        .to_owned();
    (dir, seen, instructions, header)
}

/// The value of `name` in `seen`'s environment, as a path.
fn var(seen: &Seen, name: &str) -> Option<PathBuf> {
    seen.env.get(&OsString::from(name)).map(PathBuf::from)
}

/// `seen`'s `PATH`, split.
fn path_parts(seen: &Seen) -> Vec<PathBuf> {
    let (_, path) = seen
        .env
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
fn a_session_with_a_chrome_gets_the_shell_browser() {
    let chrome = PathBuf::from("/opt/chrome/chrome");
    let (dir, seen, instructions, header) = launch("shell_browser_on", Some(&chrome), ON);
    assert_eq!(
        header_browser(&header),
        Some(tool(ClauseoTool::Browser, None)),
        "{instructions}"
    );
    let run = seen.run.clone().expect("a run folder");
    let data = dir.data();
    assert!(run.starts_with(data.join("run")));
    let session = run.file_name().unwrap().to_str().unwrap().to_owned();
    assert_eq!(
        var(&seen, "AGENT_BROWSER_SESSION"),
        Some(PathBuf::from(&session))
    );
    assert_eq!(
        var(&seen, "AGENT_BROWSER_CONFIG"),
        Some(data.join("browser").join("agent-browser.json"))
    );
    assert_eq!(
        std::fs::read(data.join("browser").join("agent-browser.json")).unwrap(),
        b"{}\n"
    );
    // The session's own profile and downloads, under the data folder's run folder for this
    // session: never the lawyer's own Chrome profile.
    let profile = var(&seen, "AGENT_BROWSER_PROFILE").unwrap();
    assert_eq!(profile, run.join("browser-profile"));
    assert!(profile.starts_with(data) && profile.parent() == Some(run.as_path()));
    assert_eq!(
        var(&seen, "AGENT_BROWSER_DOWNLOAD_PATH"),
        Some(run.join("downloads"))
    );
    assert_eq!(var(&seen, "AGENT_BROWSER_EXECUTABLE_PATH"), Some(chrome));
    for name in [
        "AGENT_BROWSER_PROXY",
        "AGENT_BROWSER_STREAM_PORT",
        "AGENT_BROWSER_SOCKET_DIR",
    ] {
        assert_eq!(var(&seen, name), None, "{name}");
    }
    let parts = path_parts(&seen);
    assert_eq!(parts.first(), Some(&data.join("stetdoc")));
    assert_eq!(parts.get(1), Some(&data.join("tools")));
    assert!(instructions.contains("use the `agent-browser` command in your shell"));
    // Codex's line: ask to run unsandboxed, with one prefix rule to approve.
    assert!(instructions.contains(r#"`sandbox_permissions: "require_escalated"`"#));
    assert!(instructions.contains(r#"`prefix_rule: ["agent-browser"]`"#));
    assert!(!instructions.contains("{browser}"));
    assert_eq!(
        header_browser(&header),
        Some(tool(ClauseoTool::Browser, None))
    );
}

#[test]
fn a_session_with_no_chrome_gets_no_browser_and_is_told_why() {
    let (dir, seen, instructions, header) = launch("shell_browser_none", None, ON);
    for name in NAMES {
        assert_eq!(var(&seen, name), None, "{name}");
    }
    assert!(!path_parts(&seen).contains(&dir.data().join("tools")));
    assert!(!instructions.contains("agent-browser"), "{instructions}");
    assert!(
        instructions.contains("found no Google Chrome"),
        "{header:?} {instructions}"
    );
    let run = seen.run.as_ref().unwrap();
    assert!(!run.join("browser-profile").exists());
    assert_eq!(
        header_browser(&header),
        Some(tool(
            ClauseoTool::Browser,
            Some(ToolsUnavailableReason::BrowserNotInstalled)
        ))
    );
}

#[test]
fn a_session_with_the_browser_off_gets_no_browser() {
    let chrome = PathBuf::from("/opt/chrome/chrome");
    let (dir, seen, instructions, header) = launch(
        "shell_browser_off",
        Some(&chrome),
        r#"{"tools": {"browser": false}}"#,
    );
    for name in NAMES {
        assert_eq!(var(&seen, name), None, "{name}");
    }
    assert!(!path_parts(&seen).contains(&dir.data().join("tools")));
    assert!(!instructions.contains("agent-browser"), "{instructions}");
    assert!(instructions.contains("You have no browser in this session."));
    assert!(!instructions.contains("found no Google Chrome"));
    assert_eq!(
        header_browser(&header),
        Some(tool(
            ClauseoTool::Browser,
            Some(ToolsUnavailableReason::Off)
        ))
    );
}

/// The header's browser state.
fn header_browser(tools: &[clauseo_protocol::ToolState]) -> Option<clauseo_protocol::ToolState> {
    tools
        .iter()
        .find(|state| state.tool == ClauseoTool::Browser)
        .cloned()
}
