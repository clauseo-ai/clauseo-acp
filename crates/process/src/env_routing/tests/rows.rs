//! #1167's table rows, on the inherited side, per system: host's
//! `routing_points_where_it_did_before_the_scrub` (`agent/tests/scrub.rs` at `lw-review-fixes`),
//! with each row marked for the systems it holds on.

use std::collections::BTreeMap;
use std::ffi::OsString;

use super::super::{governed, kept_inherited};
use super::pairs;

/// The systems a row holds on: names are exact on macOS and Linux, any case on Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum On {
    Every,
    Windows,
    MacLinux,
}

impl On {
    fn here(self) -> bool {
        match self {
            On::Every => true,
            On::Windows => cfg!(windows),
            On::MacLinux => cfg!(not(windows)),
        }
    }
}

type Pairs = &'static [(&'static str, &'static str)];

/// One row of #1167's table: what it shows, where it holds, the inherited names, the hook's (a
/// firm plan's), and every name the rule governs that the launch holds.
type Row = (&'static str, On, Pairs, Pairs, Pairs);

/// The entry's blank key, which every launch holds unless the hook sets the stored key.
const BLANK: (&str, &str) = ("ANTHROPIC_API_KEY", "");

const BASE: (&str, &str) = ("ANTHROPIC_BASE_URL", "https://gw.firm.example");

const TOKEN: (&str, &str) = ("ANTHROPIC_AUTH_TOKEN", "someone-elses-token");

const VERTEX: (&str, &str) = ("ANTHROPIC_VERTEX_BASE_URL", "https://vertex.firm.example");

const BEDROCK_COMPANIONS: Pairs = &[
    ("ANTHROPIC_BEDROCK_REGION_PREFIX", "eu"),
    ("CLAUDE_CODE_SKIP_BEDROCK_AUTH", "1"),
    ("ANTHROPIC_CUSTOM_HEADERS", "X-Firm: 1"),
    (
        "ANTHROPIC_MODEL",
        "arn:aws:bedrock:eu-west-1:1:inference-profile/firm",
    ),
];

/// The rows. `ANTHROPIC_API_KEY` is the settings entry's `""` unless the hook sets the stored key:
/// an inherited key never wins over it.
const ROWS: [Row; 21] = [
    (
        "an inherited endpoint with its token",
        On::Every,
        &[BASE, ("ANTHROPIC_AUTH_TOKEN", "firm-token")],
        &[],
        &[BLANK, ("ANTHROPIC_AUTH_TOKEN", "firm-token"), BASE],
    ),
    (
        "an inherited endpoint with no token (#1152 refuses it while no key is held)",
        On::Every,
        &[BASE],
        &[],
        &[BLANK, BASE],
    ),
    (
        "a firm plan's endpoint and the stored key, over an inherited one",
        On::Every,
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
    ("none", On::Every, &[], &[], &[BLANK]),
    (
        "an inherited account token: dropped, so Clauseo's own sign-in is used",
        On::Every,
        &[
            ("CLAUDE_CODE_OAUTH_TOKEN", "sk-ant-oat-firm"),
            ("CLAUDE_CODE_OAUTH_REFRESH_TOKEN", "rt-firm"),
        ],
        &[],
        &[BLANK],
    ),
    (
        "a lone token with no endpoint: an account swap, dropped",
        On::Every,
        &[TOKEN],
        &[],
        &[BLANK],
    ),
    (
        "an inherited key beside its endpoint: the entry's empty key still wins",
        On::Every,
        &[BASE, ("ANTHROPIC_API_KEY", "sk-inherited")],
        &[],
        &[BLANK, BASE],
    ),
    (
        "a provider switch with its companions: all passed",
        On::Every,
        &[
            ("CLAUDE_CODE_USE_BEDROCK", " True "),
            BEDROCK_COMPANIONS[0],
            BEDROCK_COMPANIONS[1],
            BEDROCK_COMPANIONS[2],
            BEDROCK_COMPANIONS[3],
        ],
        &[],
        &[
            BLANK,
            BEDROCK_COMPANIONS[0],
            BEDROCK_COMPANIONS[2],
            BEDROCK_COMPANIONS[1],
            BEDROCK_COMPANIONS[3],
            ("CLAUDE_CODE_USE_BEDROCK", " True "),
        ],
    ),
    (
        "a lower-case provider switch with its companions: Windows reads it, all passed",
        On::Windows,
        &[
            ("claude_code_use_bedrock", " True "),
            BEDROCK_COMPANIONS[0],
            BEDROCK_COMPANIONS[1],
            BEDROCK_COMPANIONS[2],
            BEDROCK_COMPANIONS[3],
        ],
        &[],
        &[
            BLANK,
            BEDROCK_COMPANIONS[0],
            BEDROCK_COMPANIONS[2],
            BEDROCK_COMPANIONS[1],
            BEDROCK_COMPANIONS[3],
            ("claude_code_use_bedrock", " True "),
        ],
    ),
    (
        "a lower-case provider switch: Claude Code does not read it, so no route, all dropped",
        On::MacLinux,
        &[
            ("claude_code_use_bedrock", " True "),
            BEDROCK_COMPANIONS[0],
            BEDROCK_COMPANIONS[1],
            BEDROCK_COMPANIONS[2],
            BEDROCK_COMPANIONS[3],
        ],
        &[],
        &[BLANK],
    ),
    (
        "companions with no route (a switch that is off, an empty endpoint): dropped",
        On::Every,
        &[
            ("CLAUDE_CODE_USE_VERTEX", "0"),
            ("ANTHROPIC_BASE_URL", ""),
            ("CLAUDE_CODE_USE_GATEWAY", "1"),
            TOKEN,
            ("ANTHROPIC_CUSTOM_HEADERS", "X-Other: 1"),
            ("ANTHROPIC_VERTEX_PROJECT_ID", "other-project"),
            ("ANTHROPIC_MODEL", "claude-other"),
            ("CLAUDE_CODE_CLIENT_CERT", "/tmp/other.pem"),
        ],
        &[],
        &[
            BLANK,
            ("ANTHROPIC_BASE_URL", ""),
            ("CLAUDE_CODE_USE_VERTEX", "0"),
        ],
    ),
    (
        "a provider address with no switch: passed, but no route, so the token drops",
        On::Every,
        &[VERTEX, TOKEN],
        &[],
        &[BLANK, VERTEX],
    ),
    (
        "a provider address with its switch off: no route, so the token drops",
        On::Every,
        &[VERTEX, ("CLAUDE_CODE_USE_VERTEX", "0"), TOKEN],
        &[],
        &[BLANK, VERTEX, ("CLAUDE_CODE_USE_VERTEX", "0")],
    ),
    (
        "a provider address with its switch on: a route, so the token passes",
        On::Every,
        &[VERTEX, ("CLAUDE_CODE_USE_VERTEX", "1"), TOKEN],
        &[],
        &[BLANK, TOKEN, VERTEX, ("CLAUDE_CODE_USE_VERTEX", "1")],
    ),
    (
        "an empty endpoint with a token: no route, so the token drops",
        On::Every,
        &[("ANTHROPIC_BASE_URL", ""), TOKEN],
        &[],
        &[BLANK, ("ANTHROPIC_BASE_URL", "")],
    ),
    (
        "an endpoint of only spaces with a token: blank, no route, so the token drops",
        On::Every,
        &[("ANTHROPIC_BASE_URL", "   "), TOKEN],
        &[],
        &[BLANK, ("ANTHROPIC_BASE_URL", "   ")],
    ),
    (
        "the nonessential-traffic switch with no route: kept; the admin-union switch: dropped",
        On::Every,
        &[
            ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
            ("CLAUDE_CODE_DISABLE_ADMIN_ENV_UNION", "1"),
        ],
        &[],
        &[BLANK, ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")],
    ),
    (
        "the first-party assumption with no route: dropped",
        On::Every,
        &[("_CLAUDE_CODE_ASSUME_FIRST_PARTY_BASE_URL", "1")],
        &[],
        &[BLANK],
    ),
    (
        "the first-party assumption beside an endpoint: kept",
        On::Every,
        &[BASE, ("_CLAUDE_CODE_ASSUME_FIRST_PARTY_BASE_URL", "1")],
        &[],
        &[
            BLANK,
            BASE,
            ("_CLAUDE_CODE_ASSUME_FIRST_PARTY_BASE_URL", "1"),
        ],
    ),
    (
        "a lower-case endpoint with a token in another case: Windows reads both, both passed",
        On::Windows,
        &[
            ("anthropic_base_url", "https://gw.firm.example"),
            ("Anthropic_Auth_Token", "firm-token"),
        ],
        &[],
        &[
            BLANK,
            ("Anthropic_Auth_Token", "firm-token"),
            ("anthropic_base_url", "https://gw.firm.example"),
        ],
    ),
    (
        "a lower-case endpoint with a token: Claude Code does not read it, so no route, dropped",
        On::MacLinux,
        &[("anthropic_base_url", "https://gw.firm.example"), TOKEN],
        &[],
        &[BLANK],
    ),
];

/// The Windows-only rows on how spellings fold, kept apart so the table above stays host's.
const FOLDING_ROWS: [Row; 2] = [
    (
        "two spellings of the endpoint that disagree: neither kept, no route, the token drops",
        On::Windows,
        &[BASE, ("anthropic_base_url", ""), TOKEN],
        &[],
        &[BLANK],
    ),
    (
        "an endpoint spelled with a dotless i (U+0131): under the rule, unlisted, dropped",
        On::Windows,
        &[
            ("ANTHROP\u{131}C_BASE_URL", "https://gw.firm.example"),
            TOKEN,
        ],
        &[],
        &[BLANK],
    ),
];

/// A launch as host builds one, for its routing names: [`kept_inherited`] over the inherited
/// environment, then the settings entry's `ANTHROPIC_API_KEY: ""`, then the hook's names, each
/// over any inherited spelling of its name. Every name the rule governs.
fn launch(inherited: &[(&str, &str)], hook: &[(&str, &str)]) -> BTreeMap<String, String> {
    let mut env: Vec<(OsString, OsString)> = kept_inherited(pairs(inherited));
    for (name, value) in [BLANK].iter().chain(hook) {
        env.retain(|(key, _)| !key.eq_ignore_ascii_case(name));
        env.push((OsString::from(name), OsString::from(value)));
    }
    env.into_iter()
        .filter(|(name, _)| governed(name))
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect()
}

/// Host's `routing_points_where_it_did_before_the_scrub`, row for row, on this system's rows:
/// every name the rule governs that a launch holds. A name that only swaps the account never
/// gets through, nor does a companion with no route; the entry's empty key and the hook's names
/// win.
#[test]
fn routing_points_where_it_did_before_the_scrub() {
    let mut ran = 0;
    for (row, on, inherited, hook, expected) in ROWS.iter().chain(&FOLDING_ROWS) {
        if !on.here() {
            continue;
        }
        ran += 1;
        let expected: BTreeMap<String, String> = expected
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        assert_eq!(launch(inherited, hook), expected, "{row}");
    }
    assert_eq!(ran, if cfg!(windows) { 21 } else { 19 });
}
