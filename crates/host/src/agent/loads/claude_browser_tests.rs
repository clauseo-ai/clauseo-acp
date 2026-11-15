//! Claude's guarded `browser` entry in its keys (`ClaudeKeys::browser_mcp`): the server-level
//! allow rule `mcp__browser` in `allowedTools` outside forced ask, so ordinary browsing never
//! asks; none in forced ask, where the `mcp__*` ask rule gives cards; no hook rule for the
//! server without the entry; and never the dormant shell browser's rules beside it.

use serde_json::{Value, json};

use super::browser::Switch;
use super::profile::{ClaudeBrowser, ClaudeKeys, Login, claude_options};
use super::profile_keys_tests::folders;
use crate::agent::data_fence::LaunchDeny;

/// The rule the entry gets.
const RULE: &str = "mcp__browser";

/// Claude's options over `hook`, with the entry (`browser_mcp`), the dormant shell tool
/// (`shell`) and forced ask as given.
fn options(hook: &Value, browser_mcp: bool, shell: bool, forced_ask: bool) -> Value {
    let deny = LaunchDeny::default();
    let keys = ClaudeKeys {
        web_search: Switch {
            on: true,
            locked: false,
        },
        login: Login::FirstParty,
        folders: folders(),
        seed_sites: &[],
        forced_ask,
        data: &deny,
        case_law: false,
        browser: ClaudeBrowser::of(browser_mcp, shell),
        sandbox: None,
        matter: None,
        trusted_servers: &[],
    };
    let mut options = hook.as_object().cloned().unwrap_or_default();
    claude_options(&mut options, &keys);
    Value::Object(options)
}

/// The strings of the array at `key`, or none when the key is missing.
fn list(value: &Value, key: &str) -> Option<Vec<String>> {
    value.get(key).and_then(Value::as_array).map(|items| {
        items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    })
}

#[test]
fn the_entry_runs_unasked_outside_forced_ask_and_asks_in_it() {
    let auto = options(&json!({}), true, false, false);
    let allowed = list(&auto, "allowedTools").unwrap();
    assert_eq!(
        allowed.iter().filter(|rule| *rule == RULE).count(),
        1,
        "{allowed:?}"
    );
    assert_eq!(RULE, crate::agent::mcp_trust::claude_rule("browser"));
    // Forced ask: no allowed tools at all, and the ask rule that covers every MCP tool.
    let forced = options(&json!({}), true, false, true);
    assert!(forced.get("allowedTools").is_none(), "{forced}");
    let ask = list(&forced["settings"]["permissions"], "ask").unwrap();
    assert!(ask.iter().any(|rule| rule == "mcp__*"), "{ask:?}");
    // Without the entry, no rule for it.
    let without = list(&options(&json!({}), false, false, false), "allowedTools").unwrap();
    assert!(
        !without.iter().any(|rule| rule.starts_with(RULE)),
        "{without:?}"
    );
}

#[test]
fn a_hook_rule_for_the_browser_server_goes_and_another_servers_stays() {
    let hook = json!({
        "allowedTools": [
            "mcp__browser",
            "mcp__browser__agent_browser_eval",
            "mcp__browsers",
            "mcp__docs__read",
        ]
    });
    let without = list(&options(&hook, false, false, false), "allowedTools").unwrap();
    assert!(
        !without
            .iter()
            .any(|rule| rule == RULE || rule.starts_with("mcp__browser__"))
    );
    for kept in ["mcp__browsers", "mcp__docs__read"] {
        assert!(
            without.iter().any(|rule| rule == kept),
            "{kept}: {without:?}"
        );
    }
    let with = list(&options(&hook, true, false, false), "allowedTools").unwrap();
    assert_eq!(with.iter().filter(|rule| *rule == RULE).count(), 1);
    assert!(
        !with.iter().any(|rule| rule.starts_with("mcp__browser__")),
        "{with:?}"
    );
    assert!(with.iter().any(|rule| rule == "mcp__browsers"));
}

/// One value (`ClaudeBrowser`) picks the rules, so the entry never carries the shell's.
#[test]
fn the_shell_browsers_rules_never_go_with_the_entry() {
    let both = options(&json!({}), true, true, false);
    let text = both.to_string();
    assert!(!text.contains("agent-browser"), "{text}");
    assert!(
        list(&both, "allowedTools")
            .unwrap()
            .iter()
            .any(|rule| rule == RULE)
    );
    // The dormant shell tool alone still gets its rules, as before.
    let shell = options(&json!({}), false, true, false).to_string();
    assert!(shell.contains("agent-browser"), "{shell}");
    assert!(!shell.contains(&format!("\"{RULE}\"")), "{shell}");
}
