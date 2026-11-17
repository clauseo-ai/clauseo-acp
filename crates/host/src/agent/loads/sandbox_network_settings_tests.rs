//! The shell's network list as the launch builds the flag settings: a hook's fetch rules never
//! widen it, and with web search off it is empty (`sandbox_network`, decision record 0065).

use serde_json::{Value, json};

use super::super::browser::Switch;
use super::super::profile::{ClaudeKeys, Login, claude_options};
use super::{NO_DENY, Session, folders, mac, texts};

/// The flag settings of a sandboxed macOS session outside forced ask with web search `on` or
/// off and the real seed sites, over `hook`.
fn web_settings(session: &Session, on: bool, hook: &Value) -> Value {
    let sandbox = mac(&session.paths()).unwrap();
    let keys = ClaudeKeys {
        web_search: Switch { on, locked: false },
        login: Login::FirstParty,
        folders: folders(session),
        seed_sites: &crate::browser::seed_sites::WEB_FETCH_SITES[..],
        forced_ask: false,
        data: &NO_DENY,
        case_law: false,
        browser: false,
        sandbox: Some(&sandbox),
        matter: None,
        trusted_servers: &[],
    };
    let mut options = hook.as_object().cloned().unwrap_or_default();
    claude_options(&mut options, &keys);
    options["settings"].clone()
}

/// The hosts the shell may reach under flag `settings`, as Claude Code 2.1.293 builds its list:
/// `allowedDomains` and the host of each `WebFetch(domain:…)` rule in `permissions.allow`,
/// which it merges in (decision record 0065). `allowedTools` are `cliArg` rules, not merged.
/// Panics on any other `WebFetch` rule there, which would mean every host.
fn shell_hosts(settings: &Value) -> Vec<String> {
    let mut hosts: Vec<String> = texts(&settings["sandbox"]["network"]["allowedDomains"])
        .into_iter()
        .map(str::to_owned)
        .collect();
    let allow = settings["permissions"]["allow"].as_array().unwrap();
    for rule in allow.iter().filter_map(Value::as_str) {
        if rule.starts_with("WebFetch") {
            let host = rule
                .strip_prefix("WebFetch(domain:")
                .and_then(|rest| rest.strip_suffix(')'))
                .unwrap_or_else(|| panic!("an open fetch rule: {rule}"));
            hosts.push(host.to_owned());
        }
    }
    hosts.sort_unstable();
    hosts.dedup();
    hosts
}

/// A hook with wildcard fetch rules and one named host, in `allowedTools` and in the flag
/// settings' `permissions.allow`: the shell reaches the seeded sites and that host, never a
/// wildcard.
#[test]
#[cfg_attr(windows, ignore = "Claude's sandbox is macOS and Linux only")]
fn a_hooks_wildcard_fetch_rule_never_widens_the_shell() {
    let session = Session::new("s-1");
    let rules = json!([
        "WebFetch(domain:*)",
        "WebFetch",
        "WebFetch(domain:*.evil.org)",
        "WebFetch(domain:127.0.0.1)",
        "WebFetch(domain:hook.example.org)",
    ]);
    let hook = json!({
        "allowedTools": rules,
        "settings": {"permissions": {"allow": rules}},
    });
    let settings = web_settings(&session, true, &hook);
    let mut expected: Vec<String> = super::super::sandbox_network::allowed_domains(&[])
        .into_iter()
        .map(str::to_owned)
        .collect();
    expected.push("hook.example.org".to_owned());
    expected.sort_unstable();
    let hosts = shell_hosts(&settings);
    assert_eq!(hosts, expected);
    assert!(!hosts.iter().any(|host| host.contains('*')), "{hosts:?}");
    assert_eq!(settings["sandbox"]["network"]["strictAllowlist"], true);
}

/// With web search off the shell reaches no host: an empty, strict list, and no fetch rule of
/// the hook's left in the flag settings to merge in.
#[test]
#[cfg_attr(windows, ignore = "Claude's sandbox is macOS and Linux only")]
fn web_off_leaves_the_shell_no_host() {
    let session = Session::new("s-1");
    let hook = json!({
        "settings": {"permissions": {"allow": ["WebFetch(domain:hook.example.org)", "WebSearch"]}},
    });
    let settings = web_settings(&session, false, &hook);
    assert_eq!(shell_hosts(&settings), Vec::<String>::new());
    assert_eq!(settings["sandbox"]["network"]["strictAllowlist"], true);
    let allow = texts(&settings["permissions"]["allow"]);
    assert!(
        !allow.iter().any(|rule| rule.starts_with("Web")),
        "{allow:?}"
    );
}
