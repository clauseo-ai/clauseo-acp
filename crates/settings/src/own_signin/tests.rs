//! When Claude Code's own sign-in would reach a custom endpoint, by a plan and by a whole env.

use std::collections::BTreeMap;
use std::ffi::OsString;

use super::{own_signin_reaches_endpoint, plan_keeps_own_signin};
use crate::schema::{Plan, PlanProvider};

fn plan(endpoint: bool, provider: Option<PlanProvider>) -> Plan {
    Plan {
        endpoint: endpoint.then(|| "https://gateway.firm.example".to_owned()),
        provider,
        env: BTreeMap::new(),
    }
}

fn reaches(vars: &[(&str, &str)]) -> bool {
    let env: BTreeMap<OsString, OsString> = vars
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect();
    own_signin_reaches_endpoint(env.iter().map(|(n, v)| (n.as_os_str(), v.as_os_str())))
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
    assert!(own_signin_reaches_endpoint(env));
}
