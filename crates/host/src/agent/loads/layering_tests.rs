//! The layering of what the launch profile and the host give a session: Claude's `_meta` (the
//! hook, then the host's switches, then the guard, each key the host sets replaced whole).
//! Codex's `CODEX_CONFIG` keys are in `codex_layering_tests.rs`.

use std::path::Path;

use serde_json::{Value, json};

use super::browser::Switch;
use super::profile::{ClaudeKeys, Folders, Login};
use super::{GuardHook, McpControl, guard_missing, session_meta};
use crate::browser::seed_sites::SEED_SITES;

pub(super) const ON: Switch = Switch {
    on: true,
    locked: false,
};
pub(super) const OFF: Switch = Switch {
    on: false,
    locked: false,
};

pub(super) fn folders() -> Folders<'static> {
    Folders {
        work: "/data/run/t-1/work",
        notes: "/data/agent-state/m/notes",
        connectors: "",
        browser: "",
    }
}

/// The hook's `_meta` goes first, the host's switches over it, then the guard's keys: a hook
/// that sets a switch's key (`strictMcpConfig`, `settingSources`, the connector variable,
/// `disableSkillShellExecution`) or the guard's (`disableAllHooks`) loses each, and each is
/// replaced whole, not merged with the hook's value. The guard's entry still goes after the
/// hook's own `PreToolUse` entries (the host's rule, `loads/tests.rs`).
#[test]
fn a_hook_meta_setting_a_switch_or_the_guards_key_loses_both() {
    let hook = json!({"claudeCode": {"options": {
        "strictMcpConfig": false,
        "settingSources": ["user", "project", "local"],
        "env": {"ENABLE_CLAUDEAI_MCP_SERVERS": "true", "KEEP": "1"},
        "settings": {
            "disableAllHooks": true,
            "disableSkillShellExecution": false,
            "hooks": {"PreToolUse": [{"matcher": "Read", "hooks": []}]},
        },
    }}});
    let program = Path::new("/app/clauseo");
    let matter = Path::new("/m");
    let guard = Some(GuardHook {
        program,
        matter,
        ask_all: false,
        protected: &[],
        protected_edit: &[],
        home: None,
    });
    let keys = ClaudeKeys {
        web_search: ON,
        login: Login::FirstParty,
        folders: folders(),
        seed_sites: SEED_SITES,
        forced_ask: false,
        data: &crate::agent::data_fence::LaunchDeny::default(),
        case_law: false,
        browser: false,
        sandbox: None,
        matter: None,
        trusted_servers: &[],
    };
    let meta = session_meta(
        hook.as_object().cloned(),
        McpControl::Clauseo,
        guard,
        Some(&keys),
    );
    let options = &meta["claudeCode"]["options"];
    assert_eq!(options["strictMcpConfig"], json!(true));
    assert_eq!(options["settingSources"], json!(["user"]));
    assert_eq!(
        options["env"]["ENABLE_CLAUDEAI_MCP_SERVERS"],
        json!("false")
    );
    assert_eq!(options["env"]["KEEP"], json!("1"));
    assert_eq!(options["settings"]["disableAllHooks"], json!(false));
    assert_eq!(
        options["settings"]["disableSkillShellExecution"],
        json!(true)
    );
    let entries = options["settings"]["hooks"]["PreToolUse"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["matcher"], json!("Read"));
    assert_eq!(entries[1]["matcher"], json!(super::GUARD_MATCHER));
}

/// Forced ask (`agent/forced_ask.rs`): no `allowedTools` at all, not the host's web rules and
/// not the hook's own; the guard runs before every tool (`*`, MCP tools included) with
/// `--ask-all` last. Without it the same inputs keep both as before.
#[test]
fn forced_ask_drops_every_allow_rule_and_hooks_every_tool() {
    let hook = json!({"claudeCode": {"options": {"allowedTools": ["Bash(git status)"]}}});
    let meta_for = |forced_ask: bool| {
        let guard = Some(GuardHook {
            program: Path::new("/app/clauseo"),
            matter: Path::new("/m"),
            ask_all: forced_ask,
            protected: &[],
            protected_edit: &[],
            home: None,
        });
        let keys = ClaudeKeys {
            web_search: ON,
            login: Login::FirstParty,
            folders: folders(),
            seed_sites: SEED_SITES,
            forced_ask,
            data: &crate::agent::data_fence::LaunchDeny::default(),
            case_law: false,
            browser: false,
            sandbox: None,
            matter: None,
            trusted_servers: &[],
        };
        session_meta(
            hook.as_object().cloned(),
            McpControl::Clauseo,
            guard,
            Some(&keys),
        )
    };
    let forced = meta_for(true);
    let options = &forced["claudeCode"]["options"];
    assert_eq!(options.get("allowedTools"), None, "{options}");
    // The second fence, in the flag settings: the ask rules and no sandbox auto-allow.
    assert_eq!(
        options["settings"]["permissions"]["ask"],
        json!(crate::agent::forced_ask::ASK_RULES),
        "{options}"
    );
    assert_eq!(
        options["settings"]["sandbox"]["autoAllowBashIfSandboxed"],
        json!(false)
    );
    let guard = &options["settings"]["hooks"]["PreToolUse"][0];
    assert_eq!(guard["matcher"], json!("*"));
    assert_eq!(
        guard["hooks"][0]["args"],
        json!(["--pre-write-guard", "--matter", "/m", "--ask-all"])
    );

    let plain = meta_for(false);
    let options = &plain["claudeCode"]["options"];
    // On Windows the one rule is the whole Bash tool (`agent/one_shell.rs`).
    let bash_only = json!({"deny": ["Bash"]});
    assert_eq!(
        options["settings"].get("permissions"),
        cfg!(windows).then(|| &bash_only),
        "{options}"
    );
    assert_eq!(options["settings"].get("sandbox"), None, "{options}");
    let allowed = options["allowedTools"].as_array().unwrap();
    assert!(allowed.contains(&json!("Bash(git status)")), "{options}");
    assert!(allowed.contains(&json!("WebSearch")), "{options}");
    let guard = &options["settings"]["hooks"]["PreToolUse"][0];
    assert_eq!(guard["matcher"], json!(super::GUARD_MATCHER));
    assert_eq!(
        guard["hooks"][0]["args"],
        json!(["--pre-write-guard", "--matter", "/m"])
    );
}

/// Forced ask keeps the hook's own `permissions` and `sandbox` keys and its ask rules, and adds
/// every forced-ask rule once, after them.
#[test]
fn forced_ask_adds_its_ask_rules_after_the_hooks() {
    let hook = json!({"claudeCode": {"options": {"settings": {
        "permissions": {"ask": ["Bash", "Read(./secret)"], "allow": ["Read"]},
        "sandbox": {"enabled": true, "autoAllowBashIfSandboxed": true},
    }}}});
    let keys = ClaudeKeys {
        web_search: OFF,
        login: Login::FirstParty,
        folders: folders(),
        seed_sites: SEED_SITES,
        forced_ask: true,
        data: &crate::agent::data_fence::LaunchDeny::default(),
        case_law: false,
        browser: false,
        sandbox: None,
        matter: None,
        trusted_servers: &[],
    };
    let meta = session_meta(
        hook.as_object().cloned(),
        McpControl::Clauseo,
        None,
        Some(&keys),
    );
    let settings = &meta["claudeCode"]["options"]["settings"];
    let mut expected = vec![json!("Bash"), json!("Read(./secret)")];
    expected.extend(
        crate::agent::forced_ask::ASK_RULES
            .iter()
            .filter(|rule| **rule != "Bash")
            .map(|rule| json!(rule)),
    );
    assert_eq!(settings["permissions"]["ask"], Value::Array(expected));
    assert_eq!(settings["permissions"]["allow"], json!(["Read"]));
    assert_eq!(
        settings["sandbox"],
        json!({"enabled": true, "autoAllowBashIfSandboxed": false})
    );
}

/// Every Claude session, forced ask or not, gets the data folder's deny rules in its flag
/// settings, after the hook's own; a hook that empties `permissions.deny` or sets another type
/// there loses, and the run folder and the notes are never denied. User settings cannot take
/// them out: flag settings outrank them and Claude Code merges every source's deny rules.
#[test]
fn every_claude_session_denies_the_data_folders_clauseo_only_subtrees() {
    let data = crate::agent::data_fence::LaunchDeny::fixed(vec![std::path::PathBuf::from("/data")]);
    let meta_for = |hook: &Value, forced_ask: bool| {
        let keys = ClaudeKeys {
            web_search: ON,
            login: Login::FirstParty,
            folders: folders(),
            seed_sites: SEED_SITES,
            forced_ask,
            data: &data,
            case_law: false,
            browser: false,
            sandbox: None,
            matter: None,
            trusted_servers: &[],
        };
        let guard = Some(GuardHook {
            program: Path::new("/app/clauseo"),
            matter: Path::new("/m"),
            ask_all: forced_ask,
            protected: &[],
            protected_edit: &[],
            home: None,
        });
        let meta = session_meta(
            hook.as_object().cloned(),
            McpControl::Clauseo,
            guard,
            Some(&keys),
        );
        meta["claudeCode"]["options"]["settings"]["permissions"]["deny"].clone()
    };
    let own =
        json!({"claudeCode": {"options": {"settings": {"permissions": {"deny": ["Bash(rm:*)"]}}}}});
    for (hook, forced_ask) in [
        (json!({}), false),
        (json!({}), true),
        (
            json!({"claudeCode": {"options": {"settings": {"permissions": {"deny": []}}}}}),
            false,
        ),
        (
            json!({"claudeCode": {"options": {"settings": {"permissions": "none"}}}}),
            false,
        ),
        (own.clone(), true),
    ] {
        let deny = meta_for(&hook, forced_ask);
        let rules: Vec<&str> = deny
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(
            rules.contains(&"Read(//data/skill-packs/**)"),
            "{hook}: {rules:?}"
        );
        assert!(
            rules.contains(&"Edit(//data/skill-packs/**)"),
            "{hook}: {rules:?}"
        );
        assert!(
            rules.contains(&"Read(//data/agents/codex-acp/**)"),
            "{hook}: {rules:?}"
        );
        assert!(
            rules.contains(&"Read(//data/settings.json)"),
            "{hook}: {rules:?}"
        );
        assert!(
            !rules.iter().any(|rule| {
                (rule.contains("/run/")
                    && !rule.ends_with("/run/*/relay-token)")
                    && !rule.ends_with("/run/*/proxy-secret)")
                    && !rule.ends_with("/run/*/agent-browser/config.json)"))
                    || rule.contains("/agent-state")
            }),
            "{rules:?}"
        );
    }
    assert_eq!(meta_for(&own, false)[0], json!("Bash(rm:*)"));
}

/// A forced-ask launch is refused when no guard hook could go to the agent
/// (`actor/turn/profile_loads.rs`; the review's `rev_forced_ask_without_a_guard_still_fences`
/// found a forced-ask `_meta` built with no guard carrying no hook). Each way there is no hook
/// has its own reason kind, and a guard that can go has none.
#[test]
fn a_guard_that_cannot_go_has_a_reason() {
    let program = Path::new("/app/clauseo");
    let matter = Path::new("/m");
    let guard = |program, matter| {
        Some(GuardHook {
            program,
            matter,
            ask_all: true,
            protected: &[],
            protected_edit: &[],
            home: None,
        })
    };
    assert_eq!(guard_missing(None), Some("no_program"));
    assert_eq!(guard_missing(guard(program, matter)), None);
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let bad = Path::new(std::ffi::OsStr::from_bytes(b"/m\xff"));
        assert_eq!(guard_missing(guard(bad, matter)), Some("program_not_utf8"));
        assert_eq!(guard_missing(guard(program, bad)), Some("matter_not_utf8"));
        assert_eq!(super::guard_entry(guard(program, bad).unwrap()), None);
    }
}

/// Whether Claude rule `rule` for `tool` covers `path` (a `//` form path), near enough for
/// these rules: a pattern ending `/**` covers what is under its folder, one ending `*` what
/// starts with the rest, any other only itself.
fn covers(rule: &str, tool: &str, path: &str) -> bool {
    let Some(pattern) = rule
        .strip_prefix(tool)
        .and_then(|rest| rest.strip_prefix('('))
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return false;
    };
    if let Some(folder) = pattern.strip_suffix("/**") {
        path.starts_with(&format!("{folder}/"))
    } else if let Some(start) = pattern.strip_suffix('*') {
        path.starts_with(start)
    } else {
        pattern == path
    }
}

/// A Claude session's flag settings allow a Read of Clauseo's built-in skill assets, for each
/// spelling of the data folder, after the hook's own allow rules, so a built-in skill never asks
/// the lawyer; no deny rule covers a Read of them (an `Edit` is denied), and no allow covers
/// anything else in the data folder but a Read of the agents' commands and Clauseo's Python
/// runtime (`data_fence::READ_ONLY`).
#[test]
fn claude_may_read_the_built_in_skills_without_a_prompt_and_nothing_else_in_data() {
    let spellings = ["/data", "/link/data"];
    let data = crate::agent::data_fence::LaunchDeny::fixed(
        spellings.iter().map(std::path::PathBuf::from).collect(),
    );
    let keys = ClaudeKeys {
        web_search: ON,
        login: Login::FirstParty,
        folders: folders(),
        seed_sites: SEED_SITES,
        forced_ask: true,
        data: &data,
        case_law: false,
        browser: false,
        sandbox: None,
        matter: None,
        trusted_servers: &[],
    };
    let hook = json!({"claudeCode": {"options": {"settings": {"permissions": {
        "allow": ["Bash(ls:*)"]}}}}});
    let meta = session_meta(
        hook.as_object().cloned(),
        McpControl::Clauseo,
        None,
        Some(&keys),
    );
    let permissions = &meta["claudeCode"]["options"]["settings"]["permissions"];
    let strings = |key: &str| -> Vec<String> {
        let list = permissions[key].as_array().cloned().unwrap_or_default();
        list.iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    let (allow, deny) = (strings("allow"), strings("deny"));
    let want: Vec<String> = std::iter::once("Bash(ls:*)".to_owned())
        .chain(spellings.iter().flat_map(|data| {
            [
                format!("Read(/{data}/profile/claude-plugin/skills/**)"),
                format!("Read(/{data}/tools/**)"),
                format!("Read(/{data}/agents/runtime/clauseo-python-*/**)"),
            ]
        }))
        .collect();
    assert_eq!(allow, want);
    for data in spellings {
        let asset =
            format!("/{data}/profile/claude-plugin/skills/legal-drafting/section-checklists.md");
        let denied: Vec<&String> = deny.iter().filter(|r| covers(r, "Read", &asset)).collect();
        assert!(denied.is_empty(), "Read {asset}: {denied:?}");
        // Changing it is denied: the whole built-in plugin is `Edit` denied.
        assert!(
            deny.iter().any(|r| covers(r, "Edit", &asset)),
            "Edit {asset}"
        );
        assert!(allow.iter().any(|rule| covers(rule, "Read", &asset)));
        for elsewhere in [
            format!("/{data}/profile/claude-plugin/.claude-plugin/plugin.json"),
            format!("/{data}/profile/claude-plugin/skills"),
            format!("/{data}/profile/x"),
            format!("/{data}/skill-packs/x/SKILL.md"),
            format!("/{data}/run/other/work/a.txt"),
            format!("/{data}/search.sqlite"),
        ] {
            for tool in ["Read", "Edit", "Write"] {
                let allowed: Vec<&String> = allow
                    .iter()
                    .filter(|r| covers(r, tool, &elsewhere))
                    .collect();
                assert!(allowed.is_empty(), "{tool} {elsewhere}: {allowed:?}");
            }
        }
        let asset_edit = allow.iter().any(|rule| covers(rule, "Edit", &asset));
        assert!(!asset_edit && !allow.iter().any(|r| r.starts_with("Write(")));
    }
}

/// A session outside forced ask (in `auto`, or its `acceptEdits` fallback) keeps every
/// guard that does not depend on asking: the data folder's deny rules (deny wins in every mode),
/// the pre-write guard's hook before each write and shell command, bypass off, and no
/// `permission-mode` flag a hook could slip in. Its auto-edit scope beyond the matter folder is
/// exactly the work folder and the matter's notes, as Codex's writable roots.
#[test]
fn an_accept_edits_session_keeps_the_deny_rules_the_guard_and_its_two_folders() {
    assert_eq!(super::CLAUDE_FALLBACK_MODE, "acceptEdits");
    let data = crate::agent::data_fence::LaunchDeny::fixed(vec![std::path::PathBuf::from("/data")]);
    let keys = ClaudeKeys {
        web_search: ON,
        login: Login::FirstParty,
        folders: folders(),
        seed_sites: SEED_SITES,
        forced_ask: false,
        data: &data,
        case_law: false,
        browser: false,
        sandbox: None,
        matter: None,
        trusted_servers: &[],
    };
    let guard = Some(GuardHook {
        program: Path::new("/app/clauseo"),
        matter: Path::new("/m"),
        ask_all: false,
        protected: &[],
        protected_edit: &[],
        home: None,
    });
    let hook = json!({"claudeCode": {"options": {"extraArgs": {"permission-mode": "default"}}}});
    let meta = session_meta(
        hook.as_object().cloned(),
        McpControl::Clauseo,
        guard,
        Some(&keys),
    );
    let options = &meta["claudeCode"]["options"];
    let deny: Vec<&str> = options["settings"]["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for rule in [
        "Read(//data/skill-packs/**)",
        "Edit(//data/skill-packs/**)",
        "Edit(//data/settings.json)",
    ] {
        assert!(deny.contains(&rule), "{rule}: {deny:?}");
    }
    let entry = &options["settings"]["hooks"]["PreToolUse"][0];
    assert_eq!(entry["matcher"], json!(super::GUARD_MATCHER));
    assert_eq!(
        entry["hooks"][0]["args"],
        json!(["--pre-write-guard", "--matter", "/m"])
    );
    assert_eq!(options["settings"]["disableAllHooks"], json!(false));
    assert_eq!(options[super::ALLOW_BYPASS_OPTION], json!(false));
    assert_eq!(options["extraArgs"], json!({}), "{options}");
    assert_eq!(
        options["additionalDirectories"],
        json!(["/data/run/t-1/work", "/data/agent-state/m/notes"])
    );
}
