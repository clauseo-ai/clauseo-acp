//! The profile hook's MCP servers for one session, and above all its `browser` entry (S27's
//! agent-browser behind `clauseo-mcp browser-guard`, decision record 0030), which the profile
//! offers Codex alone (`clauseo_profile::servers::browser_mcp`); Claude has the browser as a
//! shell tool ([`shell_env`], [`shell_state`]).
//!
//! The hook sees only the agent's id, so the entry comes with tokens
//! (`clauseo_profile::placeholders::IN_SERVERS`). The host fills them here, once per session,
//! or drops the entry and says why in the thread header. In this order:
//!
//! 1. `tools.browser` off: `Off`, or `LockedOff` when policy set it (or could not be read).
//! 2. The firm's `managed-mcp.json` ([`McpControl::Managed`]): no Clauseo server at all goes,
//!    so `ManagedByFirm`.
//! 3. No free browser slot: `TooManyBrowsers`.
//! 4. An entry that asks for the session's proxy (it holds `{proxy_port}`: the parked Safe
//!    workspace's, never the plain client's Codex entry) with no proxy set: `NoProxy`.
//! 5. The firm's Chrome or Edge policy turns remote debugging off and its Clauseo policy does
//!    not allow Clauseo's browser anyway (`browser::rd_policy`): `BrowserBlockedByPolicy`; or,
//!    with no false, a place of that policy could not be read: `BrowserPolicyUnreadable`.
//! 6. No Chrome picked, a sidecar (`clauseo-mcp` or agent-browser) missing, or the session's
//!    ports or socket folder not claimed (`browser::session`): `BrowserNotInstalled`.
//! 7. For an entry that asks for it, the session's proxy did not open, so it has no port:
//!    `NoProxy`. The proxy opens only after 6 found everything, and only for such an entry
//!    (`actor/turn/launch_checks/egress.rs`).
//!
//! Each value is filled into one decoded string (one argument, one variable's value) in one
//! pass, so a folder whose name holds a token stays literal; MCP arguments go as a list with no
//! shell, so nothing more is escaped.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;

use clauseo_acp::McpServer;
use clauseo_profile::instructions::BrowserPart;
use clauseo_profile::placeholders::{
    self, BROWSER_DIR, BROWSER_EXECUTABLE, CDP_PORT, DAEMON_PORT, IN_SERVERS, PROXY_PORT,
    SESSION_ID, SOCKET_DIR, STREAM_PORT,
};
use clauseo_profile::servers::BROWSER;
use clauseo_protocol::{ClauseoTool, ToolAvailability, ToolState, ToolsUnavailableReason};

use super::McpControl;

/// A `tools.*` switch as the settings hold it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Switch {
    /// Its value.
    pub(crate) on: bool,
    /// Whether policy set it, or an unreadable policy source locked it.
    pub(crate) locked: bool,
}

impl Switch {
    /// The reason an off switch gives: `Off` for the lawyer's, `LockedOff` for the firm's.
    pub(crate) fn off_reason(self) -> ToolsUnavailableReason {
        if self.locked {
            ToolsUnavailableReason::LockedOff
        } else {
            ToolsUnavailableReason::Off
        }
    }
}

/// What the `browser` entry of one session needs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BrowserInputs<'a> {
    /// `tools.browser`.
    pub(crate) switch: Switch,
    /// Who controls MCP on this machine.
    pub(crate) control: McpControl,
    /// Whether this session holds a browser slot or one is free (`browser::limits`).
    pub(crate) slot_free: bool,
    /// Whether a proxy is set (`HostConfig::with_egress_proxy`).
    pub(crate) proxy_set: bool,
    /// The port of this session's proxy, if it opened.
    pub(crate) port: Option<u16>,
    /// Why the firm's browser policy left the browser out, if it did (`browser::rd_policy`).
    pub(crate) blocked_by_policy: Option<ToolsUnavailableReason>,
    /// The Chrome picked for this session, if any.
    pub(crate) executable: Option<&'a Path>,
    /// The session's browser state folder, `<data>/run/<session id>/browser`.
    pub(crate) browser_dir: &'a Path,
    /// The session's claimed ports, socket folder and agent-browser name, if they were claimed.
    pub(crate) claims: Option<Claims<'a>>,
}

/// What a session's `browser::session::BrowserSession` fills: plain values, so the fill is
/// tested without claiming anything.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Claims<'a> {
    /// agent-browser's session name, `{session_id}`.
    pub(crate) name: &'a str,
    /// `{cdp_port}`.
    pub(crate) cdp_port: u16,
    /// `{stream_port}`.
    pub(crate) stream_port: u16,
    /// `{daemon_port}`, Windows only.
    pub(crate) daemon_port: Option<u16>,
    /// `{socket_dir}`, macOS and Linux only: the folder as made (`/tmp/...`), never its
    /// canonical form.
    pub(crate) socket_dir: Option<&'a Path>,
}

impl<'a> Claims<'a> {
    /// The values of `session`.
    pub(crate) fn of(session: &'a crate::browser::session::BrowserSession) -> Self {
        Self {
            name: &session.name,
            cdp_port: session.ports.cdp.port(),
            stream_port: session.ports.stream.port(),
            daemon_port: session
                .ports
                .daemon
                .as_ref()
                .map(clauseo_sandbox::ports::PortClaim::port),
            #[cfg(unix)]
            socket_dir: Some(session.socket_dir.path()),
            #[cfg(not(unix))]
            socket_dir: None,
        }
    }
}

/// The hook's servers for this session, tokens filled, and the browser's header state (none
/// when the hook offers no `browser`). Under [`McpControl::Managed`] no server goes at all.
pub(crate) fn hook_servers(
    hook: Vec<McpServer>,
    inputs: &BrowserInputs<'_>,
) -> (Vec<McpServer>, Option<ToolState>) {
    let mut servers = Vec::new();
    let mut state = None;
    for server in hook {
        let McpServer::Stdio { name, .. } = &server;
        if name != BROWSER {
            if inputs.control == McpControl::Clauseo {
                servers.push(server);
            }
            continue;
        }
        let availability = match browser_entry(server, inputs) {
            Ok(filled) => {
                servers.push(filled);
                ToolAvailability::Available
            }
            Err(reason) => ToolAvailability::Unavailable { reason },
        };
        state = Some(ToolState {
            tool: ClauseoTool::Browser,
            availability,
        });
    }
    (servers, state)
}

/// The `browser` entry with its tokens filled, or why it is left out.
fn browser_entry(
    server: McpServer,
    inputs: &BrowserInputs<'_>,
) -> Result<McpServer, ToolsUnavailableReason> {
    if !inputs.switch.on {
        return Err(inputs.switch.off_reason());
    }
    if inputs.control == McpControl::Managed {
        return Err(ToolsUnavailableReason::ManagedByFirm);
    }
    if !inputs.slot_free {
        return Err(ToolsUnavailableReason::TooManyBrowsers);
    }
    let wants_proxy = asks_for_proxy(&server);
    if wants_proxy && !inputs.proxy_set {
        return Err(ToolsUnavailableReason::NoProxy);
    }
    if let Some(reason) = inputs.blocked_by_policy {
        return Err(reason);
    }
    let executable = inputs
        .executable
        .ok_or(ToolsUnavailableReason::BrowserNotInstalled)?;
    let claims = inputs
        .claims
        .ok_or(ToolsUnavailableReason::BrowserNotInstalled)?;
    let port = if wants_proxy {
        let port = inputs.port.ok_or(ToolsUnavailableReason::NoProxy)?;
        Some(port.to_string())
    } else {
        None
    };
    let (Some(executable), Some(browser_dir)) = (
        executable.to_str(),
        dunce::simplified(inputs.browser_dir).to_str(),
    ) else {
        tracing::warn!(
            "the Chrome or the session's browser folder is not UTF-8: the browser is left out"
        );
        return Err(ToolsUnavailableReason::BrowserNotInstalled);
    };
    let cdp_port = claims.cdp_port.to_string();
    let stream_port = claims.stream_port.to_string();
    let daemon_port = claims.daemon_port.map(|port| port.to_string());
    let socket_dir = match claims.socket_dir.map(Path::to_str) {
        Some(Some(text)) => Some(text),
        Some(None) => {
            tracing::warn!("the browser's socket folder is not UTF-8: the browser is left out");
            return Err(ToolsUnavailableReason::BrowserNotInstalled);
        }
        None => None,
    };
    // A token whose value is absent here (`{daemon_port}` off Windows, `{socket_dir}` on
    // Windows) is unfillable if the entry holds it, which leaves the browser out below.
    let mut values = vec![
        (BROWSER_DIR, browser_dir),
        (SESSION_ID, claims.name),
        (BROWSER_EXECUTABLE, executable),
        (CDP_PORT, cdp_port.as_str()),
        (STREAM_PORT, stream_port.as_str()),
    ];
    values.extend(port.as_deref().map(|port| (PROXY_PORT, port)));
    values.extend(daemon_port.as_deref().map(|port| (DAEMON_PORT, port)));
    values.extend(socket_dir.map(|dir| (SOCKET_DIR, dir)));
    let fill = |text: &str| placeholders::fill(text, &IN_SERVERS, &values);
    let McpServer::Stdio {
        name,
        command,
        args,
        env,
    } = server;
    let filled = (|| -> Result<McpServer, placeholders::Unfilled> {
        Ok(McpServer::Stdio {
            name,
            command,
            args: args.iter().map(|arg| fill(arg)).collect::<Result<_, _>>()?,
            env: env
                .iter()
                .map(|(key, value)| Ok((key.clone(), fill(value)?)))
                .collect::<Result<_, _>>()?,
        })
    })();
    filled.map_err(|unfilled| {
        // A token the profile sends that this OS's claims do not fill: leave the browser out.
        tracing::error!(%unfilled, "the browser entry has a token the host cannot fill");
        ToolsUnavailableReason::BrowserNotInstalled
    })
}

/// The `{browser}` part of a session the profile offers the `browser` entry (Codex), from the
/// entry's header state ([`hook_servers`]): on when the entry goes; the not-installed sentence
/// when it was left out for want of a Chrome (`chrome_found` false); the no-browser sentence for
/// any other reason (off, the firm's policy or `managed-mcp.json`, no slot, no proxy, no claims).
pub(crate) fn entry_part(state: &ToolState, chrome_found: bool) -> BrowserPart {
    match state.availability {
        ToolAvailability::Available => BrowserPart::On,
        ToolAvailability::Unavailable {
            reason: ToolsUnavailableReason::BrowserNotInstalled,
        } if !chrome_found => BrowserPart::NotInstalled,
        ToolAvailability::Unavailable { .. } | ToolAvailability::Unknown { .. } => BrowserPart::Off,
    }
}

/// Whether `server` asks for the session's egress proxy: an argument or a variable holds
/// `{proxy_port}`. The plain client's Codex entry does not (Chrome goes direct, as Claude's shell
/// browser does); the parked Safe workspace's would.
pub(crate) fn asks_for_proxy(server: &McpServer) -> bool {
    let McpServer::Stdio { args, env, .. } = server;
    args.iter()
        .chain(env.iter().map(|(_, value)| value))
        .any(|text| text.contains(PROXY_PORT))
}

/// agent-browser's variables for a session with the browser as a shell tool, which the host
/// forces into the agent's environment (the hook is session-blind): the session's name
/// (`AGENT_BROWSER_SESSION`, the session's id), the session's config file (`AGENT_BROWSER_CONFIG`,
/// `{}`, so the lawyer's `~/.agent-browser/config.json` is never read), the session's own
/// downloads folder and Chrome profile (`AGENT_BROWSER_DOWNLOAD_PATH`, `AGENT_BROWSER_PROFILE`,
/// so never the lawyer's own profile), all three in its run folder's `agent-browser` folder,
/// the one place the data fences open to this session's agent-browser and to no other session's
/// (`agent::codex_fence`, `agent::data_fence`), and the Chrome the launch prep
/// found (`AGENT_BROWSER_EXECUTABLE_PATH`), and [`IDLE_TIMEOUT_MS`]. No proxy, stream port or
/// socket folder: the shell tool has none of the guard's claims.
pub(crate) fn shell_env(inputs: &ShellInputs<'_>) -> BTreeMap<OsString, OsString> {
    [
        ("AGENT_BROWSER_IDLE_TIMEOUT_MS", OsStr::new(IDLE_TIMEOUT_MS)),
        ("AGENT_BROWSER_SESSION", OsStr::new(inputs.session)),
        ("AGENT_BROWSER_CONFIG", inputs.config.as_os_str()),
        ("AGENT_BROWSER_DOWNLOAD_PATH", inputs.downloads.as_os_str()),
        ("AGENT_BROWSER_PROFILE", inputs.profile.as_os_str()),
        (
            "AGENT_BROWSER_EXECUTABLE_PATH",
            inputs.executable.as_os_str(),
        ),
    ]
    .into_iter()
    .map(|(name, value)| (OsString::from(name), value.to_owned()))
    .collect()
}

/// `AGENT_BROWSER_IDLE_TIMEOUT_MS`, in milliseconds: 10 minutes. agent-browser's daemon leaves
/// the agent's process tree and outlives the session; with no value it closes its Chrome after
/// an hour idle (`DEFAULT_IDLE_TIMEOUT_MS`, `cli/src/native/daemon.rs:165-173` at v0.41.1). An
/// explicit value is read as whole milliseconds (`resolve_idle_timeout`, :184-203), and it also
/// closes a headed browser, which the instructions forbid anyway. The host runs no
/// `agent-browser close` at the session's end: the timeout is the backstop, and the
/// instructions ask the agent to close the browser when done.
pub(crate) const IDLE_TIMEOUT_MS: &str = "600000";

/// What [`shell_env`] fills: plain values, so it is tested without a launch.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ShellInputs<'a> {
    /// The session's id (`SessionKey::id`).
    pub(crate) session: &'a str,
    /// `<data>/run/<session id>/agent-browser/config.json` (`browser::write_session_config`).
    pub(crate) config: &'a Path,
    /// `<data>/run/<session id>/agent-browser/downloads`.
    pub(crate) downloads: &'a Path,
    /// `<data>/run/<session id>/agent-browser/profile`.
    pub(crate) profile: &'a Path,
    /// The Chrome the launch prep found.
    pub(crate) executable: &'a Path,
}

/// What a session with the shell tool wired (`ProfileHook::browser_tool`) shows in its
/// header and is told in `{browser}`: available once its Chrome was found; else turned off
/// (`tools.browser`), left out by the firm's browser policy, or not installed.
pub(crate) fn shell_state(
    switch: Switch,
    blocked: Option<ToolsUnavailableReason>,
    found: bool,
) -> (ToolState, BrowserPart) {
    let (availability, part) = if found {
        (ToolAvailability::Available, BrowserPart::On)
    } else if !switch.on {
        let reason = switch.off_reason();
        (ToolAvailability::Unavailable { reason }, BrowserPart::Off)
    } else if let Some(reason) = blocked {
        (ToolAvailability::Unavailable { reason }, BrowserPart::Off)
    } else {
        let reason = ToolsUnavailableReason::BrowserNotInstalled;
        (
            ToolAvailability::Unavailable { reason },
            BrowserPart::NotInstalled,
        )
    };
    let state = ToolState {
        tool: ClauseoTool::Browser,
        availability,
    };
    (state, part)
}

/// The Chrome file names a support line may show (`browser::chrome`'s candidates); any other
/// reads `other`, since a Chrome set in the settings could be named anything.
const KNOWN_CHROMES: [&str; 7] = [
    "chrome.exe",
    "msedge.exe",
    "Google Chrome",
    "Google Chrome for Testing",
    "google-chrome",
    "chromium",
    "chrome",
];

/// What the support line for a session's `{browser}` shows (`profile_loads::session_loads`):
/// the part's word (`on`, `off`, `not_installed`) and the Chrome's file name, only when it
/// is one of [`KNOWN_CHROMES`] (`other` for any other, `none` without one). Never a path:
/// one holds the user's name.
pub(crate) fn part_log(
    part: BrowserPart,
    executable: Option<&Path>,
) -> (&'static str, &'static str) {
    let word = match part {
        BrowserPart::On => "on",
        BrowserPart::Off => "off",
        BrowserPart::NotInstalled => "not_installed",
    };
    let name = executable.map_or("none", |path| {
        let file = path.file_name().and_then(OsStr::to_str).unwrap_or_default();
        KNOWN_CHROMES
            .into_iter()
            .find(|known| *known == file)
            .unwrap_or("other")
    });
    (word, name)
}

#[cfg(test)]
mod shell_tests {
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
            };
            claude_options(&mut options, &keys);
            let allowed = names(&options, "allowedTools");
            let denied = names(&options, "disallowedTools");
            if browser {
                let want: Vec<String> = clauseo_profile::claude::agent_browser_allowed().collect();
                assert_eq!(allowed, want);
                let want: Vec<String> = clauseo_profile::claude::agent_browser_denied().collect();
                assert_eq!(denied, want);
            } else {
                assert_eq!((allowed, denied), (Vec::new(), Vec::new()));
            }
        }
    }
}
