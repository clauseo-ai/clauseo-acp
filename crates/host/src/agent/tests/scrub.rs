//! No `CLAUDE_*`, `ANTHROPIC_*` or `IS_SANDBOX` from the app's own environment reaches an agent:
//! only the ones Clauseo sets itself (`agent::spec::SCRUBBED_PREFIXES`), and the firm's routing
//! (`agent::spec::ROUTING_KEPT`).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use clauseo_settings::AgentLaunch;

use super::absolute_program;
use crate::agent::launch_spec;

/// Whether `name` is one the scrub is about.
fn scrubbed(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.starts_with("CLAUDE_") || upper.starts_with("ANTHROPIC_") || upper == "IS_SANDBOX"
}

/// A Claude launch as the host builds it (routing names Clauseo sets win over inherited ones):
/// the home and temp folder forced, the sign-in's variables in the agent's settings entry, the
/// stored key and the firm plan's in the hook. With a pile of loosening variables inherited, in
/// any case, every name with those prefixes the spec holds is one Clauseo set, with Clauseo's
/// value; the rest of the inherited env stays.
#[test]
fn only_the_claude_and_anthropic_variables_clauseo_sets_reach_the_agent() {
    let home = Path::new("/data/agents/claude-acp/home");
    let agent = AgentLaunch {
        command: absolute_program(),
        args: Vec::new(),
        env: BTreeMap::from([
            ("ANTHROPIC_API_KEY".to_owned(), String::new()),
            (
                "CLAUDE_CONFIG_DIR".to_owned(),
                "/data/agents/claude-acp/home".to_owned(),
            ),
            (
                "CLAUDE_SECURESTORAGE_CONFIG_DIR".to_owned(),
                "/data/store".to_owned(),
            ),
        ]),
    };
    let mut forced = crate::agent::home::claude_launch_env(home);
    forced.extend(crate::agent::temp::session_env(
        Path::new("/data/run/s1"),
        Some(Path::new("/tmp/c1")),
        false,
    ));
    let hook = crate::agent::loads::launch_env(BTreeMap::from([
        (
            OsString::from("ANTHROPIC_API_KEY"),
            OsString::from("sk-stored"),
        ),
        (
            OsString::from("ANTHROPIC_BASE_URL"),
            OsString::from("https://firm.example"),
        ),
        (
            OsString::from("CLAUDE_CODE_USE_BEDROCK"),
            OsString::from("1"),
        ),
    ]));
    let planted = [
        "IS_SANDBOX",
        "is_sandbox",
        "CLAUDE_CODE_BUBBLEWRAP",
        "CLAUDE_CODE_SANDBOXED",
        "CLAUDE_BG_SESSION_PERMISSION_RULES",
        "CLAUDE_CODE_SESSION_KIND",
        "CLAUDE_CODE_AUTO_MODE_SERVER",
        "CLAUDE_CODE_DISABLE_PERMISSION_PROMPT_NOTIFY_HOOKS",
        "CLAUDE_CODE_DISABLE_HOOK_FORWARDING",
        "CLAUDE_CODE_SESSIONEND_HOOKS_TIMEOUT_MS",
        "CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD",
        "CLAUDE_CODE_USE_COWORK_PLUGINS",
        "CLAUDE_CODE_REMOTE",
        "CLAUDE_CONFIG_DIR",
        "Claude_Code_Tmpdir",
        "CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "ENABLE_CLAUDEAI_MCP_SERVERS",
    ];
    let inherited = planted
        .iter()
        .map(|name| (OsString::from(name), OsString::from("planted")))
        .chain([
            // unix-paths-ok: a planted settings or env value; it never reaches a path API
            (OsString::from("HOME"), OsString::from("/home/ada")),
            // unix-paths-ok: a planted settings or env value; it never reaches a path API
            (OsString::from("PATH"), OsString::from("/usr/bin")),
            (
                OsString::from("CLAUDEISH"),
                OsString::from("kept: no prefix"),
            ),
        ]);
    let spec = launch_spec(
        &agent,
        hook,
        forced,
        &[],
        inherited,
        std::env::temp_dir(),
        &[],
    )
    .unwrap();
    let ours: BTreeMap<&str, &str> = spec
        .env()
        .iter()
        .map(|(k, v)| (k.to_str().unwrap(), v.to_str().unwrap()))
        .filter(|(name, _)| scrubbed(name))
        .collect();
    let expected = BTreeMap::from([
        ("ANTHROPIC_API_KEY", "sk-stored"),
        ("ANTHROPIC_BASE_URL", "https://firm.example"),
        ("CLAUDE_CODE_TMPDIR", "/tmp/c1"),
        ("CLAUDE_CODE_USE_BEDROCK", "1"),
        ("CLAUDE_CONFIG_DIR", "/data/agents/claude-acp/home"),
        ("CLAUDE_SECURESTORAGE_CONFIG_DIR", "/data/store"),
    ]);
    assert_eq!(ours, expected);
    let env = spec.env();
    assert!(!env.values().any(|value| value == "planted"));
    assert_eq!(
        env.get(&OsString::from("ENABLE_CLAUDEAI_MCP_SERVERS"))
            .unwrap(),
        "false"
    );
    assert_eq!(
        env.get(&OsString::from("CLAUDEISH")).unwrap(),
        "kept: no prefix"
    );
    // unix-paths-ok: a planted settings or env value; it never reaches a path API
    assert_eq!(env.get(&OsString::from("HOME")).unwrap(), "/home/ada");
}

/// A Codex agent loses nothing it reads to the scrub: none of its names has those prefixes.
#[test]
fn codex_reads_none_of_the_scrubbed_names() {
    let names = crate::agent::codex::DROPPED_VARS.iter().chain(&[
        "CODEX_HOME",
        "CODEX_CONFIG",
        "CODEX_API_KEY",
        "OPENAI_BASE_URL",
    ]);
    for name in names {
        assert!(
            !crate::agent::spec::is_dropped_inherited(std::ffi::OsStr::new(name), false),
            "{name}"
        );
    }
    assert_eq!(
        crate::agent::spec::SCRUBBED_PREFIXES,
        ["CLAUDE_", "ANTHROPIC_"]
    );
    assert_eq!(crate::agent::spec::SCRUBBED.len(), 8);
}

/// The names outside both prefixes on the scrub's list (`SCRUBBED`) drop, inherited, in any case,
/// route or not; the firm's proxy and CA stay.
#[test]
fn the_loosening_names_outside_the_prefixes_drop_and_the_firms_proxy_and_ca_stay() {
    use std::ffi::OsStr;

    use crate::agent::spec::is_dropped_inherited;
    for name in [
        "IS_SANDBOX",
        "NODE_TLS_REJECT_UNAUTHORIZED",
        "node_tls_reject_unauthorized",
        "SESSION_INGRESS_URL",
        "AGENT_PROXY_URL",
        "AGENT_PROXY_AUTH_TOKEN",
        "SELF_HOSTED_RUNNER_TRUST_WORKSPACE",
        "SELF_HOSTED_RUNNER_HOOKS_DIR",
        "SELF_HOSTED_RUNNER_CONFINE_REPO_SETTINGS",
    ] {
        for route in [true, false] {
            assert!(
                is_dropped_inherited(OsStr::new(name), route),
                "{name} drops"
            );
        }
    }
    for name in [
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "NODE_EXTRA_CA_CERTS",
        "SSL_CERT_FILE",
    ] {
        assert!(
            !is_dropped_inherited(OsStr::new(name), false),
            "{name} stays"
        );
    }
}

/// A route (`ROUTES`) passes on its own, in any case; every other name on `ROUTING_KEPT` passes
/// only beside a route; every other inherited `CLAUDE_*` and `ANTHROPIC_*` name drops, in any
/// case, route or not.
#[test]
fn only_the_listed_routing_names_pass_and_every_other_prefixed_name_drops() {
    use std::ffi::OsStr;

    use crate::agent::spec::{ROUTES, ROUTING_KEPT, is_dropped_inherited};
    for name in ROUTING_KEPT {
        let lower = name.to_ascii_lowercase();
        for name in [name, lower.as_str()] {
            let route = ROUTES
                .iter()
                .any(|listed| listed.eq_ignore_ascii_case(name));
            assert!(
                !is_dropped_inherited(OsStr::new(name), true),
                "{name} passes"
            );
            assert_eq!(
                is_dropped_inherited(OsStr::new(name), false),
                !route,
                "{name}"
            );
        }
    }
    // Names Claude Code 2.1.293 reads that are not routing, names left out on purpose
    // (`agent/spec/routing.rs`), and near-misses of listed names: none passes on a pattern.
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
        "IS_SANDBOX",
    ] {
        assert!(is_dropped_inherited(OsStr::new(name), true), "{name} drops");
        assert!(!ROUTING_KEPT.contains(&name), "{name} is not on the list");
    }
}

/// `CLAUDE_PROJECT_UUID` names a claude.ai Project (2.1.293's project context block);
/// `CLAUDE_CODE_BRIEF` and `CLAUDE_CODE_PEWTER_OWL_TOOL` each offer its `SendUserMessage` tool.
/// Clauseo never sets them, and an inherited one drops, with a route or without:
/// none is routing, so the `CLAUDE_` prefix scrub takes it (a policy's `env` setting one
/// refuses the launch, `weakens.rs`).
#[test]
fn an_inherited_project_id_or_brief_switch_never_reaches_the_agent() {
    use std::ffi::OsStr;

    use crate::agent::spec::{ROUTING_KEPT, is_dropped_inherited};

    for name in [
        "CLAUDE_PROJECT_UUID",
        "CLAUDE_CODE_BRIEF",
        "CLAUDE_CODE_PEWTER_OWL_TOOL",
    ] {
        assert!(!ROUTING_KEPT.contains(&name), "{name}");
        for route in [false, true] {
            let dropped = is_dropped_inherited(OsStr::new(name), route);
            assert!(dropped, "{name}, route {route}");
        }
    }
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

/// The table in #1167's description (`ROWS`): every `CLAUDE_*` and `ANTHROPIC_*` name a launch
/// holds. The routing rows are what main sends (main drops no `ANTHROPIC_*` or `CLAUDE_*` name),
/// and the hook's names win over inherited ones. A name that only swaps the account (an OAuth
/// token, a lone bearer token) never reaches the agent, which signs in with Clauseo's own login
/// in its home (`CLAUDE_CONFIG_DIR`, forced in every row); nor does a companion with no route.
#[test]
fn routing_points_where_it_did_before_the_scrub() {
    let home = std::path::Path::new("/data/agents/claude-acp/home");
    for (row, inherited, hook, expected) in ROWS {
        let agent = AgentLaunch {
            command: absolute_program(),
            args: Vec::new(),
            env: BTreeMap::from([("ANTHROPIC_API_KEY".to_owned(), String::new())]),
        };
        let hook = crate::agent::loads::launch_env(
            hook.iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value)))
                .collect(),
        );
        let inherited: Vec<(OsString, OsString)> = inherited
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect();
        let spec = launch_spec(
            &agent,
            hook,
            crate::agent::home::claude_launch_env(home),
            &[],
            inherited.into_iter(),
            std::env::temp_dir(),
            &[],
        )
        .unwrap();
        let config = spec.env().get(&OsString::from("CLAUDE_CONFIG_DIR"));
        assert_eq!(config, Some(&home.as_os_str().to_owned()), "{row}");
        let routing: BTreeMap<String, String> = spec
            .env()
            .iter()
            .filter_map(|(name, value)| {
                Some((name.to_str()?.to_owned(), value.to_str()?.to_owned()))
            })
            .filter(|(name, _)| scrubbed(name) && name != "CLAUDE_CONFIG_DIR")
            .collect();
        let expected: BTreeMap<String, String> = expected
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        assert_eq!(routing, expected, "{row}");
    }
}
