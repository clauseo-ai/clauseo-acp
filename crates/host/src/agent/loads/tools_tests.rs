//! The built-in tools a Claude launch offers and denies, per system, and the variables every
//! Claude launch sets (`tools.rs`, `loads.rs`). A pin bump that adds a tool changes nothing here
//! until someone decides; these lists say what was decided.

use std::ffi::OsString;

use serde_json::{Value, json};

use super::profile::{ClaudeBrowser, ClaudeKeys, Folders, Login};
use super::profile_tests::{OFF, ON};
use super::tools::{NOT_OFFERED, claude_tools};
use super::{
    CLAUDE_ADVISOR_OFF_VAR, CLAUDE_CODE_MODE_VAR, CLAUDE_CRON_OFF_VAR, CLAUDE_GIT_STATUS_OFF_VAR,
    GIT_STATUS_SETTING, McpControl, claude_launch_vars, session_meta,
};

/// What macOS and Linux offer, web or not; Windows offers the same without `Bash`. `Glob` and
/// `Grep` on every system: the instructions send listings, searches and reads to them.
const EVERY_SYSTEM: [&str; 23] = [
    "Read",
    "Glob",
    "Grep",
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
    "Yield",
    "REPL",
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

/// The launch's `claudeCode.options` over a hook that sent `hook` as its `claudeCode.options`.
fn options(hook: &Value, web: super::browser::Switch) -> Value {
    let given = json!({"claudeCode": {"options": hook}});
    let meta = session_meta(
        given.as_object().cloned(),
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

/// Windows: the same without `Bash`, plus its shell.
#[test]
fn windows_offers_exactly_the_decided_tools() {
    let mut windows = EVERY_SYSTEM.to_vec();
    windows.retain(|name| *name != "Bash");
    windows.push("PowerShell");
    assert_eq!(claude_tools(true, false), windows);
    windows.extend(["WebFetch", "WebSearch"]);
    assert_eq!(claude_tools(true, true), windows);
}

/// `Glob`, `Grep` and `Read` on every system, web or not: Claude's instructions send every
/// listing, search and read of the matter to them (`clauseo_profile::instructions::CLAUDE_FILES`),
/// and Claude Code 2.1.330 offers `Glob` and `Grep` on macOS and Linux only when `tools` names
/// them.
#[test]
fn every_system_offers_glob_grep_and_read() {
    for windows in [false, true] {
        for web in [false, true] {
            let tools = claude_tools(windows, web);
            for name in ["Glob", "Grep", "Read"] {
                assert_eq!(
                    tools.iter().filter(|t| **t == name).count(),
                    1,
                    "{name}: {tools:?}"
                );
            }
        }
    }
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
            "RemoteTrigger",
            "EnterWorktree",
            "ExitWorktree",
            "Workflow",
            "LSP",
            "DesignSync",
            "PushNotification",
            "SendUserMessage",
            "Brief",
            "Monitor",
            "ListAgents",
            "ListPeers",
        ]
    );
    for web in [ON, OFF] {
        let options = options(&json!({}), web);
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
    let options = options(&hook, OFF);
    for name in [
        "CronCreate",
        "CronDelete",
        "CronList",
        "ScheduleWakeup",
        "RemoteTrigger",
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
        let options = options(&hook, ON);
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

/// Every Claude launch sets its variable in the adapter's environment, in the env it spawns
/// Claude Code with and in the flag settings' `env`, and its flag setting, over a hook's own
/// values.
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
        claude_launch_vars().get(&OsString::from("CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS")),
        Some(&OsString::from("1"))
    );
}

/// Every Claude launch sets these launch variables in the adapter's environment, in the env it
/// spawns Claude Code with and in the flag settings' `env`, and these flag settings, over a
/// hook's own values.
#[test]
fn every_claude_launch_keeps_messaging_between_sessions_and_remote_control_off() {
    let vars = [
        ("CLAUDE_CODE_HARBOR_KITE", "0"),
        ("CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL", "1"),
    ];
    for hook in [
        json!({}),
        json!({"claudeCode": {"options": {
            "env": {"CLAUDE_CODE_HARBOR_KITE": "1"},
            "settings": {
                "env": {"CLAUDE_CODE_HARBOR_KITE": "1"},
                "crossSessionInbound": "accept",
                "disableRemoteControl": false,
            },
        }}}),
    ] {
        for control in [McpControl::Clauseo, McpControl::Managed] {
            let meta = Value::Object(session_meta(
                hook.as_object().cloned(),
                control,
                None,
                Some(&keys(OFF)),
            ));
            let options = &meta["claudeCode"]["options"];
            for (name, value) in vars {
                assert_eq!(options["env"][name], value, "{hook} {name}");
                assert_eq!(options["settings"]["env"][name], value, "{hook} {name}");
            }
            assert_eq!(
                options["settings"]["crossSessionInbound"], "refuse",
                "{hook}"
            );
            assert_eq!(options["settings"]["disableRemoteControl"], true, "{hook}");
        }
    }
    let launch = claude_launch_vars();
    for (name, value) in vars {
        assert_eq!(
            launch.get(&OsString::from(name)),
            Some(&OsString::from(value))
        );
    }
    assert_eq!(launch.len(), 9);
}

/// Every Claude launch keeps Claude Code's experimental `advisor` tool off: its variable is `1`
/// in the adapter's environment, in the env it spawns Claude Code with and in the flag
/// settings' `env`, over a hook's own value, and a hook's `advisorModel` cannot bring the tool
/// back, since Claude Code checks the variable first.
#[test]
fn every_claude_launch_keeps_the_advisor_tool_off() {
    assert_eq!(CLAUDE_ADVISOR_OFF_VAR, "CLAUDE_CODE_DISABLE_ADVISOR_TOOL");
    for hook in [
        json!({}),
        json!({"claudeCode": {"options": {
            "env": {"CLAUDE_CODE_DISABLE_ADVISOR_TOOL": "0"},
            "settings": {
                "env": {"CLAUDE_CODE_DISABLE_ADVISOR_TOOL": ""},
                "advisorModel": "opus",
            },
        }}}),
    ] {
        for control in [McpControl::Clauseo, McpControl::Managed] {
            let meta = Value::Object(session_meta(
                hook.as_object().cloned(),
                control,
                None,
                Some(&keys(ON)),
            ));
            let options = &meta["claudeCode"]["options"];
            assert_eq!(options["env"][CLAUDE_ADVISOR_OFF_VAR], "1", "{hook}");
            assert_eq!(
                options["settings"]["env"][CLAUDE_ADVISOR_OFF_VAR], "1",
                "{hook}"
            );
        }
    }
    assert_eq!(
        claude_launch_vars().get(&OsString::from("CLAUDE_CODE_DISABLE_ADVISOR_TOOL")),
        Some(&OsString::from("1"))
    );
}

/// The variables that keep helpers off the lead's model: Claude Code's own agent types, its
/// web-fetch agent and its fork off and at most six helpers at once on every login, and every
/// helper on Haiku on Anthropic's own login only. Each wins over a hook's own value; a cloud
/// login gets neither helper-model variable, and the adapter's own environment never has them.
#[test]
fn helpers_stay_off_the_lead_model_and_only_anthropics_login_forces_haiku() {
    let every_login = [
        ("CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS", "1"),
        ("CLAUDE_CODE_WEB_FETCH_AGENT", "0"),
        ("CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS", "6"),
        ("CLAUDE_CODE_FORK_SUBAGENT", "0"),
    ];
    let first_party = [
        ("CLAUDE_CODE_SUBAGENT_MODEL", "haiku"),
        ("CLAUDE_CODE_SUBAGENT_MODEL_FORCE", "1"),
    ];
    let against = json!({"claudeCode": {"options": {
        "env": {
            "CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS": "0",
            "CLAUDE_CODE_WEB_FETCH_AGENT": "1",
            "CLAUDE_CODE_FORK_SUBAGENT": "1",
            "CLAUDE_CODE_SUBAGENT_MODEL": "opus",
        },
        "settings": {"env": {
            "CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS": "20",
            "CLAUDE_CODE_SUBAGENT_MODEL_FORCE": "0",
            "CLAUDE_CODE_FORK_SUBAGENT": "1",
        }},
    }}});
    for control in [McpControl::Clauseo, McpControl::Managed] {
        for hook in [json!({}), against.clone()] {
            let keys = ClaudeKeys {
                login: Login::FirstParty,
                ..keys(ON)
            };
            let meta = Value::Object(session_meta(
                hook.as_object().cloned(),
                control,
                None,
                Some(&keys),
            ));
            let options = &meta["claudeCode"]["options"];
            for (name, value) in every_login.iter().chain(&first_party) {
                assert_eq!(options["env"][name], *value, "{hook} {name}");
                assert_eq!(options["settings"]["env"][name], *value, "{hook} {name}");
            }
        }
        for login in [Login::Bedrock, Login::OtherCloud] {
            let keys = ClaudeKeys { login, ..keys(ON) };
            let meta = Value::Object(session_meta(None, control, None, Some(&keys)));
            let options = &meta["claudeCode"]["options"];
            for (name, value) in every_login {
                assert_eq!(options["env"][name], value, "{login:?} {name}");
                assert_eq!(options["settings"]["env"][name], value, "{login:?} {name}");
            }
            for (name, _) in first_party {
                assert_eq!(options["env"].get(name), None, "{login:?} {name}");
                assert_eq!(
                    options["settings"]["env"].get(name),
                    None,
                    "{login:?} {name}"
                );
            }
        }
    }
    let launch = claude_launch_vars();
    for (name, value) in every_login {
        assert_eq!(
            launch.get(&OsString::from(name)),
            Some(&OsString::from(value))
        );
    }
    for (name, _) in first_party {
        assert_eq!(launch.get(&OsString::from(name)), None, "{name}");
    }
}

/// Every Claude launch asks for Code Mode in the flag settings' `env`, the one place an SDK
/// session reads it, over a hook's `0`, and offers `REPL`, which it must not leave out.
#[test]
fn every_claude_launch_asks_for_code_mode_in_its_settings() {
    for hook in [
        json!({}),
        json!({"claudeCode": {"options": {"settings": {"env": {"CLAUDE_CODE_EXPERIMENTAL_CODE_MODE": "0"}}}}}),
    ] {
        for control in [McpControl::Clauseo, McpControl::Managed] {
            let meta = Value::Object(session_meta(
                hook.as_object().cloned(),
                control,
                None,
                Some(&keys(OFF)),
            ));
            let options = &meta["claudeCode"]["options"];
            assert_eq!(
                options["settings"]["env"][CLAUDE_CODE_MODE_VAR], "1",
                "{hook}"
            );
            assert_eq!(options["env"].get(CLAUDE_CODE_MODE_VAR), None, "{hook}");
            assert!(strings(&options["tools"]).contains(&"REPL"), "{hook}");
            assert!(!strings(&options["disallowedTools"]).contains(&"REPL"));
        }
    }
    // Not a Claude agent: no settings of Claude's at all.
    let meta = Value::Object(session_meta(None, McpControl::Clauseo, None, None));
    assert_eq!(meta["claudeCode"]["options"].get("settings"), None);
}

/// `SendMessage` stays offered (it continues a helper); `ListAgents` is neither offered nor
/// allowed to run: not in `tools`, and named in `disallowedTools`.
#[test]
fn send_message_stays_and_list_agents_is_left_out() {
    let options = options(&json!({"allowedTools": ["ListAgents", "ListPeers"]}), ON);
    let offered = strings(&options["tools"]);
    assert!(offered.contains(&"SendMessage") && offered.contains(&"Agent"));
    for name in ["ListAgents", "ListPeers"] {
        assert!(!offered.contains(&name));
        assert!(
            strings(&options["disallowedTools"]).contains(&name),
            "{name}"
        );
    }
}

/// A hook's extra flags cannot set the tools or the flag settings beside the launch's own keys;
/// its other extra flags stay.
#[test]
fn extra_flags_cannot_set_the_tools_or_the_settings() {
    let hook = json!({"claudeCode": {"options": {"extraArgs": {
        "tools": "default",
        "allowedTools=CronCreate": null,
        "disallowed-tools": "",
        "settings": "{}",
        "debug": null,
    }}}});
    let meta = session_meta(
        hook.as_object().cloned(),
        McpControl::Clauseo,
        None,
        Some(&keys(OFF)),
    );
    assert_eq!(
        meta["claudeCode"]["options"]["extraArgs"],
        json!({"debug": null})
    );
}
