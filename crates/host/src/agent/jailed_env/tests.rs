//! The jailed environment's rules (reviewer M3): table-driven, so a credential or an allowed name
//! is one line in its table.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use clauseo_settings::AgentLaunch;

use super::filters::{CREDENTIALS, Pattern};
use super::{JAILED_ENV, OS_PATH, Rule, host_vars, ignored, path, sources};
use crate::agent::{codex, home, launch_spec, loads};

/// One example per [`CREDENTIALS`] entry and every name the review listed: each set in the
/// lawyer's environment must be absent from a jailed spec.
const CREDENTIAL_EXAMPLES: [&str; 45] = [
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_CUSTOM_HEADERS",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_SKIP_BEDROCK_AUTH",
    "ANTHROPIC_BEDROCK_BASE_URL",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AWS_PROFILE",
    "AWS_REGION",
    "AWS_BEARER_TOKEN_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_SKIP_VERTEX_AUTH",
    "ANTHROPIC_VERTEX_PROJECT_ID",
    "ANTHROPIC_VERTEX_BASE_URL",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "GOOGLE_CLOUD_PROJECT",
    "CLOUD_ML_REGION",
    "VERTEX_REGION_CLAUDE_3_7_SONNET",
    "CLAUDE_CODE_USE_FOUNDRY",
    "CLAUDE_CODE_SKIP_FOUNDRY_AUTH",
    "ANTHROPIC_FOUNDRY_API_KEY",
    "ANTHROPIC_FOUNDRY_RESOURCE",
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
    "OPENAI_ORG_ID",
    "OPENAI_ORGANIZATION",
    "OPENAI_PROJECT",
    "CODEX_API_KEY",
    "AZURE_OPENAI_API_KEY",
    "AZURE_OPENAI_ENDPOINT",
    "AZURE_CLIENT_SECRET",
    "CLAUDE_CODE_CLIENT_KEY",
    "CLAUDE_CODE_CLIENT_CERT",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "NPM_TOKEN",
    "HF_TOKEN",
    // Any case: Windows ignores it.
    "openai_api_key",
    "Anthropic_Api_Key",
];

/// The lawyer's environment besides the credentials: the names a jailed launch keeps, and some
/// it drops though they look harmless.
const KEPT: [(&str, &str); 3] = [
    ("LANG", "en_GB.UTF-8"),
    ("LC_ALL", "en_GB.UTF-8"),
    ("HOME", "/home/ada"),
];
const KEPT_ON_WINDOWS: [(&str, &str); 3] = [
    ("TEMP", "C:\\Users\\ada\\AppData\\Local\\Temp"),
    ("TMP", "C:\\Users\\ada\\AppData\\Local\\Temp"),
    ("PATHEXT", ".COM;.EXE;.BAT;.CMD"),
];
const DROPPED: [(&str, &str); 10] = [
    ("PATH", "/home/ada/bin:/opt/homebrew/bin:/usr/bin"),
    ("SHELL", "/bin/zsh"),
    ("USER", "ada"),
    ("EDITOR", "vim"),
    ("SystemRoot", "C:\\Elsewhere"),
    ("GH_TOKEN", "ghp_secret"),
    ("NPM_TOKEN", "npm_secret"),
    ("HTTPS_PROXY", "http://lawyer-proxy:8080"),
    ("NODE_EXTRA_CA_CERTS", "/home/ada/ca.pem"),
    ("CODEX_HOME", "/home/ada/.codex"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Claude,
    Codex,
}

fn launch(env: &[(&str, &str)]) -> AgentLaunch {
    AgentLaunch {
        command: super::super::tests::absolute_program(),
        args: Vec::new(),
        env: env
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
    }
}

fn os(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    pairs
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

fn state() -> PathBuf {
    std::env::temp_dir().join("clauseo-jailed-env-state")
}

fn runtime() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/opt/node/bin"),
        PathBuf::from("/data/agents/claude/install"),
    ]
}

/// The jail's variables as `forced::jailed` puts them over the forced ones: the proxy's (with
/// `ALL_PROXY` removed from every source), the CA bundle's and `host_vars`.
fn jail_env() -> Vec<(OsString, Option<OsString>)> {
    let url = "http://clauseo:token@127.0.0.1:4100";
    let mut env: Vec<(OsString, Option<OsString>)> = [
        ("HTTPS_PROXY", Some(url)),
        ("HTTP_PROXY", Some(url)),
        ("https_proxy", Some(url)),
        ("http_proxy", Some(url)),
        ("NO_PROXY", Some("")),
        ("no_proxy", Some("")),
        ("ALL_PROXY", None),
        ("all_proxy", None),
        ("CODEX_CA_CERTIFICATE", Some("/data/run/ca-bundle.pem")),
        ("SSL_CERT_FILE", Some("/data/run/ca-bundle.pem")),
        ("NODE_EXTRA_CA_CERTS", Some("/data/run/ca-bundle.pem")),
    ]
    .into_iter()
    .map(|(name, value)| (OsString::from(name), value.map(OsString::from)))
    .collect();
    env.extend(host_vars(&runtime()));
    env
}

/// The forced variables of `kind`'s launch, the jail's over them, and the names to drop.
fn forced(kind: Kind) -> (BTreeMap<OsString, OsString>, Vec<String>) {
    let mut forced = match kind {
        Kind::Claude => {
            home::claude_launch_env(&state(), Some(Path::new("/data/agents/claude/home")))
        }
        Kind::Codex => {
            let mut env =
                codex::launch_env(&state(), &BTreeMap::new(), serde_json::Map::new(), None);
            env.insert(
                OsString::from(codex::TMPDIR_VAR),
                OsString::from("/data/run/tmp"),
            );
            env
        }
    };
    let mut dropped: Vec<String> = match kind {
        Kind::Claude => Vec::new(),
        Kind::Codex => codex::DROPPED_VARS
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
    };
    for (name, value) in jail_env() {
        if let Some(value) = value {
            forced.insert(name, value);
        } else {
            forced.remove(&name);
            dropped.push(name.into_string().unwrap());
        }
    }
    (forced, dropped)
}

/// The jailed spec's environment for `kind` from these sources, as the launch builds it.
fn jailed(
    kind: Kind,
    inherited: Vec<(OsString, OsString)>,
    settings: &AgentLaunch,
    hook: BTreeMap<OsString, OsString>,
) -> BTreeMap<String, String> {
    let sources = sources(inherited, settings, hook);
    let (forced, dropped) = forced(kind);
    let dropped: Vec<&str> = dropped.iter().map(String::as_str).collect();
    let spec = launch_spec(
        &sources.settings,
        loads::launch_env(sources.hook),
        forced,
        &dropped,
        sources.inherited.into_iter(),
        std::env::temp_dir(),
        None,
    )
    .unwrap();
    text(spec.env())
}

fn text(env: &BTreeMap<OsString, OsString>) -> BTreeMap<String, String> {
    env.iter()
        .map(|(name, value)| {
            (
                name.to_str().unwrap().to_owned(),
                value.to_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn has(env: &BTreeMap<String, String>, name: &str) -> bool {
    env.keys().any(|key| key.eq_ignore_ascii_case(name))
}

fn lawyers_env() -> Vec<(OsString, OsString)> {
    let mut env = os(&KEPT);
    env.extend(os(&KEPT_ON_WINDOWS));
    env.extend(os(&DROPPED));
    env.extend(
        CREDENTIAL_EXAMPLES
            .iter()
            .map(|name| (OsString::from(name), OsString::from("secret"))),
    );
    env
}

mod filters;

#[test]
fn every_credential_in_the_lawyer_s_environment_is_absent_from_a_jailed_claude_or_codex_spec() {
    for kind in [Kind::Claude, Kind::Codex] {
        let env = jailed(kind, lawyers_env(), &launch(&[]), BTreeMap::new());
        for name in CREDENTIAL_EXAMPLES {
            assert!(!has(&env, name), "{kind:?}: {name}");
        }
        assert!(!env.values().any(|value| value == "secret"), "{kind:?}");
    }
}

#[test]
fn every_credentials_entry_has_an_example_and_every_example_is_a_credential() {
    for name in CREDENTIAL_EXAMPLES {
        assert!(super::is_credential(OsStr::new(name)), "{name}");
    }
    for credential in CREDENTIALS {
        let covered = CREDENTIAL_EXAMPLES.iter().any(|name| match credential {
            Pattern::Name(known) => name.eq_ignore_ascii_case(known),
            Pattern::Prefix(prefix) => name.to_ascii_uppercase().starts_with(prefix),
        });
        assert!(covered, "{credential:?}");
    }
    // Not credentials: a settings model choice and a locale.
    for name in ["ANTHROPIC_MODEL", "CLAUDE_CODE_MAX_OUTPUT_TOKENS", "LANG"] {
        assert!(!super::is_credential(OsStr::new(name)), "{name}");
    }
}

/// The jailed spec holds exactly the allow-list's names: what the host sets for the agent's kind
/// and the jail, and from the lawyer's environment only the locale (and Claude's `HOME`), with
/// Windows' own on Windows.
#[test]
fn a_jailed_spec_holds_exactly_the_allow_listed_names() {
    let proxy = [
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "https_proxy",
        "http_proxy",
        "NO_PROXY",
        "no_proxy",
    ];
    let ca = [
        "CODEX_CA_CERTIFICATE",
        "SSL_CERT_FILE",
        "NODE_EXTRA_CA_CERTS",
    ];
    let mut common: Vec<&str> = proxy.iter().chain(&ca).copied().collect();
    common.extend([
        "PATH",
        "LANG",
        "LC_ALL",
        "HOME",
        "ENABLE_CLAUDEAI_MCP_SERVERS",
    ]);
    if cfg!(windows) {
        common.extend(["TEMP", "TMP", "PATHEXT", "SystemRoot", "windir"]);
        common.extend(super::system::SYSTEM_NAMES);
    }
    let claude = ["CLAUDE_CONFIG_DIR", "CLAUDE_SECURESTORAGE_CONFIG_DIR"];
    let codex = [
        "CODEX_HOME",
        "CODEX_CONFIG",
        "INITIAL_AGENT_MODE",
        "DISABLE_MCP_CONFIG_FILTERING",
        "TMPDIR",
    ];
    for (kind, own) in [(Kind::Claude, &claude[..]), (Kind::Codex, &codex[..])] {
        let env = jailed(kind, lawyers_env(), &launch(&[]), BTreeMap::new());
        let got: BTreeSet<&str> = env.keys().map(String::as_str).collect();
        let want: BTreeSet<&str> = common.iter().chain(own).copied().collect();
        assert_eq!(got, want, "{kind:?}");
        // Every name in it is on the allow-list, under the rule it came by.
        for name in &got {
            assert!(super::allowed(OsStr::new(name)).is_some(), "{name}");
        }
        // The locale is the lawyer's; the PATH is the runtime's and the OS's, never theirs.
        assert_eq!(env["LANG"], "en_GB.UTF-8");
        assert_eq!(OsString::from(&env["PATH"]), path(&runtime()).unwrap());
        assert!(!env["PATH"].contains("homebrew"), "{}", env["PATH"]);
        let home = if kind == Kind::Codex && cfg!(unix) {
            state().to_str().unwrap().to_owned()
        } else {
            "/home/ada".to_owned()
        };
        assert_eq!(env["HOME"], home, "{kind:?}");
    }
}

/// Every variable the host forces for either kind and the jail is a host-set name on the list,
/// so no hook or setting can override it and adding one without its line fails here.
#[test]
fn every_forced_and_jail_name_is_host_set_on_the_list() {
    for kind in [Kind::Claude, Kind::Codex] {
        let (forced, dropped) = forced(kind);
        let names = forced
            .keys()
            .cloned()
            .chain(dropped.into_iter().map(OsString::from));
        for name in names.filter(|name| {
            !codex::DROPPED_VARS
                .iter()
                .any(|drop| OsStr::new(drop) == name)
        }) {
            let rule = super::allowed(&name);
            assert!(
                matches!(rule, Some(Rule::Host | Rule::HostElseInherited)),
                "{name:?}: {rule:?}"
            );
        }
    }
    assert!(matches!(
        super::allowed(OsStr::new(loads::CLAUDE_AI_MCP_VAR)),
        Some(Rule::Host)
    ));
    // One line per name: no entry twice.
    let mut seen = BTreeSet::new();
    for entry in JAILED_ENV {
        assert!(
            seen.insert(entry.name.to_ascii_uppercase()),
            "{}",
            entry.name
        );
    }
}

#[test]
fn the_setup_hook_s_stored_key_and_plan_pass_but_not_over_a_host_set_name() {
    let hook = BTreeMap::from([
        (
            OsString::from("ANTHROPIC_API_KEY"),
            OsString::from("clauseo-stored-key"),
        ),
        (
            OsString::from("CLAUDE_CODE_USE_BEDROCK"),
            OsString::from("1"),
        ),
        (OsString::from("AWS_REGION"), OsString::from("eu-west-2")),
        (OsString::from("PATH"), OsString::from("/hook/bin")),
        (
            OsString::from("HTTPS_PROXY"),
            OsString::from("http://firm-proxy:3128"),
        ),
    ]);
    let env = jailed(Kind::Claude, lawyers_env(), &launch(&[]), hook);
    assert_eq!(env["ANTHROPIC_API_KEY"], "clauseo-stored-key");
    assert_eq!(env["CLAUDE_CODE_USE_BEDROCK"], "1");
    assert_eq!(env["AWS_REGION"], "eu-west-2");
    assert_eq!(OsString::from(&env["PATH"]), path(&runtime()).unwrap());
    assert!(
        env["HTTPS_PROXY"].starts_with("http://clauseo:"),
        "{}",
        env["HTTPS_PROXY"]
    );
    // Codex's stored key too.
    let hook = BTreeMap::from([(
        OsString::from("CODEX_API_KEY"),
        OsString::from("clauseo-key"),
    )]);
    let env = jailed(Kind::Codex, Vec::new(), &launch(&[]), hook);
    assert_eq!(env["CODEX_API_KEY"], "clauseo-key");
}

#[test]
fn a_credential_in_the_settings_is_dropped_when_jailed_and_passes_when_not() {
    let settings = launch(&[
        ("OPENAI_API_KEY", "settings-key"),
        ("ANTHROPIC_MODEL", "claude-opus"),
        ("PATH", "/settings/bin"),
        ("HOME", "/elsewhere"),
        ("CLAUDE_CONFIG_DIR", "/home/ada/.claude"),
    ]);
    for kind in [Kind::Claude, Kind::Codex] {
        let env = jailed(kind, lawyers_env(), &settings, BTreeMap::new());
        assert!(!has(&env, "OPENAI_API_KEY"), "{kind:?}");
        assert_eq!(env["ANTHROPIC_MODEL"], "claude-opus", "{kind:?}");
        assert_eq!(
            OsString::from(&env["PATH"]),
            path(&runtime()).unwrap(),
            "{kind:?}"
        );
        assert_ne!(env["HOME"], "/elsewhere", "{kind:?}");
        assert_ne!(
            env.get("CLAUDE_CONFIG_DIR").map(String::as_str),
            Some("/home/ada/.claude")
        );
    }
    // Unjailed (tier C), the settings' variables pass as they always have.
    let spec = launch_spec(
        &settings,
        BTreeMap::new(),
        BTreeMap::new(),
        &[],
        os(&[("PATH", "/usr/bin")]).into_iter(),
        std::env::temp_dir(),
        None,
    )
    .unwrap();
    let env = text(spec.env());
    assert_eq!(env["OPENAI_API_KEY"], "settings-key");
    assert_eq!(env["PATH"], "/settings/bin");
    assert_eq!(ignored(&settings.env), ["OPENAI_API_KEY"]);
}

#[test]
fn the_jailed_path_is_the_runtime_then_the_fixed_os_folders() {
    let joined = path(&runtime()).unwrap();
    #[cfg(not(windows))]
    let os: Vec<PathBuf> = ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
        .iter()
        .map(PathBuf::from)
        .collect();
    #[cfg(windows)]
    let os: Vec<PathBuf> = {
        let root = super::system_root().unwrap();
        vec![
            root.join("System32"),
            root.clone(),
            root.join("System32").join("Wbem"),
            root.join("System32").join("WindowsPowerShell").join("v1.0"),
        ]
    };
    let want = std::env::join_paths(runtime().into_iter().chain(os)).unwrap();
    assert_eq!(joined, want);
    for os in OS_PATH {
        assert!(!os.contains("homebrew") && !os.contains("local"), "{os}");
    }
}

/// Windows' system names (round 3b): the host's values on Windows, absent elsewhere, and never
/// the lawyer's, the hook's or the settings'.
#[test]
fn windows_system_names_are_the_host_s_and_no_other_source_s() {
    let evil = [
        ("ProgramFiles", "C:\\evil"),
        ("ComSpec", "C:\\evil\\cmd.exe"),
        ("NUMBER_OF_PROCESSORS", "999"),
        ("OS", "Evil_OS"),
    ];
    let mut inherited = lawyers_env();
    inherited.extend(os(&evil));
    let hook: BTreeMap<OsString, OsString> = os(&evil).into_iter().collect();
    let host: BTreeMap<String, String> = host_vars(&runtime())
        .into_iter()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value?.into_string().ok()?)))
        .collect();
    for kind in [Kind::Claude, Kind::Codex] {
        let env = jailed(kind, inherited.clone(), &launch(&evil), hook.clone());
        for name in super::system::SYSTEM_NAMES {
            if cfg!(windows) {
                assert_eq!(env.get(name), host.get(name), "{kind:?}: {name}");
                assert!(env.contains_key(name), "{kind:?}: {name}");
            } else {
                assert!(!has(&env, name), "{kind:?}: {name}");
            }
        }
        assert!(
            !env.values().any(|value| value.contains("evil")),
            "{kind:?}"
        );
        assert!(!env.values().any(|value| value == "999"), "{kind:?}");
    }
}

/// The profile names are host-set: the Windows runner puts them at the jailed home
/// (`set_by_runner`), and no inherited, hook or settings value reaches the spec before it.
#[test]
fn the_profile_names_come_from_no_source_but_the_runner() {
    let profile = [
        ("USERPROFILE", "C:\\Users\\ada"),
        ("APPDATA", "C:\\Users\\ada\\AppData\\Roaming"),
        ("LOCALAPPDATA", "C:\\Users\\ada\\AppData\\Local"),
        ("HOMEDRIVE", "C:"),
        ("HOMEPATH", "\\Users\\ada"),
    ];
    let mut inherited = lawyers_env();
    inherited.extend(os(&profile));
    let hook: BTreeMap<OsString, OsString> = os(&profile).into_iter().collect();
    for kind in [Kind::Claude, Kind::Codex] {
        let env = jailed(kind, inherited.clone(), &launch(&profile), hook.clone());
        for (name, _) in profile {
            assert_eq!(super::allowed(OsStr::new(name)), Some(Rule::Host), "{name}");
            assert!(!has(&env, name), "{kind:?}: {name}");
        }
    }
}
