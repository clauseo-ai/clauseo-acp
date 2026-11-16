//! The routing rule: routes pass alone, companions only beside a route, every other prefixed
//! name drops; and #1167's table rows, on the inherited side (the cases of host's
//! `agent/tests/scrub.rs` at `lw-review-fixes`).

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

use super::*;

fn os(text: &str) -> &OsStr {
    OsStr::new(text)
}

fn pairs(list: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    list.iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

/// 13 routes then 52 companions, as host's table counts; no name twice, none also on the
/// always-dropped list, every name under a prefix.
#[test]
fn the_lists_are_host_s_table() {
    assert_eq!(ROUTES.len(), 13);
    assert_eq!(ROUTING_KEPT.len(), 65);
    let mut all: Vec<&str> = ROUTING_KEPT
        .iter()
        .chain(&CLAUDE_ACCOUNT_ONLY)
        .copied()
        .collect();
    assert!(all.iter().all(|name| under_prefixes(os(name))));
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), 65 + 26, "a name twice");
}

/// Host's `only_the_listed_routing_names_pass_and_every_other_prefixed_name_drops`: a route
/// passes on its own, in any case; every other name on `ROUTING_KEPT` passes only beside a
/// route; every other prefixed name drops, in any case, route or not.
#[test]
fn only_the_listed_routing_names_pass_and_every_other_prefixed_name_drops() {
    for name in ROUTING_KEPT {
        let lower = name.to_ascii_lowercase();
        for name in [name, lower.as_str()] {
            let route = ROUTES
                .iter()
                .any(|listed| listed.eq_ignore_ascii_case(name));
            assert!(kept(os(name), true), "{name} passes");
            assert_eq!(kept(os(name), false), route, "{name}");
        }
    }
    for name in [
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CODE_OAUTH_REFRESH_TOKEN",
        "CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR",
        "CLAUDE_CODE_SESSION_ACCESS_TOKEN",
        "ANTHROPIC_UNIX_SOCKET",
        "CLAUDE_CODE_API_BASE_URL",
        "ANTHROPIC_IDENTITY_TOKEN",
        "ANTHROPIC_FEDERATION_RULE_ID",
        "ANTHROPIC_PROFILE",
        "ANTHROPIC_CONFIG_DIR",
        "CLAUDE_CODE_HOST_CREDS_FILE",
        "CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST",
        "CLAUDE_CODE_CUSTOM_OAUTH_URL",
        "CLAUDE_CODE_REMOTE_SETTINGS_PATH",
        "CLAUDE_CODE_DISABLE_ADMIN_ENV_UNION",
        "CLAUDE_CODE_USE_COWORK_PLUGINS",
        "CLAUDE_CODE_USE_POWERSHELL_TOOL",
        "CLAUDE_CODE_USE_NATIVE_FILE_SEARCH",
        "CLAUDE_CODE_HTTPS_PROXY",
        "CLAUDE_CODE_SANDBOXED",
        "CLAUDE_CODE_BUBBLEWRAP",
        "CLAUDE_CODE_3P_SEEDED_OPUS_DEFAULT",
        "CLAUDE_CONFIG_DIR",
        "ANTHROPIC_BETAS",
        "ANTHROPIC_BASE_URL_EXTRA",
        "ANTHROPIC_DEFAULT_NEXT_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL_EXTRA",
        "CLAUDE_CODE_SKIP_ANY_AUTH",
        "CLAUDE_CODE_USE_SOMETHING_NEW",
        "ANTHROPIC_",
        "CLAUDE_",
    ] {
        assert!(!kept(os(name), true), "{name} drops");
        assert!(under_prefixes(os(name)), "{name}");
        assert!(!ROUTING_KEPT.contains(&name), "{name} is not on the list");
    }
    for name in CLAUDE_ACCOUNT_ONLY {
        let lower = name.to_ascii_lowercase();
        for name in [name, lower.as_str()] {
            assert!(
                !kept(os(name), true) && !kept(os(name), false),
                "{name} drops"
            );
        }
    }
}

#[test]
fn a_route_is_an_endpoint_with_a_value_or_a_switch_that_is_on() {
    let route = |list: &[(&str, &str)]| {
        route_present(list.iter().map(|(name, value)| (os(name), os(value))))
    };
    assert!(!route(&[]));
    assert!(route(&[("anthropic_base_url", "https://gw.firm.example")]));
    assert!(!route(&[("ANTHROPIC_BASE_URL", "")]));
    for on in ["1", "true", " True ", "YES", "on\n"] {
        assert!(route(&[("CLAUDE_CODE_USE_VERTEX", on)]), "{on:?}");
    }
    for off in ["", "0", "false", "no", "off", "2", "y"] {
        assert!(!route(&[("CLAUDE_CODE_USE_VERTEX", off)]), "{off:?}");
    }
    // A companion is no route, nor is the gateway switch.
    assert!(!route(&[
        ("ANTHROPIC_AUTH_TOKEN", "t"),
        ("CLAUDE_CODE_USE_GATEWAY", "1")
    ]));
}

/// One row of #1167's table: what it shows, the inherited names, the hook's (a firm plan's), and
/// every `CLAUDE_*` and `ANTHROPIC_*` name the launch holds but its home.
type Row = (
    &'static str,
    &'static [(&'static str, &'static str)],
    &'static [(&'static str, &'static str)],
    &'static [(&'static str, &'static str)],
);

/// The rows. `ANTHROPIC_API_KEY` is the settings entry's `""` unless the hook sets the stored key:
/// an inherited key never wins over it.
const ROWS: [Row; 9] = [
    (
        "an inherited endpoint with its token",
        &[
            ("ANTHROPIC_BASE_URL", "https://gw.firm.example"),
            ("ANTHROPIC_AUTH_TOKEN", "firm-token"),
        ],
        &[],
        &[
            ("ANTHROPIC_API_KEY", ""),
            ("ANTHROPIC_AUTH_TOKEN", "firm-token"),
            ("ANTHROPIC_BASE_URL", "https://gw.firm.example"),
        ],
    ),
    (
        "an inherited endpoint with no token (#1152 refuses it while no key is held)",
        &[("ANTHROPIC_BASE_URL", "https://gw.firm.example")],
        &[],
        &[
            ("ANTHROPIC_API_KEY", ""),
            ("ANTHROPIC_BASE_URL", "https://gw.firm.example"),
        ],
    ),
    (
        "a firm plan's endpoint and the stored key, over an inherited one",
        &[("ANTHROPIC_BASE_URL", "https://elsewhere.example")],
        &[
            ("ANTHROPIC_BASE_URL", "https://plan.firm.example"),
            ("ANTHROPIC_API_KEY", "sk-stored"),
        ],
        &[
            ("ANTHROPIC_API_KEY", "sk-stored"),
            ("ANTHROPIC_BASE_URL", "https://plan.firm.example"),
        ],
    ),
    ("none", &[], &[], &[("ANTHROPIC_API_KEY", "")]),
    (
        "an inherited account token: dropped, so Clauseo's own sign-in is used",
        &[
            ("CLAUDE_CODE_OAUTH_TOKEN", "sk-ant-oat-firm"),
            ("CLAUDE_CODE_OAUTH_REFRESH_TOKEN", "rt-firm"),
        ],
        &[],
        &[("ANTHROPIC_API_KEY", "")],
    ),
    (
        "a lone token with no endpoint: an account swap, dropped",
        &[("ANTHROPIC_AUTH_TOKEN", "someone-elses-token")],
        &[],
        &[("ANTHROPIC_API_KEY", "")],
    ),
    (
        "an inherited key beside its endpoint: the entry's empty key still wins",
        &[
            ("ANTHROPIC_BASE_URL", "https://gw.firm.example"),
            ("ANTHROPIC_API_KEY", "sk-inherited"),
        ],
        &[],
        &[
            ("ANTHROPIC_API_KEY", ""),
            ("ANTHROPIC_BASE_URL", "https://gw.firm.example"),
        ],
    ),
    (
        "a provider switch with its companions: all passed",
        &[
            ("claude_code_use_bedrock", " True "),
            ("ANTHROPIC_BEDROCK_REGION_PREFIX", "eu"),
            ("CLAUDE_CODE_SKIP_BEDROCK_AUTH", "1"),
            ("ANTHROPIC_CUSTOM_HEADERS", "X-Firm: 1"),
            (
                "ANTHROPIC_MODEL",
                "arn:aws:bedrock:eu-west-1:1:inference-profile/firm",
            ),
        ],
        &[],
        &[
            ("ANTHROPIC_API_KEY", ""),
            ("ANTHROPIC_BEDROCK_REGION_PREFIX", "eu"),
            ("ANTHROPIC_CUSTOM_HEADERS", "X-Firm: 1"),
            (
                "ANTHROPIC_MODEL",
                "arn:aws:bedrock:eu-west-1:1:inference-profile/firm",
            ),
            ("CLAUDE_CODE_SKIP_BEDROCK_AUTH", "1"),
            ("claude_code_use_bedrock", " True "),
        ],
    ),
    (
        "companions with no route (a switch that is off, an empty endpoint): dropped",
        &[
            ("CLAUDE_CODE_USE_VERTEX", "0"),
            ("ANTHROPIC_BASE_URL", ""),
            ("CLAUDE_CODE_USE_GATEWAY", "1"),
            ("ANTHROPIC_AUTH_TOKEN", "someone-elses-token"),
            ("ANTHROPIC_CUSTOM_HEADERS", "X-Other: 1"),
            ("ANTHROPIC_VERTEX_PROJECT_ID", "other-project"),
            ("ANTHROPIC_MODEL", "claude-other"),
            ("CLAUDE_CODE_CLIENT_CERT", "/tmp/other.pem"),
        ],
        &[],
        &[
            ("ANTHROPIC_API_KEY", ""),
            ("ANTHROPIC_BASE_URL", ""),
            ("CLAUDE_CODE_USE_VERTEX", "0"),
        ],
    ),
];

/// A launch as host builds one, for its routing names: [`kept_inherited`] over the inherited
/// environment, then the settings entry's `ANTHROPIC_API_KEY: ""`, then the hook's names, each
/// over any inherited spelling of its name. Its `CLAUDE_*` and `ANTHROPIC_*` names.
fn launch(inherited: &[(&str, &str)], hook: &[(&str, &str)]) -> BTreeMap<String, String> {
    let mut env: Vec<(OsString, OsString)> = kept_inherited(pairs(inherited));
    for (name, value) in [("ANTHROPIC_API_KEY", "")].iter().chain(hook) {
        env.retain(|(key, _)| !key.eq_ignore_ascii_case(name));
        env.push((OsString::from(name), OsString::from(value)));
    }
    env.into_iter()
        .filter(|(name, _)| under_prefixes(name))
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect()
}

/// Host's `routing_points_where_it_did_before_the_scrub`, row for row: every `CLAUDE_*` and
/// `ANTHROPIC_*` name a launch holds. A name that only swaps the account never gets through, nor
/// does a companion with no route; the entry's empty key and the hook's names win.
#[test]
fn routing_points_where_it_did_before_the_scrub() {
    for (row, inherited, hook, expected) in ROWS {
        let expected: BTreeMap<String, String> = expected
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        assert_eq!(launch(inherited, hook), expected, "{row}");
    }
}

/// The route is decided once over the whole inherited env: a companion listed before its route
/// is kept too, and the order of what is kept is the order given.
#[test]
fn kept_inherited_decides_the_route_once_and_keeps_the_order() {
    let inherited = pairs(&[
        ("ANTHROPIC_AUTH_TOKEN", "t"),
        ("CLAUDE_CODE_OAUTH_TOKEN", "o"),
        ("ANTHROPIC_BASE_URL", "https://gw.firm.example"),
        ("HTTPS_PROXY", "http://proxy.firm.example:8080"),
        ("_CLAUDE_CODE_ASSUME_FIRST_PARTY_BASE_URL", "1"),
    ]);
    let kept = kept_inherited(inherited.clone());
    let expected: Vec<(OsString, OsString)> = [0, 2, 3, 4]
        .into_iter()
        .map(|at| inherited[at].clone())
        .collect();
    assert_eq!(kept, expected);
}

/// Host's `SCRUBBED` names outside the prefixes drop in any case, route or not; the firm's proxy
/// and CA stay.
#[test]
fn the_names_also_dropped_go_and_the_firm_s_proxy_and_ca_stay() {
    for route in [
        None,
        Some(("ANTHROPIC_BASE_URL", "https://gw.firm.example")),
    ] {
        for name in ALSO_DROPPED {
            for spelling in [name.to_ascii_uppercase(), name.to_ascii_lowercase()] {
                let mut list = vec![(spelling.as_str(), "1"), ("PATH", "bin")];
                list.extend(route);
                let kept = kept_inherited(pairs(&list));
                assert!(
                    !kept.iter().any(|(key, _)| key.eq_ignore_ascii_case(name)),
                    "{spelling}"
                );
                assert!(kept.iter().any(|(key, _)| key == "PATH"));
            }
        }
    }
    let firm = [
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "NODE_EXTRA_CA_CERTS",
        "SSL_CERT_FILE",
    ];
    let list: Vec<(&str, &str)> = firm.iter().map(|name| (*name, "v")).collect();
    assert_eq!(kept_inherited(pairs(&list)).len(), firm.len());
}
