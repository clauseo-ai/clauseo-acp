//! Every Claude session's data-fence deny rules, as they layer over the hook's own in its
//! `_meta` (`layering_tests.rs` for the rest of the layering).

use std::path::Path;

use serde_json::{Value, json};

use super::layering_tests::{ON, folders};
use super::profile::{ClaudeKeys, Login};
use super::{GuardHook, McpControl, session_meta};
use crate::browser::seed_sites::SEED_SITES;

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
            versions: crate::versions::VersionsTool::Absent,
            browser: false,
            sandbox: None,
            matter: None,
            trusted_servers: &[],
        };
        let guard = Some(GuardHook {
            program: Path::new("/app/clauseo"),
            matter: Path::new("/m"),
            ask_all: forced_ask,
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
                    && !crate::run_tokens::RUN_TOKEN_FILES
                        .iter()
                        .any(|token| rule.ends_with(&format!("/run/*/{token})")))
                    && !rule.ends_with("/run/*/proxy-secret)")
                    && !rule.ends_with("/run/*/agent-browser/config.json)"))
                    || rule.contains("/agent-state")
            }),
            "{rules:?}"
        );
    }
    assert_eq!(meta_for(&own, false)[0], json!("Bash(rm:*)"));
}
