//! When Claude Code's own sign-in would reach a custom endpoint, by a plan and by a whole env.

use std::collections::BTreeMap;
use std::ffi::OsString;

use super::{
    SettingsEnv, is_default_address, js_trim, own_signin_reaches_endpoint, plan_keeps_own_signin,
};
use crate::schema::{Plan, PlanProvider};

fn plan(endpoint: bool, provider: Option<PlanProvider>) -> Plan {
    plan_at(endpoint.then_some("https://gateway.firm.example"), provider)
}

fn plan_at(endpoint: Option<&str>, provider: Option<PlanProvider>) -> Plan {
    Plan {
        endpoint: endpoint.map(str::to_owned),
        provider,
        env: BTreeMap::new(),
    }
}

fn reaches(vars: &[(&str, &str)]) -> bool {
    let env: BTreeMap<OsString, OsString> = vars
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect();
    own_signin_reaches_endpoint(
        env.iter().map(|(n, v)| (n.as_os_str(), v.as_os_str())),
        &SettingsEnv::default(),
    )
}

/// `vars` in this order, with `settings`' pairs over them, names folded as on Windows when
/// `windows`.
fn reaches_under(vars: &[(&str, &str)], settings: &SettingsEnv, windows: bool) -> bool {
    let env: Vec<(OsString, OsString)> = vars
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect();
    super::reaches(
        env.iter().map(|(n, v)| (n.as_os_str(), v.as_os_str())),
        settings,
        windows,
    )
}

/// Settings sources that add `pairs` (and are `unsure`).
fn from_settings(pairs: &[(&str, &str)], unsure: bool) -> SettingsEnv {
    SettingsEnv {
        pairs: pairs
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect(),
        unsure,
        content: !pairs.is_empty(),
        helper: false,
        without_remote: None,
    }
}

/// An endpoint with the first-party provider keeps the sign-in on; a cloud provider or no
/// endpoint does not.
#[test]
fn a_plan_keeps_the_sign_in_only_with_an_endpoint_and_the_first_party_provider() {
    assert!(plan_keeps_own_signin(&plan(true, None)));
    assert!(plan_keeps_own_signin(&plan(
        true,
        Some(PlanProvider::Anthropic)
    )));
    assert!(!plan_keeps_own_signin(&plan(
        true,
        Some(PlanProvider::Bedrock)
    )));
    assert!(!plan_keeps_own_signin(&plan(
        true,
        Some(PlanProvider::Vertex)
    )));
    assert!(!plan_keeps_own_signin(&plan(false, None)));
    assert!(!plan_keeps_own_signin(&plan(
        false,
        Some(PlanProvider::Bedrock)
    )));
}

/// Clauseo's own launch env: the empty key placeholder and the firm's base URL reach it.
#[test]
fn a_base_url_with_the_empty_key_placeholder_reaches_it() {
    let url = "https://gateway.firm.example";
    assert!(reaches(&[
        ("ANTHROPIC_BASE_URL", url),
        ("ANTHROPIC_API_KEY", "")
    ]));
    assert!(reaches(&[("ANTHROPIC_BASE_URL", url)]));
    assert!(reaches(&[
        ("ANTHROPIC_BASE_URL", url),
        ("CLAUDE_CODE_USE_BEDROCK", "0")
    ]));
}

/// Each switch on the 2.1.293 list that Clauseo can see turns it off; no base URL never reaches.
#[test]
fn a_key_a_token_or_a_cloud_provider_turns_it_off() {
    let url = ("ANTHROPIC_BASE_URL", "https://gateway.firm.example");
    assert!(!reaches(&[url, ("ANTHROPIC_API_KEY", "stored-key")]));
    assert!(!reaches(&[url, ("ANTHROPIC_AUTH_TOKEN", "t")]));
    for var in [
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
    ] {
        for on in ["1", "true", " YES ", "on"] {
            assert!(!reaches(&[url, (var, on)]), "{var}={on}");
        }
    }
    assert!(!reaches(&[
        ("ANTHROPIC_BASE_URL", ""),
        ("ANTHROPIC_API_KEY", "")
    ]));
    assert!(!reaches(&[("ANTHROPIC_API_KEY", "")]));
}

/// A name that is not UTF-8 is skipped, never a panic.
#[cfg(unix)]
#[test]
fn a_name_that_is_not_utf8_is_skipped() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;
    let odd = OsStr::from_bytes(b"\xff");
    let url = OsStr::new("https://gateway.firm.example");
    let env = [(odd, url), (OsStr::new("ANTHROPIC_BASE_URL"), url)];
    assert!(own_signin_reaches_endpoint(env, &SettingsEnv::default()));
}

/// The vendor's own address, in any spelling, is the default and no custom endpoint: no plan
/// refusal and no env refusal. Another host, path or port is one.
#[test]
fn the_default_address_in_any_spelling_is_no_custom_endpoint() {
    for url in [
        "https://api.anthropic.com",
        "https://api.anthropic.com/",
        "HTTPS://API.ANTHROPIC.COM",
        "https://Api.Anthropic.com/",
        "https://api.anthropic.com:443",
        "https://api.anthropic.com:443/",
    ] {
        assert!(is_default_address(url), "{url}");
        assert!(!reaches(&[("ANTHROPIC_BASE_URL", url)]), "{url}");
        assert!(!plan_keeps_own_signin(&plan_at(Some(url), None)), "{url}");
    }
    for url in [
        "http://api.anthropic.com",
        "https://api.anthropic.com:8443",
        "https://api.anthropic.com/v1",
        "https://api.anthropic.com.firm.example",
        "https://api.anthropic.com//",
        " https://api.anthropic.com",
    ] {
        assert!(!is_default_address(url), "{url}");
        assert!(reaches(&[("ANTHROPIC_BASE_URL", url)]), "{url}");
    }
}

/// A base URL or a key from the settings sources counts as the env's would, applied over it; an
/// unsure read counts as a base URL unless a key turns the sign-in off.
#[test]
fn the_settings_sources_env_applies_over_the_launch_env() {
    let url = ("ANTHROPIC_BASE_URL", "https://gateway.firm.example");
    let none = SettingsEnv::default();
    assert!(reaches_under(&[], &from_settings(&[url], false), false));
    assert!(reaches_under(
        &[("ANTHROPIC_API_KEY", "")],
        &from_settings(&[url], false),
        false
    ));
    assert!(!reaches_under(
        &[url],
        &from_settings(&[("ANTHROPIC_API_KEY", "set")], false),
        false
    ));
    assert!(reaches_under(
        &[url, ("ANTHROPIC_API_KEY", "k")],
        &from_settings(&[("ANTHROPIC_API_KEY", "")], false),
        false
    ));
    let default = ("ANTHROPIC_BASE_URL", "https://api.anthropic.com");
    assert!(!reaches_under(
        &[url],
        &from_settings(&[default], false),
        false
    ));
    assert!(reaches_under(&[], &from_settings(&[], true), false));
    assert!(!reaches_under(
        &[("ANTHROPIC_API_KEY", "k")],
        &from_settings(&[], true),
        false
    ));
    let bedrock = ("CLAUDE_CODE_USE_BEDROCK", "1");
    assert!(!reaches_under(
        &[],
        &from_settings(&[url, bedrock], true),
        false
    ));
    assert!(!reaches_under(&[], &none, false));
}

/// On Windows a name in any case is one, and the last spelling the spawn applies wins, as
/// `std::process::Command` keeps it; elsewhere each spelling is its own name.
#[test]
fn on_windows_the_last_spelling_of_a_name_wins() {
    let none = SettingsEnv::default();
    let url = ("ANTHROPIC_BASE_URL", "https://gateway.firm.example");
    let spelled = [url, ("ANTHROPIC_API_KEY", "k"), ("anthropic_api_key", "")];
    assert!(reaches_under(&spelled, &none, true));
    assert!(!reaches_under(&spelled, &none, false));
    let reversed = [url, ("anthropic_api_key", ""), ("ANTHROPIC_API_KEY", "k")];
    assert!(!reaches_under(&reversed, &none, true));
    let lower_url = [("anthropic_base_url", "https://gateway.firm.example")];
    assert!(reaches_under(&lower_url, &none, true));
    assert!(!reaches_under(&lower_url, &none, false));
    let unset = [url, ("Anthropic_Base_Url", "")];
    assert!(!reaches_under(&unset, &none, true));
}

/// A key or token of only white space is no key, as Claude Code trims them: in the launch env
/// and from the settings sources, under either name.
#[test]
fn a_blank_key_or_token_is_no_key() {
    let url = ("ANTHROPIC_BASE_URL", "https://gateway.firm.example");
    for name in ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"] {
        for blank in [" ", "\t", " \t ", "\u{FEFF}", "\u{FEFF}  ", " \u{FEFF}\t"] {
            assert!(reaches(&[url, (name, blank)]), "{name}={blank:?}");
            assert!(
                reaches_under(&[url], &from_settings(&[(name, blank)], false), false),
                "{name}={blank:?}"
            );
        }
        assert!(!reaches(&[url, (name, " k ")]), "{name}");
        // JavaScript's `trim` keeps U+0085, so to Claude Code that is a key.
        assert!(!reaches(&[url, (name, "\u{85}")]), "{name}");
    }
}

/// A provider switch is read as Claude Code's `isEnvTruthy` reads it, after JavaScript's `trim`:
/// U+FEFF around `1` is trimmed, U+0085 is not, so that switch leaves the sign-in on.
#[test]
fn a_provider_switch_is_trimmed_as_bun_trims() {
    let url = ("ANTHROPIC_BASE_URL", "https://gateway.firm.example");
    assert!(!reaches(&[
        url,
        ("CLAUDE_CODE_USE_BEDROCK", "\u{FEFF} 1\u{FEFF}")
    ]));
    assert!(reaches(&[url, ("CLAUDE_CODE_USE_BEDROCK", "\u{85}1")]));
    // An endpoint of only U+FEFF is still an endpoint: that errs toward refusing.
    assert!(reaches(&[("ANTHROPIC_BASE_URL", "\u{FEFF}")]));
    assert!(reaches(&[("ANTHROPIC_BASE_URL", "\u{FEFF} ")]));
}

/// `CLAUDE_CODE_API_BASE_URL` names the API's address as `ANTHROPIC_BASE_URL` does, and a unix
/// socket for the API counts as a custom endpoint; a key turns either off.
#[test]
fn the_api_base_url_and_a_unix_socket_count_as_an_endpoint() {
    let api = ("CLAUDE_CODE_API_BASE_URL", "https://api.firm.example");
    let socket = ("ANTHROPIC_UNIX_SOCKET", "proxy.sock");
    for var in [api, socket] {
        assert!(reaches(&[var]), "{var:?}");
        assert!(!reaches(&[var, ("ANTHROPIC_API_KEY", "k")]), "{var:?}");
        assert!(
            reaches_under(&[], &from_settings(&[var], false), false),
            "{var:?}"
        );
    }
    assert!(!reaches(&[(
        "CLAUDE_CODE_API_BASE_URL",
        "https://api.anthropic.com/"
    )]));
    assert!(!reaches(&[("ANTHROPIC_UNIX_SOCKET", "")]));
    assert!(reaches_under(
        &[("claude_code_api_base_url", "https://api.firm.example")],
        &SettingsEnv::default(),
        true
    ));
}

/// The trim is JavaScript's: exactly its white space and line ends go from both ends, every
/// other character stays (U+0085, U+180E and U+200B among them, which some other trims remove).
#[test]
fn the_trim_is_javascript_s() {
    let js: Vec<char> = [
        '\t', '\n', '\u{B}', '\u{C}', '\r', ' ', '\u{A0}', '\u{1680}', '\u{2028}', '\u{2029}',
        '\u{202F}', '\u{205F}', '\u{3000}', '\u{FEFF}',
    ]
    .into_iter()
    .chain('\u{2000}'..='\u{200A}')
    .collect();
    for c in (0..=0x3_FFFF).filter_map(char::from_u32) {
        let text = format!("{c}k{c}");
        let trimmed = if js.contains(&c) { "k" } else { text.as_str() };
        assert_eq!(js_trim(&text), trimmed, "U+{:04X}", u32::from(c));
    }
    assert_eq!(js_trim("\u{FEFF} \u{3000}k j\u{2028}\t"), "k j");
}
