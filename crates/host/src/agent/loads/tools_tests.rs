//! The built-in tools a Claude launch offers and denies, per system, and the variables every
//! Claude launch sets (`tools.rs`, `loads.rs`). A pin bump that adds a tool changes nothing here
//! until someone decides; these lists say what was decided.

use std::collections::BTreeMap;
use std::ffi::OsString;

use serde_json::{Value, json};

use super::profile::{ClaudeBrowser, ClaudeKeys, Folders, Login};
use super::profile_tests::{OFF, ON};
use super::tools::{NOT_OFFERED, claude_tools};
use super::{
    CLAUDE_CRON_OFF_VAR, CLAUDE_GIT_STATUS_OFF_VAR, GIT_STATUS_SETTING, McpControl,
    claude_launch_vars, session_meta,
};

/// What macOS and Linux offer, web or not; Windows offers the same without `Bash`.
const EVERY_SYSTEM: [&str; 19] = [
    "Read",
    "Edit",
    "Write",
    "NotebookEdit",
    "Bash",
    "Agent",
    "SendMessage",
    "TaskStop",
    "Skill",
    "EnterPlanMode",
    "ExitPlanMode",
    "AskUserQuestion",
    "ToolSearch",
    "ReportFindings",
    "TodoWrite",
    "TaskCreate",
    "TaskGet",
    "TaskList",
    "TaskUpdate",
];

static NO_DENY: crate::agent::data_fence::LaunchDeny = crate::agent::data_fence::LaunchDeny {
    spellings: Vec::new(),
    targets: Vec::new(),
    kept: Vec::new(),
    own_reads: Vec::new(),
};

fn keys(web_search: super::browser::Switch) -> ClaudeKeys<'static> {
    ClaudeKeys {
        web_search,
        login: Login::FirstParty,
        folders: Folders {
            work: "/data/run/t-1/work",
            notes: "/data/agent-state/m/notes",
            connectors: "",
            browser: "",
        },
        seed_sites: &[],
        forced_ask: false,
        data: &NO_DENY,
        case_law: false,
        browser: ClaudeBrowser::None,
        sandbox: None,
        matter: None,
        trusted_servers: &[],
    }
}

fn strings(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn options(hook: Value, web: super::browser::Switch) -> Value {
    let meta = session_meta(
        hook.as_object().cloned(),
        McpControl::Clauseo,
        None,
        Some(&keys(web)),
    );
    meta["claudeCode"]["options"].clone()
}

/// macOS and Linux: exactly these, and the web's two only with web search on.
#[test]
fn macos_and_linux_offer_exactly_the_decided_tools() {
    assert_eq!(claude_tools(false, false), EVERY_SYSTEM);
    let mut web = EVERY_SYSTEM.to_vec();
    web.extend(["WebFetch", "WebSearch"]);
    assert_eq!(claude_tools(false, true), web);
}

/// Windows: the same without `Bash`, plus its shell and its search tools.
#[test]
fn windows_offers_exactly_the_decided_tools() {
    let mut windows = EVERY_SYSTEM.to_vec();
    windows.retain(|name| *name != "Bash");
    windows.extend(["PowerShell", "Glob", "Grep"]);
    assert_eq!(claude_tools(true, false), windows);
    windows.extend(["WebFetch", "WebSearch"]);
    assert_eq!(claude_tools(true, true), windows);
}

/// Windows' one shell is `PowerShell`: its list names it and never `Bash`, web search on or off;
/// macOS's and Linux's keep `Bash` and name no `PowerShell`.
#[test]
fn windows_offers_powershell_and_no_bash() {
    for web in [false, true] {
        let windows = claude_tools(true, web);
        assert!(
            windows.contains(&"PowerShell") && !windows.contains(&"Bash"),
            "{windows:?}"
        );
        let unix = claude_tools(false, web);
        assert!(
            unix.contains(&"Bash") && !unix.contains(&"PowerShell"),
            "{unix:?}"
        );
    }
}

/// The tools left out, every one named in `disallowedTools` on every launch, web or not.
#[test]
fn the_tools_left_out_are_denied_by_name() {
    assert_eq!(
        NOT_OFFERED,
        [
            "CronCreate",
            "CronDelete",
            "CronList",
            "ScheduleWakeup",
            "EnterWorktree",
            "ExitWorktree",
            "Workflow",
            "DesignSync",
            "PushNotification",
            "Monitor",
            "ListAgents",
            "ListPeers",
        ]
    );
    for web in [ON, OFF] {
        let options = options(json!({}), web);
        let disallowed = strings(&options["disallowedTools"]);
        let offered = strings(&options["tools"]);
        assert_eq!(offered, claude_tools(cfg!(windows), web.on));
        for name in NOT_OFFERED {
            assert!(disallowed.contains(&name), "{name} is not denied");
            assert!(!offered.contains(&name), "{name} is offered");
        }
        let allowed = strings(&options["allowedTools"]);
        assert!(!allowed.iter().any(|rule| NOT_OFFERED.contains(rule)));
    }
}

/// The scheduler's tools, and the listing of other agents, are neither offered nor allowed.
#[test]
fn the_launch_keeps_claude_codes_scheduler_and_agent_listing_off() {
    let hook = json!({"allowedTools": ["CronCreate", "ScheduleWakeup", "ListAgents", "Read"]});
    let options = options(hook, OFF);
    for name in [
        "CronCreate",
        "CronDelete",
        "CronList",
        "ScheduleWakeup",
        "ListAgents",
    ] {
        assert!(
            strings(&options["disallowedTools"]).contains(&name),
            "{name}"
        );
        assert!(!strings(&options["tools"]).contains(&name), "{name}");
    }
}

/// A hook's own `tools` (the whole preset, or another list) never reaches Claude Code; the
/// host's list replaces it, and the hook's own denials stay, first.
#[test]
fn the_hook_cannot_widen_the_tool_list() {
    for given in [
        json!({"type": "preset", "preset": "claude_code"}),
        json!(["Read", "CronCreate", "Workflow"]),
    ] {
        let hook = json!({"tools": given, "disallowedTools": ["NotebookEdit"]});
        let options = options(hook, ON);
        assert_eq!(
            strings(&options["tools"]),
            claude_tools(cfg!(windows), true)
        );
        assert_eq!(strings(&options["disallowedTools"])[0], "NotebookEdit");
    }
}

/// Every Claude launch keeps Claude Code's scheduler off: the variable is `1` in the env the
/// adapter spawns Claude Code with and in the flag settings' `env`, over a hook's own value.
#[test]
fn every_claude_launch_keeps_the_scheduler_off() {
    for hook in [
        json!({}),
        json!({"claudeCode": {"options": {
            "env": {"CLAUDE_CODE_DISABLE_CRON": "0"},
            "settings": {"env": {"CLAUDE_CODE_DISABLE_CRON": "0"}},
        }}}),
    ] {
        let meta = Value::Object(session_meta(
            hook.as_object().cloned(),
            McpControl::Clauseo,
            None,
            Some(&keys(OFF)),
        ));
        let options = &meta["claudeCode"]["options"];
        assert_eq!(options["env"][CLAUDE_CRON_OFF_VAR], "1", "{hook}");
        assert_eq!(
            options["settings"]["env"][CLAUDE_CRON_OFF_VAR], "1",
            "{hook}"
        );
    }
    // Under the firm's managed MCP too.
    let meta = session_meta(None, McpControl::Managed, None, Some(&keys(ON)));
    assert_eq!(
        meta["claudeCode"]["options"]["env"][CLAUDE_CRON_OFF_VAR],
        "1"
    );
    // And in the adapter's own environment, which the launch forces over any other value.
    let vars = claude_launch_vars();
    assert_eq!(
        vars.get(&OsString::from("CLAUDE_CODE_DISABLE_CRON")),
        Some(&OsString::from("1"))
    );
}

/// Every Claude launch leaves the repository status out of the conversation: the variable is
/// `1` in the adapter's environment, in the env it spawns Claude Code with and in the flag
/// settings' `env`, and the flag setting is `false`, over a hook's own values.
#[test]
fn the_launch_does_not_add_repository_status_to_the_conversation() {
    for hook in [
        json!({}),
        json!({"claudeCode": {"options": {
            "env": {"CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS": "0"},
            "settings": {
                "env": {"CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS": "0"},
                "includeGitInstructions": true,
            },
        }}}),
    ] {
        let meta = Value::Object(session_meta(
            hook.as_object().cloned(),
            McpControl::Clauseo,
            None,
            Some(&keys(ON)),
        ));
        let options = &meta["claudeCode"]["options"];
        assert_eq!(options["env"][CLAUDE_GIT_STATUS_OFF_VAR], "1", "{hook}");
        let settings = &options["settings"];
        assert_eq!(settings["env"][CLAUDE_GIT_STATUS_OFF_VAR], "1", "{hook}");
        assert_eq!(settings[GIT_STATUS_SETTING], json!(false), "{hook}");
    }
    assert_eq!(GIT_STATUS_SETTING, "includeGitInstructions");
    assert_eq!(
        claude_launch_vars(),
        BTreeMap::from([
            (
                OsString::from("CLAUDE_CODE_DISABLE_CRON"),
                OsString::from("1")
            ),
            (
                OsString::from("CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS"),
                OsString::from("1")
            ),
        ])
    );
}
