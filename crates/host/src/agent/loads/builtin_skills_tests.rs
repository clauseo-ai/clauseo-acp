//! A Claude session's read of Clauseo's built-in skill assets, from the layering of its `_meta`
//! (moved from `layering_tests.rs` as they were).

use serde_json::{Value, json};

use super::profile::{ClaudeKeys, Login};
use super::profile_keys_tests::folders;
use super::profile_tests::ON;
use super::{McpControl, session_meta};
use crate::browser::seed_sites::SEED_SITES;

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
/// the lawyer; no deny rule covers them, and no allow covers anything else in the data folder
/// but a Read of the agents' commands and Clauseo's Python runtime (`data_fence::READ_ONLY`).
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
        for tool in ["Read", "Edit"] {
            let denied: Vec<&String> = deny.iter().filter(|r| covers(r, tool, &asset)).collect();
            assert!(denied.is_empty(), "{tool} {asset}: {denied:?}");
        }
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
