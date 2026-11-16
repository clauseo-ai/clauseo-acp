//! The routing rule: routes pass alone, companions only beside a route, the always-kept names
//! always, every other prefixed name drops; each OS asserting how it reads names. #1167's table
//! rows, per system, are in `rows.rs`.

use std::ffi::{OsStr, OsString};

use super::*;

mod rows;

fn os(text: &str) -> &OsStr {
    OsStr::new(text)
}

fn pairs(list: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    list.iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

fn route(list: &[(&str, &str)]) -> bool {
    route_present(list.iter().map(|(name, value)| (os(name), os(value))))
}

/// The names [`kept_inherited`] keeps from `list`.
fn kept_names(list: &[(&str, &str)]) -> Vec<String> {
    kept_inherited(pairs(list))
        .into_iter()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect()
}

/// The six provider addresses: passed on their own, never a route.
const PROVIDER_ADDRESSES: [&str; 6] = [
    "ANTHROPIC_BEDROCK_BASE_URL",
    "ANTHROPIC_VERTEX_BASE_URL",
    "ANTHROPIC_FOUNDRY_BASE_URL",
    "ANTHROPIC_AWS_BASE_URL",
    "ANTHROPIC_GOOGLE_CLOUD_BASE_URL",
    "ANTHROPIC_BEDROCK_MANTLE_BASE_URL",
];

/// 13 routes then 52 companions, as host's table counts; no name twice, none also on the
/// always-dropped or always-kept lists, every name under a prefix.
#[test]
fn the_lists_are_host_s_table() {
    assert_eq!(ROUTES.len(), 13);
    assert_eq!(ROUTING_KEPT.len(), 65);
    assert_eq!(ROUTES[1..7], PROVIDER_ADDRESSES);
    let mut all: Vec<&str> = ROUTING_KEPT
        .iter()
        .chain(&CLAUDE_ACCOUNT_ONLY)
        .chain(&ALWAYS_KEPT)
        .copied()
        .collect();
    assert!(all.iter().all(|name| under_prefixes(os(name))));
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), 65 + 26 + 1, "a name twice");
    assert_eq!(ALWAYS_KEPT, ["CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"]);
    assert_eq!(
        COMPANIONS_OUTSIDE_PREFIXES,
        ["_CLAUDE_CODE_ASSUME_FIRST_PARTY_BASE_URL"]
    );
    assert!(!under_prefixes(os(COMPANIONS_OUTSIDE_PREFIXES[0])));
}

/// Host's `only_the_listed_routing_names_pass_and_every_other_prefixed_name_drops`, as spelled:
/// a route name passes on its own; every other name on `ROUTING_KEPT`, and the first-party
/// assumption, passes only beside a route; the always-kept name passes route or not; every
/// other prefixed name drops, route or not.
#[test]
fn only_the_listed_routing_names_pass_and_every_other_prefixed_name_drops() {
    for name in ROUTING_KEPT {
        assert!(kept(os(name), true), "{name} passes");
        assert_eq!(kept(os(name), false), ROUTES.contains(&name), "{name}");
    }
    for name in ALWAYS_KEPT {
        assert!(kept(os(name), true) && kept(os(name), false), "{name}");
    }
    for name in COMPANIONS_OUTSIDE_PREFIXES {
        assert!(kept(os(name), true) && !kept(os(name), false), "{name}");
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
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC_EXTRA",
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
        assert!(!kept(os(name), true) && !kept(os(name), false), "{name}");
    }
}

/// `CLAUDE_CODE_DISABLE_ADMIN_ENV_UNION` stays dropped, route or not, while the always-kept
/// name stays with no route at all.
#[test]
fn the_always_kept_name_stays_with_no_route_and_the_admin_union_switch_drops() {
    for route in [
        None,
        Some(("ANTHROPIC_BASE_URL", "https://gw.firm.example")),
    ] {
        let mut list = vec![
            ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
            ("CLAUDE_CODE_DISABLE_ADMIN_ENV_UNION", "1"),
        ];
        list.extend(route);
        let kept = kept_names(&list);
        assert!(kept.contains(&ALWAYS_KEPT[0].to_owned()), "{route:?}");
        assert!(!kept.iter().any(|name| name.contains("ADMIN")), "{route:?}");
    }
}

/// Lower-case spellings: on macOS and Linux Claude Code reads only the exact name, so none is
/// kept and none is a route.
#[cfg(not(windows))]
#[test]
fn a_lower_case_listed_name_is_never_kept_nor_a_route() {
    for name in ROUTING_KEPT
        .iter()
        .chain(&ALWAYS_KEPT)
        .chain(&COMPANIONS_OUTSIDE_PREFIXES)
        .chain(&CLAUDE_ACCOUNT_ONLY)
    {
        for spelling in [name.to_ascii_lowercase(), mixed(name)] {
            assert!(!kept(os(&spelling), true), "{spelling}");
            assert!(
                kept_names(&[(spelling.as_str(), "1")]).is_empty(),
                "{spelling}"
            );
        }
    }
    assert!(!route(&[("anthropic_base_url", "https://gw.firm.example")]));
    assert!(!route(&[("Claude_Code_Use_Vertex", "1")]));
}

/// Lower-case spellings: Windows reads names in any case, so each is kept as its upper-case
/// spelling would be.
#[cfg(windows)]
#[test]
fn a_lower_case_listed_name_is_kept_as_its_upper_case_one() {
    for name in ROUTING_KEPT
        .iter()
        .chain(&ALWAYS_KEPT)
        .chain(&COMPANIONS_OUTSIDE_PREFIXES)
        .chain(&CLAUDE_ACCOUNT_ONLY)
    {
        for spelling in [name.to_ascii_lowercase(), mixed(name)] {
            for route in [true, false] {
                assert_eq!(
                    kept(os(&spelling), route),
                    kept(os(name), route),
                    "{spelling}"
                );
            }
        }
    }
    assert!(route(&[("anthropic_base_url", "https://gw.firm.example")]));
    assert!(route(&[("Claude_Code_Use_Vertex", "1")]));
}

/// `name` with every other letter lower-cased.
fn mixed(name: &str) -> String {
    name.chars()
        .enumerate()
        .map(|(at, c)| {
            if at % 2 == 0 {
                c.to_ascii_lowercase()
            } else {
                c
            }
        })
        .collect()
}

#[test]
fn a_route_is_a_base_url_that_is_not_blank_or_a_switch_that_is_on() {
    assert!(!route(&[]));
    assert!(route(&[("ANTHROPIC_BASE_URL", "https://gw.firm.example")]));
    assert!(route(&[("ANTHROPIC_BASE_URL", " x ")]));
    for blank in [
        "",
        " ",
        "   ",
        "\t\n",
        "\u{FEFF}",
        "\u{3000}",
        "\u{A0}\u{2028}",
    ] {
        assert!(!route(&[("ANTHROPIC_BASE_URL", blank)]), "{blank:?}");
    }
    // U+0085 is not JavaScript whitespace: Claude Code reads it as an address.
    assert!(route(&[("ANTHROPIC_BASE_URL", "\u{85}")]));
    for on in [
        "1",
        "true",
        " True ",
        "YES",
        "on\n",
        "\u{FEFF}1",
        "\u{3000}on\u{A0}",
    ] {
        assert!(route(&[("CLAUDE_CODE_USE_VERTEX", on)]), "{on:?}");
    }
    for off in [
        "",
        "0",
        "false",
        "no",
        "off",
        "2",
        "y",
        "1\u{85}",
        "\u{85}true",
        "1\u{200B}",
    ] {
        assert!(!route(&[("CLAUDE_CODE_USE_VERTEX", off)]), "{off:?}");
    }
    // A provider address alone is no route, whatever its switch but on.
    for address in PROVIDER_ADDRESSES {
        for switch in [None, Some("0"), Some("")] {
            let mut list = vec![(address, "https://cloud.example")];
            let name = provider_switch(address);
            list.extend(switch.map(|value| (name, value)));
            assert!(!route(&list), "{address} {switch:?}");
        }
        assert!(
            route(&[(address, "x"), (provider_switch(address), "1")]),
            "{address}"
        );
    }
    // A companion is no route, nor is the gateway switch, nor the always-kept name.
    assert!(!route(&[
        ("ANTHROPIC_AUTH_TOKEN", "t"),
        ("CLAUDE_CODE_USE_GATEWAY", "1"),
        ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
        ("_CLAUDE_CODE_ASSUME_FIRST_PARTY_BASE_URL", "1"),
    ]));
}

/// The switch of a provider's address (Mantle's is `CLAUDE_CODE_USE_MANTLE`).
fn provider_switch(address: &str) -> &'static str {
    let at = PROVIDER_ADDRESSES
        .iter()
        .position(|listed| *listed == address)
        .unwrap();
    ROUTES[7 + at]
}

/// A lone inherited token never passes: beside a provider address whose switch is off or
/// absent, or a blank `ANTHROPIC_BASE_URL`, the token drops (the address passes on its own).
#[test]
fn a_token_beside_something_that_is_no_route_drops() {
    let mut shapes: Vec<Vec<(&str, &str)>> = Vec::new();
    for address in PROVIDER_ADDRESSES {
        shapes.push(vec![(address, "https://cloud.example")]);
        shapes.push(vec![
            (address, "https://cloud.example"),
            (provider_switch(address), "0"),
        ]);
    }
    for blank in ["", "   ", "\u{FEFF}", "\u{3000}"] {
        shapes.push(vec![("ANTHROPIC_BASE_URL", blank)]);
    }
    if cfg!(not(windows)) {
        shapes.push(vec![("anthropic_base_url", "https://gw.firm.example")]);
    }
    for mut shape in shapes {
        shape.push(("ANTHROPIC_AUTH_TOKEN", "someone-elses-token"));
        let kept = kept_names(&shape);
        assert!(!kept.iter().any(|name| name.contains("TOKEN")), "{shape:?}");
        let first = shape[0].0;
        let expected = first.to_ascii_uppercase() == first;
        assert_eq!(kept.contains(&first.to_owned()), expected, "{shape:?}");
    }
}

/// The route is decided once over the whole inherited env: a companion listed before its route
/// is kept too, and the order of what is kept is the order given. The first-party assumption is
/// kept beside a route and dropped without one.
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
    let lone = kept_names(&[
        ("_CLAUDE_CODE_ASSUME_FIRST_PARTY_BASE_URL", "1"),
        ("ANTHROPIC_BASE_URL", " "),
        ("PATH", "bin"),
    ]);
    assert_eq!(lone, ["ANTHROPIC_BASE_URL", "PATH"]);
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

/// On macOS and Linux a name with a non-ASCII letter is not under the prefixes (Claude Code
/// would not read it as one of its names) and passes as any other name does.
#[cfg(not(windows))]
#[test]
fn a_non_ascii_spelling_is_an_ordinary_name() {
    assert!(!under_prefixes(os("ANTHROP\u{131}C_AUTH_TOKEN")));
    assert!(under_prefixes(os("anthropic_auth_token")));
    let kept = kept_names(&[
        ("ANTHROP\u{131}C_BASE_URL", "x"),
        ("ANTHROPIC_AUTH_TOKEN", "t"),
    ]);
    assert_eq!(kept, ["ANTHROP\u{131}C_BASE_URL"]);
}

/// On Windows spellings that differ only in case are folded before anything is decided: one
/// value, one spelling kept (the first) and a route; two values, none kept and no route.
#[cfg(windows)]
#[test]
fn windows_folds_case_only_spellings_before_deciding() {
    let disagree = kept_names(&[
        ("ANTHROPIC_BASE_URL", "https://gw.firm.example"),
        ("anthropic_base_url", ""),
        ("ANTHROPIC_AUTH_TOKEN", "t"),
        ("PATH", "bin"),
    ]);
    assert_eq!(disagree, ["PATH"]);
    assert!(!route(&[
        ("anthropic_base_url", ""),
        ("ANTHROPIC_BASE_URL", "https://gw.firm.example"),
    ]));
    let agree = kept_names(&[
        ("Anthropic_Base_Url", "https://gw.firm.example"),
        ("ANTHROPIC_BASE_URL", "https://gw.firm.example"),
        ("anthropic_auth_token", "t"),
    ]);
    assert_eq!(agree, ["Anthropic_Base_Url", "anthropic_auth_token"]);
    let switches = kept_names(&[
        ("CLAUDE_CODE_USE_BEDROCK", "1"),
        ("claude_code_use_bedrock", "0"),
        ("ANTHROPIC_AUTH_TOKEN", "t"),
    ]);
    assert_eq!(switches, Vec::<String>::new());
    let tokens = kept_names(&[
        ("ANTHROPIC_BASE_URL", "https://gw.firm.example"),
        ("ANTHROPIC_AUTH_TOKEN", "a"),
        ("Anthropic_Auth_Token", "b"),
    ]);
    assert_eq!(tokens, ["ANTHROPIC_BASE_URL"]);
}

/// On Windows the prefix test upper-cases by Unicode: a dotless ı (U+0131) spelling falls under
/// the rule, matches no listed name, drops, and is no route.
#[cfg(windows)]
#[test]
fn windows_reads_the_prefixes_by_unicode_case() {
    assert!(under_prefixes(os("anthrop\u{131}c_auth_token")));
    let kept = kept_names(&[
        ("ANTHROP\u{131}C_BASE_URL", "https://gw.firm.example"),
        ("ANTHROPIC_AUTH_TOKEN", "t"),
        ("PATH", "bin"),
    ]);
    assert_eq!(kept, ["PATH"]);
    assert!(!route(&[(
        "ANTHROP\u{131}C_BASE_URL",
        "https://gw.firm.example"
    )]));
    let token = kept_names(&[
        ("ANTHROPIC_BASE_URL", "https://gw.firm.example"),
        ("ANTHROP\u{131}C_AUTH_TOKEN", "t"),
    ]);
    assert_eq!(token, ["ANTHROPIC_BASE_URL"]);
}
