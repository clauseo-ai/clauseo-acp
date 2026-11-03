//! The launch profile's host half (R21, R22): the browser entry, Claude's keys, Codex's keys
//! and the instructions' tokens.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_acp::McpServer;
use clauseo_protocol::{
    AgentKind, ClauseoTool, ToolAvailability, ToolState, ToolsUnavailableReason,
};
use serde_json::{Value, json};

use super::browser::{BrowserInputs, Switch, hook_servers};
use super::profile::{
    ClaudeKeys, Folders, Login, claude_options, fill_instructions, fill_meta, web_search_state,
};
use super::{GuardHook, McpControl, session_meta};
use crate::browser::seed_sites::SEED_SITES;

const ON: Switch = Switch {
    on: true,
    locked: false,
};
const OFF: Switch = Switch {
    on: false,
    locked: false,
};
const LOCKED_OFF: Switch = Switch {
    on: false,
    locked: true,
};

/// The `browser` entry exactly as `clauseo_profile::servers::browser` builds it.
fn browser_server() -> McpServer {
    McpServer::Stdio {
        name: "browser".to_owned(),
        command: PathBuf::from("/app/clauseo-mcp"),
        args: [
            "browser-guard",
            "--agent-browser",
            "/app/agent-browser",
            "--cwd",
            "{browser_dir}",
            "--",
            "mcp",
            "--tools",
            "core",
        ]
        .map(str::to_owned)
        .to_vec(),
        env: vec![
            (
                "AGENT_BROWSER_CONFIG".to_owned(),
                "/data/browser/agent-browser.json".to_owned(),
            ),
            (
                "AGENT_BROWSER_PROXY".to_owned(),
                "http://127.0.0.1:{proxy_port}".to_owned(),
            ),
            (
                "AGENT_BROWSER_SESSION".to_owned(),
                "{session_id}".to_owned(),
            ),
            (
                "AGENT_BROWSER_EXECUTABLE_PATH".to_owned(),
                "{browser_executable}".to_owned(),
            ),
        ],
    }
}

fn other_server() -> McpServer {
    McpServer::Stdio {
        name: "firm-docs".to_owned(),
        command: PathBuf::from("/opt/firm-docs"),
        args: vec!["{session_id}".to_owned()],
        env: Vec::new(),
    }
}

fn inputs<'a>(browser_dir: &'a Path, chrome: Option<&'a Path>) -> BrowserInputs<'a> {
    BrowserInputs {
        switch: ON,
        control: McpControl::Clauseo,
        slot_free: true,
        port: Some(8123),
        executable: chrome,
        browser_dir,
        session_id: "t-1",
    }
}

fn reason(state: Option<ToolState>) -> Option<ToolsUnavailableReason> {
    match state?.availability {
        ToolAvailability::Unavailable { reason } => Some(reason),
        ToolAvailability::Available | ToolAvailability::Unknown { .. } => None,
    }
}

#[test]
fn the_browser_entry_is_filled_per_session() {
    let dir = PathBuf::from("/data/run/t-1/browser");
    let chrome = PathBuf::from("/Applications/Chrome");
    let (servers, state) = hook_servers(
        vec![browser_server(), other_server()],
        &inputs(&dir, Some(&chrome)),
    );
    assert_eq!(
        state,
        Some(ToolState {
            tool: ClauseoTool::Browser,
            availability: ToolAvailability::Available,
        })
    );
    let McpServer::Stdio { args, env, .. } = &servers[0];
    assert_eq!(args[4], dir.to_str().unwrap());
    assert_eq!(
        env.iter()
            .map(|(_, value)| value.as_str())
            .collect::<Vec<_>>(),
        [
            "/data/browser/agent-browser.json",
            "http://127.0.0.1:8123",
            "t-1",
            "/Applications/Chrome"
        ]
    );
    // Another hook server goes as it came: only the browser's tokens are the host's.
    assert_eq!(servers[1], other_server());
}

#[test]
fn a_folder_named_like_a_token_stays_literal() {
    let dir = PathBuf::from("/data/{session_id}/{proxy_port}/browser");
    let chrome = PathBuf::from("/chrome/{browser_dir}");
    let (servers, _) = hook_servers(vec![browser_server()], &inputs(&dir, Some(&chrome)));
    let McpServer::Stdio { args, env, .. } = &servers[0];
    assert_eq!(args[4], "/data/{session_id}/{proxy_port}/browser");
    assert_eq!(env[3].1, "/chrome/{browser_dir}");
    let folders = Folders {
        work: "/w/{matter_notes}",
        notes: "/n/{work_folder}",
    };
    assert_eq!(
        fill_instructions("{work_folder} and {matter_notes}", folders).unwrap(),
        "/w/{matter_notes} and /n/{work_folder}"
    );
}

#[test]
fn each_reason_drops_the_browser_in_order() {
    let dir = PathBuf::from("/b");
    let chrome = PathBuf::from("/chrome");
    let base = inputs(&dir, Some(&chrome));
    let cases = [
        (
            BrowserInputs {
                switch: OFF,
                ..base
            },
            ToolsUnavailableReason::Off,
        ),
        (
            BrowserInputs {
                switch: LOCKED_OFF,
                control: McpControl::Managed,
                ..base
            },
            ToolsUnavailableReason::LockedOff,
        ),
        (
            BrowserInputs {
                control: McpControl::Managed,
                port: None,
                ..base
            },
            ToolsUnavailableReason::ManagedByFirm,
        ),
        (
            BrowserInputs {
                slot_free: false,
                port: None,
                ..base
            },
            ToolsUnavailableReason::TooManyBrowsers,
        ),
        (
            BrowserInputs {
                port: None,
                executable: None,
                ..base
            },
            ToolsUnavailableReason::NoProxy,
        ),
        (
            BrowserInputs {
                executable: None,
                ..base
            },
            ToolsUnavailableReason::BrowserNotInstalled,
        ),
    ];
    for (case, expected) in cases {
        let (servers, state) = hook_servers(vec![browser_server()], &case);
        assert_eq!(servers.len(), 0, "{expected:?}");
        assert_eq!(reason(state), Some(expected));
    }
}

#[test]
fn managed_mcp_sends_no_hook_server_at_all() {
    let dir = PathBuf::from("/b");
    let base = inputs(&dir, None);
    let managed = BrowserInputs {
        control: McpControl::Managed,
        ..base
    };
    let (servers, _) = hook_servers(vec![other_server(), browser_server()], &managed);
    assert_eq!(servers.len(), 0);
    let (servers, state) = hook_servers(vec![other_server()], &base);
    assert_eq!(servers, [other_server()]);
    assert_eq!(state, None, "no browser offered, no browser state");
}

fn folders() -> Folders<'static> {
    Folders {
        work: "/data/run/t-1/work",
        notes: "/data/agent-state/m/notes",
    }
}

fn keys(web_search: Switch, login: Login) -> ClaudeKeys<'static> {
    ClaudeKeys {
        web_search,
        login,
        folders: folders(),
        seed_sites: SEED_SITES,
    }
}

fn options(hook: &Value, keys: &ClaudeKeys<'_>) -> Value {
    let mut options = hook.as_object().cloned().unwrap_or_default();
    claude_options(&mut options, keys);
    Value::Object(options)
}

fn strings(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// R21's "Permissions": the profile's allow list, then `WebSearch`, then one rule per seed site.
#[test]
fn the_allow_list_is_exactly_the_permissions_list() {
    let hook = json!({"allowedTools": [
        "mcp__browser",
        "Bash(stetdoc view:*)",
        "Bash(stetdoc inspect:*)",
        "Bash(stetdoc check:*)",
        "Bash(stetdoc lookup:*)",
        "Bash(stetdoc capabilities:*)",
    ]});
    let options = options(&hook, &keys(ON, Login::FirstParty));
    let mut expected = vec![
        "mcp__browser".to_owned(),
        "Bash(stetdoc view:*)".to_owned(),
        "Bash(stetdoc inspect:*)".to_owned(),
        "Bash(stetdoc check:*)".to_owned(),
        "Bash(stetdoc lookup:*)".to_owned(),
        "Bash(stetdoc capabilities:*)".to_owned(),
        "WebSearch".to_owned(),
    ];
    expected.extend(
        SEED_SITES
            .iter()
            .map(|site| format!("WebFetch(domain:{site})")),
    );
    assert_eq!(strings(&options["allowedTools"]), expected);
    assert!(strings(&options["allowedTools"]).contains(&"WebFetch(domain:sci.gov.in)"));
    assert_eq!(options.get("disallowedTools"), None);
}

#[test]
fn web_search_off_disallows_both_tools_whatever_the_hook_sent() {
    let hook = json!({
        "allowedTools": ["mcp__browser", "WebSearch", "WebFetch", "WebFetch(domain:x.in)"],
        "disallowedTools": ["Task"],
    });
    let options = options(&hook, &keys(LOCKED_OFF, Login::FirstParty));
    assert_eq!(
        strings(&options["disallowedTools"]),
        ["Task", "WebSearch", "WebFetch"]
    );
    assert_eq!(strings(&options["allowedTools"]), ["mcp__browser"]);
}

#[test]
fn a_bedrock_login_keeps_webfetch_and_skips_the_preflight() {
    let options = options(&json!({}), &keys(ON, Login::Bedrock));
    let allowed = strings(&options["allowedTools"]);
    assert!(!allowed.contains(&"WebSearch"));
    assert!(allowed.contains(&"WebFetch(domain:indiacode.nic.in)"));
    assert_eq!(options["settings"]["skipWebFetchPreflight"], json!(true));
    assert_eq!(options.get("disallowedTools"), None);
    let other = self::options(&json!({}), &keys(ON, Login::OtherCloud));
    assert_eq!(other["settings"]["skipWebFetchPreflight"], json!(true));
    assert!(strings(&other["allowedTools"]).contains(&"WebSearch"));
    let first = self::options(&json!({}), &keys(ON, Login::FirstParty));
    assert_eq!(first["settings"].get("skipWebFetchPreflight"), None);
}

/// R22 `skill_shell_off` and `writable_folders`, at the merge.
#[test]
fn skill_shell_is_off_and_the_folders_come_after_the_hooks() {
    let hook = json!({
        "additionalDirectories": ["/firm/precedents"],
        "settings": {"disableSkillShellExecution": false, "keepMe": 1},
    });
    let options = options(&hook, &keys(ON, Login::Bedrock));
    assert_eq!(
        strings(&options["additionalDirectories"]),
        [
            "/firm/precedents",
            "/data/run/t-1/work",
            "/data/agent-state/m/notes"
        ]
    );
    assert_eq!(
        options["settings"],
        json!({"disableSkillShellExecution": true, "keepMe": 1, "skipWebFetchPreflight": true})
    );
}

/// The guard's entry and `disableAllHooks: false` survive beside the profile's settings, and
/// the forced MCP keys stay.
#[test]
fn the_guard_and_the_forced_keys_survive_the_profile_keys() {
    let hook = json!({"claudeCode": {"options": {
        "settings": {"disableSkillShellExecution": true},
        "strictMcpConfig": false,
        "mcpServers": {"x": {}},
    }}});
    let guard = GuardHook {
        program: Path::new("/app/clauseo"),
        matter: Path::new("/matters/a"),
    };
    let keys = keys(OFF, Login::Bedrock);
    let meta = Value::Object(session_meta(
        hook.as_object().cloned(),
        McpControl::Clauseo,
        Some(guard),
        Some(&keys),
    ));
    let options = &meta["claudeCode"]["options"];
    assert_eq!(options["strictMcpConfig"], json!(true));
    assert_eq!(options["settingSources"], json!(["user"]));
    assert_eq!(
        options["env"]["ENABLE_CLAUDEAI_MCP_SERVERS"],
        json!("false")
    );
    assert_eq!(options.get("mcpServers"), None);
    let settings = &options["settings"];
    assert_eq!(settings["disableAllHooks"], json!(false));
    assert_eq!(settings["disableSkillShellExecution"], json!(true));
    assert_eq!(settings["skipWebFetchPreflight"], json!(true));
    let entry = &settings["hooks"]["PreToolUse"][0];
    assert_eq!(entry["hooks"][0]["args"][1], json!("--matter"));
    assert_eq!(entry["hooks"][0]["args"][2], json!("/matters/a"));
}

/// No kind, no Claude keys: only the switches every agent gets.
#[test]
fn an_agent_with_no_kind_gets_no_claude_keys() {
    let meta = Value::Object(session_meta(None, McpControl::Clauseo, None, None));
    let options = &meta["claudeCode"]["options"];
    for key in [
        "allowedTools",
        "disallowedTools",
        "additionalDirectories",
        "settings",
    ] {
        assert_eq!(options.get(key), None, "{key}");
    }
    assert_eq!(web_search_state(None, ON, Login::FirstParty), None);
}

#[test]
fn the_instructions_are_filled_and_escaped_for_json() {
    let mut meta = json!({"systemPrompt": {"append": "Work in `{work_folder}`; brief at {matter_notes}/matter-brief.md."}})
        .as_object()
        .cloned()
        .unwrap();
    let odd = Folders {
        work: "/data/run/a \"b\"\n\\c/work",
        notes: "/data/agent-state/m/notes",
    };
    fill_meta(&mut meta, odd).unwrap();
    let text = Value::Object(meta).to_string();
    let back: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        back["systemPrompt"]["append"],
        json!(
            "Work in `/data/run/a \"b\"\n\\c/work`; brief at /data/agent-state/m/notes/matter-brief.md."
        )
    );
    assert!(!text.contains("{work_folder}") && !text.contains("{matter_notes}"));
}

#[test]
fn the_login_follows_the_merged_launch_env() {
    let none = BTreeMap::<OsString, OsString>::new();
    let settings = BTreeMap::from([("claude_code_use_vertex".to_owned(), "1".to_owned())]);
    assert_eq!(Login::of(&none, &settings, |_| None), Login::OtherCloud);
    let hook = BTreeMap::from([(
        OsString::from("CLAUDE_CODE_USE_VERTEX"),
        OsString::from("0"),
    )]);
    assert_eq!(Login::of(&hook, &settings, |_| None), Login::FirstParty);
    assert_eq!(
        Login::of(&none, &BTreeMap::new(), |name| (name
            == "CLAUDE_CODE_USE_BEDROCK")
            .then(|| OsString::from("true"))),
        Login::Bedrock
    );
    assert_eq!(
        Login::of(&none, &BTreeMap::new(), |name| (name
            == "CLAUDE_CODE_USE_FOUNDRY")
            .then(OsString::new)),
        Login::FirstParty
    );
}

#[test]
fn the_web_search_state_per_kind_and_login() {
    let state = |kind, switch, login| reason(web_search_state(Some(kind), switch, login));
    assert_eq!(state(AgentKind::Claude, ON, Login::FirstParty), None);
    assert_eq!(
        state(AgentKind::Claude, OFF, Login::FirstParty),
        Some(ToolsUnavailableReason::Off)
    );
    assert_eq!(
        state(AgentKind::Codex, LOCKED_OFF, Login::FirstParty),
        Some(ToolsUnavailableReason::LockedOff)
    );
    assert_eq!(
        state(AgentKind::Claude, ON, Login::Bedrock),
        Some(ToolsUnavailableReason::NoSearchOnThisLogin)
    );
    assert_eq!(state(AgentKind::Codex, ON, Login::Bedrock), None);
}
