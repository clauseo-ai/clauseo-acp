//! The shell tool's and the entry's parts, header states and support line, and Claude's rules.

use std::path::PathBuf;

use super::*;

/// The support line names the part and a known Chrome's file name, never a folder; any other
/// file name reads `other`.
#[test]
fn the_part_log_names_a_known_chrome_file_and_no_folder() {
    let edge = Path::new(r"C:\Users\someone\AppData\Local\Microsoft\Edge\msedge.exe");
    let edge = if cfg!(windows) {
        edge.to_path_buf()
    } else {
        PathBuf::from("/home/someone/edge").join("msedge.exe")
    };
    assert_eq!(part_log(BrowserPart::On, Some(&edge)), ("on", "msedge.exe"));
    let own = PathBuf::from("/home/someone/bin").join("someones-browser");
    assert_eq!(part_log(BrowserPart::On, Some(&own)), ("on", "other"));
    assert_eq!(
        part_log(BrowserPart::Off, Some(&edge)),
        ("off", "msedge.exe")
    );
    assert_eq!(
        part_log(BrowserPart::NotInstalled, None),
        ("not_installed", "none")
    );
}

/// The six variables, each exactly once, with the values given (the idle timeout 10 minutes,
/// in milliseconds): nothing about a proxy, a stream port or a socket folder. The config, the
/// downloads and the profile all sit in the session's own `agent-browser` folder, never in
/// `<data>/browser` (`launch_checks`' tests check the folders the launch builds).
#[test]
fn shell_env_is_the_session_config_folders_and_chrome() {
    // Joined as the host joins them, so the separator is the OS's own (`\\` on Windows).
    let data = PathBuf::from("/data");
    let session = "t-1".to_owned();
    let own = data.join("run").join(&session).join("agent-browser");
    let config = own.join("config.json");
    let downloads = own.join("downloads");
    let profile = own.join("profile");
    let inputs = ShellInputs {
        session: &session,
        config: &config,
        downloads: &downloads,
        profile: &profile,
        executable: Path::new("/opt/chrome"),
    };
    let env = shell_env(&inputs);
    let text: Vec<(&str, &str)> = env
        .iter()
        .map(|(name, value)| (name.to_str().unwrap(), value.to_str().unwrap()))
        .collect();
    assert_eq!(
        text,
        [
            ("AGENT_BROWSER_CONFIG", config.to_str().unwrap()),
            ("AGENT_BROWSER_DOWNLOAD_PATH", downloads.to_str().unwrap()),
            ("AGENT_BROWSER_EXECUTABLE_PATH", "/opt/chrome"),
            ("AGENT_BROWSER_IDLE_TIMEOUT_MS", "600000"),
            ("AGENT_BROWSER_PROFILE", profile.to_str().unwrap()),
            ("AGENT_BROWSER_SESSION", session.as_str()),
        ]
    );
    for name in [
        "AGENT_BROWSER_CONFIG",
        "AGENT_BROWSER_DOWNLOAD_PATH",
        "AGENT_BROWSER_PROFILE",
    ] {
        let path = Path::new(&env[&OsString::from(name)]);
        assert!(path.starts_with(&own), "{name}");
        assert!(!path.starts_with(data.join("browser")), "{name}");
    }
}

/// Codex's part follows its entry: on when it goes, not installed only for want of a Chrome,
/// off for every other reason.
#[test]
fn entry_part_follows_the_entry() {
    let state = |availability| ToolState {
        tool: ClauseoTool::Browser,
        availability,
    };
    let out = |reason| state(ToolAvailability::Unavailable { reason });
    assert_eq!(
        entry_part(&state(ToolAvailability::Available), true),
        BrowserPart::On
    );
    let missing = out(ToolsUnavailableReason::BrowserNotInstalled);
    assert_eq!(entry_part(&missing, false), BrowserPart::NotInstalled);
    // A Chrome found but a sidecar or a claim missing: no "install Chrome" advice.
    assert_eq!(entry_part(&missing, true), BrowserPart::Off);
    for reason in [
        ToolsUnavailableReason::Off,
        ToolsUnavailableReason::LockedOff,
        ToolsUnavailableReason::ManagedByFirm,
        ToolsUnavailableReason::TooManyBrowsers,
        ToolsUnavailableReason::NoProxy,
        ToolsUnavailableReason::BrowserBlockedByPolicy,
    ] {
        assert_eq!(
            entry_part(&out(reason), false),
            BrowserPart::Off,
            "{reason:?}"
        );
    }
}

#[test]
fn shell_state_says_why_there_is_no_browser() {
    let on = Switch {
        on: true,
        locked: false,
    };
    let off = Switch { on: false, ..on };
    let locked = Switch {
        on: false,
        locked: true,
    };
    let blocked = Some(ToolsUnavailableReason::BrowserBlockedByPolicy);
    let cases = [
        (on, None, true, None, BrowserPart::On),
        (
            on,
            None,
            false,
            Some(ToolsUnavailableReason::BrowserNotInstalled),
            BrowserPart::NotInstalled,
        ),
        (
            off,
            None,
            false,
            Some(ToolsUnavailableReason::Off),
            BrowserPart::Off,
        ),
        (
            locked,
            None,
            false,
            Some(ToolsUnavailableReason::LockedOff),
            BrowserPart::Off,
        ),
        (on, blocked, false, blocked, BrowserPart::Off),
    ];
    for (switch, block, found, reason, part) in cases {
        let (state, got) = shell_state(switch, block, found);
        assert_eq!(state.tool, ClauseoTool::Browser);
        let want = match reason {
            Some(reason) => ToolAvailability::Unavailable { reason },
            None => ToolAvailability::Available,
        };
        assert_eq!(
            (state.availability, got),
            (want, part),
            "{switch:?} {block:?}"
        );
    }
}

/// Claude's agent-browser rules come only with the browser in that session: on, the allow
/// and deny rules; off or unavailable, none, and a hook's stray agent-browser rule is dropped.
#[test]
fn claude_gets_agent_browser_rules_only_with_the_browser() {
    use crate::agent::loads::profile::{ClaudeKeys, Folders, Login, claude_options};
    let names = |options: &serde_json::Map<String, serde_json::Value>, key: &str| {
        options
            .get(key)
            .and_then(serde_json::Value::as_array)
            .map(|rules| {
                rules
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .filter(|rule| rule.contains("agent-browser"))
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let data = crate::agent::data_fence::LaunchDeny::default();
    for browser in [true, false] {
        let mut options = serde_json::Map::new();
        options.insert(
            "allowedTools".to_owned(),
            serde_json::json!(["Bash(agent-browser eval:*)"]),
        );
        let keys = ClaudeKeys {
            web_search: Switch {
                on: true,
                locked: false,
            },
            login: Login::FirstParty,
            folders: Folders {
                work: "/w",
                notes: "/n",
                connectors: "",
                browser: "",
            },
            seed_sites: &[],
            forced_ask: false,
            data: &data,
            case_law: false,
            browser,
            sandbox: None,
            matter: None,
            trusted_servers: &[],
        };
        claude_options(&mut options, &keys);
        let allowed = names(&options, "allowedTools");
        let denied = names(&options, "disallowedTools");
        let asked = options
            .get("settings")
            .and_then(|settings| settings.get("permissions"))
            .and_then(serde_json::Value::as_object)
            .map(|permissions| names(permissions, "ask"))
            .unwrap_or_default();
        if browser {
            let want: Vec<String> = clauseo_profile::claude::agent_browser_allowed().collect();
            assert_eq!(allowed, want);
            for sub in [" eval", " pdf", " download"] {
                assert!(!allowed.iter().any(|rule| rule.contains(sub)), "{sub}");
            }
            assert!(allowed.contains(&"Bash(agent-browser read:*)".to_owned()));
            let want: Vec<String> = clauseo_profile::claude::agent_browser_denied().collect();
            assert_eq!(denied, want);
            // eval, pdf, download and upload are refused (a deny rule wins over allow and ask),
            // bare, behind `--json` and behind any option; none of them asks.
            for sub in ["eval", "pdf", "download", "upload"] {
                for rule in [
                    format!("Bash(agent-browser {sub}*)"),
                    format!("Bash(agent-browser --json {sub}*)"),
                    format!("Bash(agent-browser * {sub}*)"),
                ] {
                    assert!(denied.contains(&rule), "{rule}");
                }
                assert!(!asked.iter().any(|rule| rule.contains(sub)), "{sub}");
            }
            // A wait running page JavaScript is refused though `wait` is allowed.
            assert!(denied.contains(&"Bash(agent-browser wait*-f*)".to_owned()));
            let want: Vec<String> = clauseo_profile::claude::agent_browser_ask().collect();
            assert_eq!(asked, want);
            assert_eq!(asked[0], "Bash(agent-browser *--init-script*)");
        } else {
            assert_eq!(
                (allowed, denied, asked),
                (Vec::new(), Vec::new(), Vec::new())
            );
        }
    }
}
