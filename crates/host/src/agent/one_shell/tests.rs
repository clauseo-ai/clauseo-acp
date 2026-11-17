//! Windows: Claude's one shell tool is PowerShell (`agent/one_shell.rs`). The Windows rules are
//! pure functions of the OS, so they run here on every OS; what `cfg!(windows)` picks is checked
//! through `session_meta` on each lane.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use clauseo_settings::AgentLaunch;
use serde_json::{Map, Value, json};

use super::{BASH_RULE, GIT_BASH_VAR, ON, POWERSHELL_TOOL_VAR, apply_env, deny_bash, meta_env};
use crate::agent::loads::browser::Switch;
use crate::agent::loads::profile::{ClaudeKeys, Folders, Login};
use crate::agent::loads::{GuardHook, McpControl, session_meta};
use crate::agent::shell_path::Os;

/// A matter folder named the way lawyers name them: spaces, an apostrophe, an ampersand and a
/// non-ASCII letter.
const MATTER: &str = r"C:\Users\ada\Documents\O'Brien & Sons - Sharma v State 2024 (Müller)";

/// A Claude launch's merged environment as `agent::launch_env` makes it, with the variables set
/// to other values every way they can come: inherited, in the agent's settings entry (any case)
/// and from the hook.
fn merged() -> BTreeMap<OsString, OsString> {
    let agent = AgentLaunch {
        command: "claude-agent-acp".to_owned(),
        args: Vec::new(),
        env: BTreeMap::from([(
            "claude_code_use_powershell_tool".to_owned(),
            "off".to_owned(),
        )]),
    };
    let hook = BTreeMap::from([(
        OsString::from(GIT_BASH_VAR),
        OsString::from(r"C:\Program Files\Git\bin\bash.exe"),
    )]);
    let inherited = [
        (POWERSHELL_TOOL_VAR, "0"),
        ("Path", r"C:\Windows\system32;C:\Program Files\Git\cmd"),
        ("SystemRoot", r"C:\Windows"),
    ]
    .map(|(name, value)| (OsString::from(name), OsString::from(value)));
    crate::agent::launch_env(
        &agent,
        hook,
        BTreeMap::new(),
        &[],
        inherited.into_iter(),
        &[],
    )
}

/// The names of `env` equal to `name` in any case.
fn named<'a>(env: &'a BTreeMap<OsString, OsString>, name: &str) -> Vec<&'a OsString> {
    env.iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value)
        .collect()
}

/// The Windows launch offers one shell: whatever the inherited environment, the agent's settings
/// entry or the hook set, the variable is Clauseo's `1`, once, and no Git Bash path is left. The
/// rest of the environment, `Path` included, is as the merge made it (no folder is put first:
/// Claude Code picks its PowerShell itself, `BG()`).
#[test]
fn a_windows_launch_turns_powershell_on_over_every_other_value() {
    let before = merged();
    assert_eq!(named(&before, POWERSHELL_TOOL_VAR).len(), 1, "{before:?}");
    assert_eq!(named(&before, GIT_BASH_VAR).len(), 1);
    let mut env = before.clone();
    apply_env(&mut env, Os::Windows);
    assert_eq!(named(&env, POWERSHELL_TOOL_VAR), [&OsString::from(ON)]);
    assert!(env.contains_key(&OsString::from(POWERSHELL_TOOL_VAR)));
    assert!(named(&env, GIT_BASH_VAR).is_empty(), "{env:?}");
    let mut rest = before;
    rest.retain(|name, _| {
        !name.eq_ignore_ascii_case(POWERSHELL_TOOL_VAR) && !name.eq_ignore_ascii_case(GIT_BASH_VAR)
    });
    env.remove(&OsString::from(POWERSHELL_TOOL_VAR));
    assert_eq!(env, rest);
}

/// macOS and Linux launches are left exactly as the merge made them.
#[test]
fn a_mac_or_linux_launch_is_unchanged() {
    for os in [Os::Mac, Os::Linux] {
        let mut env = merged();
        apply_env(&mut env, os);
        assert_eq!(env, merged(), "{os:?}");
    }
}

/// The `_meta` and flag settings `env` objects on Windows: Clauseo's `1` over a hook's value in
/// any case, and no Git Bash path; off Windows nothing changes.
#[test]
fn the_meta_env_holds_clauseos_value_on_windows_only() {
    let hook =
        json!({"claude_code_use_powershell_tool": "0", GIT_BASH_VAR: "C:\\bash.exe", "K": 1});
    let mut env = hook.as_object().cloned().unwrap();
    meta_env(&mut env, true);
    assert_eq!(Value::Object(env), json!({POWERSHELL_TOOL_VAR: ON, "K": 1}));
    let mut env = hook.as_object().cloned().unwrap();
    meta_env(&mut env, false);
    assert_eq!(Value::Object(env), hook);
}

/// On Windows the flag settings deny the whole Bash tool once, after the rules already there,
/// replacing a value of another type; off Windows nothing changes.
#[test]
fn the_bash_tool_is_denied_on_windows_only() {
    let mut settings = Map::new();
    settings.insert(
        "permissions".to_owned(),
        json!({"deny": ["Read(x)"], "allow": ["Bash"]}),
    );
    deny_bash(&mut settings, true);
    deny_bash(&mut settings, true);
    assert_eq!(
        settings["permissions"],
        json!({"deny": ["Read(x)", BASH_RULE], "allow": ["Bash"]})
    );
    let mut odd = Map::new();
    odd.insert("permissions".to_owned(), json!({"deny": "Bash(rm:*)"}));
    deny_bash(&mut odd, true);
    assert_eq!(odd["permissions"], json!({"deny": [BASH_RULE]}));
    let mut off = Map::new();
    deny_bash(&mut off, false);
    assert!(off.is_empty());
}

static NO_DENY: crate::agent::data_fence::LaunchDeny = crate::agent::data_fence::LaunchDeny {
    spellings: Vec::new(),
    targets: Vec::new(),
    kept: Vec::new(),
    own_reads: Vec::new(),
};

/// A Claude session's `_meta`, the hook setting other values for both variables and a `Bash`
/// allow in every place `_meta` has, with the guard (forced ask or not).
fn meta(forced_ask: bool) -> Value {
    let hook = json!({"claudeCode": {"options": {
        "env": {POWERSHELL_TOOL_VAR: "0", GIT_BASH_VAR: "C:\\bash.exe"},
        "settings": {
            "env": {"Claude_Code_Use_PowerShell_Tool": "false"},
            "permissions": {"allow": ["Bash"], "deny": ["Read(x)"]},
        },
    }}});
    let keys = ClaudeKeys {
        web_search: Switch {
            on: false,
            locked: false,
        },
        login: Login::FirstParty,
        folders: Folders {
            work: "C:/data/run/t-1/work",
            notes: "C:/data/agent-state/m/notes",
            connectors: "",
            browser: "",
        },
        seed_sites: crate::browser::seed_sites::SEED_SITES,
        forced_ask,
        data: &NO_DENY,
        case_law: false,
        browser: false,
        sandbox: None,
        matter: Some(Path::new(MATTER)),
        trusted_servers: &[],
    };
    let guard = GuardHook {
        program: Path::new(r"C:\Program Files\Clauseo\clauseo.exe"),
        matter: Path::new(MATTER),
        ask_all: forced_ask,
    };
    let meta = session_meta(
        hook.as_object().cloned(),
        McpControl::Clauseo,
        Some(guard),
        Some(&keys),
    );
    Value::Object(meta)
}

/// One shell on Windows: Claude Code's `sBr` (2.1.293) turns PowerShell off with any Bash deny
/// rule where Git Bash exists, unless `bS()` reads the variable as on; so the Bash deny and the
/// variable at `1` must both be there, in both `env` routes, whatever the hook sent. Off Windows
/// neither is added and the hook's values pass as they did.
#[test]
fn a_windows_session_denies_bash_and_turns_powershell_on_together() {
    for forced_ask in [false, true] {
        let meta = meta(forced_ask);
        let options = &meta["claudeCode"]["options"];
        let settings = &options["settings"];
        let deny = settings["permissions"]["deny"].as_array().unwrap();
        assert!(deny.contains(&json!("Read(x)")), "{deny:?}");
        if cfg!(windows) {
            assert!(deny.contains(&json!(BASH_RULE)), "{deny:?}");
            assert_eq!(options["env"][POWERSHELL_TOOL_VAR], json!(ON));
            assert_eq!(options["env"].get(GIT_BASH_VAR), None);
            assert_eq!(settings["env"][POWERSHELL_TOOL_VAR], json!(ON));
            assert_eq!(settings["env"].get("Claude_Code_Use_PowerShell_Tool"), None);
        } else {
            assert!(!deny.contains(&json!(BASH_RULE)), "{deny:?}");
            assert_eq!(options["env"][POWERSHELL_TOOL_VAR], json!("0"));
            assert_eq!(options["env"][GIT_BASH_VAR], json!("C:\\bash.exe"));
            assert_eq!(
                settings["env"]["Claude_Code_Use_PowerShell_Tool"],
                json!("false")
            );
        }
    }
}

/// Every hook Clauseo writes for Claude is exec form: a `command` with `args`, which Claude Code
/// spawns with no shell. A hook without `args` would run in the machine's default shell,
/// PowerShell or Git Bash depending on what is installed, so the same guard would behave
/// differently from laptop to laptop.
#[test]
fn every_hook_clauseo_writes_is_exec_form() {
    for forced_ask in [false, true] {
        let meta = meta(forced_ask);
        let hooks = meta["claudeCode"]["options"]["settings"]["hooks"]
            .as_object()
            .unwrap();
        let mut seen = 0;
        for (event, entries) in hooks {
            for entry in entries.as_array().unwrap() {
                for hook in entry["hooks"].as_array().unwrap() {
                    assert_eq!(hook["type"], json!("command"), "{event}: {hook}");
                    assert!(
                        hook["command"].as_str().is_some_and(|c| !c.is_empty()),
                        "{hook}"
                    );
                    assert!(hook["args"].is_array(), "{event}: {hook} has no args");
                    seen += 1;
                }
            }
        }
        assert_eq!(seen, 1, "the guard's entry: {hooks:?}");
        assert_eq!(
            meta["claudeCode"]["options"]["settings"]["hooks"]["PreToolUse"][0]["hooks"][0]["args"]
                [2],
            json!(MATTER)
        );
    }
}
