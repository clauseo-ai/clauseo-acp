//! The `case-law` entry's fill: the relay's port and token file, the reasons it is left out, its
//! place after the other servers, `SystemRoot` on Windows, and Claude's pre-allow rule names.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_acp::McpServer;
use clauseo_protocol::{ClauseoTool, ToolAvailability, ToolState, ToolsUnavailableReason};

use super::{CaseLawInputs, Relay, hook_servers, is_case_law_rule, system_root};
use crate::agent::loads::McpControl;

fn server(name: &str, args: &[&str], env: &[(&str, &str)]) -> McpServer {
    McpServer::Stdio {
        name: name.to_owned(),
        command: PathBuf::from("/app/clauseo-mcp"),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        env: env
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
    }
}

/// The hook's servers as the profile offers them: `browser`, then `case-law`.
fn hook() -> Vec<McpServer> {
    vec![
        server("browser", &["browser-guard"], &[]),
        server(
            "case-law",
            &["connector", "case-law"],
            &[
                ("CLAUSEO_RELAY", "127.0.0.1:{relay_port}"),
                ("CLAUSEO_RELAY_TOKEN_FILE", "{relay_token_file}"),
            ],
        ),
    ]
}

fn token_file() -> PathBuf {
    std::env::temp_dir()
        .join("run")
        .join("s-1")
        .join("relay-token")
}

fn inputs(relay: Relay<'_>) -> CaseLawInputs<'_> {
    CaseLawInputs {
        relay,
        control: McpControl::Clauseo,
        system_root: None,
    }
}

fn open(file: &Path) -> Relay<'_> {
    Relay::Open {
        port: 61_234,
        token_file: file,
    }
}

fn state(reason: Option<ToolsUnavailableReason>) -> ToolState {
    ToolState {
        tool: ClauseoTool::CaseLaw,
        availability: reason.map_or(ToolAvailability::Available, |reason| {
            ToolAvailability::Unavailable { reason }
        }),
    }
}

#[test]
fn an_open_relay_fills_the_entry_with_its_port_and_token_file() {
    let file = token_file();
    let (rest, entry, shown) = hook_servers(hook(), &inputs(open(&file)));
    assert_eq!(rest, vec![server("browser", &["browser-guard"], &[])]);
    let path = dunce::simplified(&file).to_str().unwrap().to_owned();
    assert_eq!(
        entry,
        Some(server(
            "case-law",
            &["connector", "case-law"],
            &[
                ("CLAUSEO_RELAY", "127.0.0.1:61234"),
                ("CLAUSEO_RELAY_TOKEN_FILE", &path),
            ],
        ))
    );
    assert_eq!(shown, Some(state(None)));
}

#[test]
fn no_entry_without_an_open_relay_and_the_header_says_why_once_added() {
    let file = token_file();
    let (rest, entry, shown) = hook_servers(hook(), &inputs(Relay::Absent));
    assert_eq!((rest.len(), entry, shown), (1, None, None));
    for reason in [
        ToolsUnavailableReason::Off,
        ToolsUnavailableReason::LockedOff,
        ToolsUnavailableReason::ManagedByFirm,
        ToolsUnavailableReason::NoRelay,
    ] {
        let (rest, entry, shown) = hook_servers(hook(), &inputs(Relay::Unavailable(reason)));
        assert_eq!(
            (rest.len(), entry, shown),
            (1, None, Some(state(Some(reason))))
        );
    }
    // The firm's managed-mcp.json appeared after the relay opened: no entry all the same.
    let managed = CaseLawInputs {
        control: McpControl::Managed,
        ..inputs(open(&file))
    };
    let (_, entry, shown) = hook_servers(hook(), &managed);
    assert_eq!(entry, None);
    assert_eq!(
        shown,
        Some(state(Some(ToolsUnavailableReason::ManagedByFirm)))
    );
    // A hook with no `case-law` entry: nothing, whatever the relay.
    let browser_only = vec![server("browser", &[], &[])];
    let (rest, entry, shown) = hook_servers(browser_only, &inputs(open(&file)));
    assert_eq!((rest.len(), entry, shown), (1, None, None));
}

#[test]
fn a_token_the_host_cannot_fill_leaves_case_law_out() {
    let file = token_file();
    let odd = vec![server("case-law", &["{socket_dir}"], &[])];
    let (_, entry, shown) = hook_servers(odd, &inputs(open(&file)));
    assert_eq!(entry, None);
    assert_eq!(shown, Some(state(Some(ToolsUnavailableReason::NoRelay))));
}

/// Winsock needs `SystemRoot`: on Windows the entry carries the host's, so the connector's
/// launch keeps it whatever the agent's MCP client passes on.
#[test]
fn on_windows_the_entry_keeps_system_root() {
    let inherited = || {
        vec![
            (
                OsString::from("PATH"),
                OsString::from(r"C:\Windows\system32"),
            ),
            (OsString::from("SYSTEMROOT"), OsString::from(r"D:\Win")),
        ]
    };
    assert_eq!(system_root(true, inherited()).as_deref(), Some(r"D:\Win"));
    assert_eq!(system_root(false, inherited()), None);
    assert_eq!(system_root(true, Vec::new()), None);
    let file = token_file();
    let windows = CaseLawInputs {
        system_root: Some(r"D:\Win"),
        ..inputs(open(&file))
    };
    let (_, entry, _) = hook_servers(hook(), &windows);
    let Some(McpServer::Stdio { env, .. }) = entry else {
        panic!("no entry");
    };
    assert_eq!(
        env.last(),
        Some(&("SystemRoot".to_owned(), r"D:\Win".to_owned()))
    );
    // An entry that names it already keeps its own.
    let own = vec![server("case-law", &[], &[("systemroot", r"E:\Own")])];
    let (_, entry, _) = hook_servers(own, &windows);
    let Some(McpServer::Stdio { env, .. }) = entry else {
        panic!("no entry");
    };
    assert_eq!(env, [("systemroot".to_owned(), r"E:\Own".to_owned())]);
}

#[test]
fn only_rules_for_the_case_law_server_are_case_law_rules() {
    for rule in [
        "mcp__case-law",
        "mcp__case-law__search_law",
        "mcp__case-law__*",
    ] {
        assert!(is_case_law_rule(rule), "{rule}");
    }
    for rule in [
        "mcp__case-law-firm",
        "mcp__case-laws__x",
        "mcp__browser",
        "Bash",
    ] {
        assert!(!is_case_law_rule(rule), "{rule}");
    }
}

/// Claude's pre-allow (decision D7): `mcp__case-law` only with our entry; a hook's rule for a
/// `case-law` server never goes without it, so a firm's own server of that name (under its
/// `managed-mcp.json`, where our entry never goes) inherits nothing. Forced ask: no
/// `allowedTools` at all.
#[test]
fn claude_pre_allows_case_law_only_with_our_entry() {
    use serde_json::{Value, json};

    use crate::agent::loads::browser::Switch;
    use crate::agent::loads::profile::{ClaudeKeys, Folders, Login, claude_options};
    let allowed = |case_law: bool, forced_ask: bool| {
        let mut options = json!({"allowedTools": [
            "mcp__browser", "mcp__case-law", "mcp__case-law__search_law", "mcp__case-law-firm"
        ]})
        .as_object()
        .cloned()
        .unwrap();
        let keys = ClaudeKeys {
            web_search: Switch {
                on: false,
                locked: false,
            },
            login: Login::FirstParty,
            folders: Folders {
                work: "/w",
                notes: "/n",
                connectors: "",
            },
            seed_sites: &[],
            forced_ask,
            data: &crate::agent::data_fence::LaunchDeny::default(),
            case_law,
        };
        claude_options(&mut options, &keys);
        options.get("allowedTools").cloned()
    };
    assert_eq!(
        allowed(true, false),
        Some(json!([
            "mcp__browser",
            "mcp__case-law-firm",
            "mcp__case-law"
        ]))
    );
    assert_eq!(
        allowed(false, false),
        Some(json!(["mcp__browser", "mcp__case-law-firm"]))
    );
    assert_eq!(allowed(true, true), None::<Value>);
}

/// A caller of `Folders::of` that never says whether the session got case law fills
/// `{connectors}` with the no-database sentence, never with nothing.
#[test]
fn folders_start_with_the_no_case_law_sentence() {
    use crate::agent::loads::profile::{Folders, fill_instructions};
    let (work, notes) = (PathBuf::from("/w"), PathBuf::from("/n"));
    let folders = Folders::of(&work, &notes).unwrap();
    assert_eq!(
        folders.connectors,
        clauseo_profile::instructions::connectors_part(false)
    );
    let filled = fill_instructions("Tools.{connectors}", folders).unwrap();
    assert!(filled.contains("no case-law database"), "{filled}");
}

/// Claude's tools may not read or edit any session's relay token file, its own included: only
/// `clauseo-mcp` reads it, and a token read through Claude's tools would land in the transcript.
#[test]
fn claude_is_denied_every_relay_token_file() {
    use crate::agent::data_fence::{LaunchDeny, claude_deny_rules};
    let data = std::env::temp_dir().join("clauseo-data");
    let rules = claude_deny_rules(&LaunchDeny::fixed(vec![data]));
    for tool in ["Read", "Edit"] {
        assert!(
            rules
                .iter()
                .any(|rule| rule.starts_with(&format!("{tool}("))
                    && rule.ends_with("/run/*/relay-token)")),
            "{tool}: {rules:?}"
        );
    }
}
