//! The environment builder: every stripped name is gone, and each run kind gets its own
//! variables and no others.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use super::{EnvInputs, RunKind, Shim, Vendor, codex_store_args, run_env};
use crate::platform::Os;

/// The test's "own environment": a few ordinary variables plus every name a run must strip,
/// some in another case.
fn base() -> BTreeMap<OsString, OsString> {
    [
        ("PATH", "/usr/bin"),
        ("HOME", "/home/lawyer"),
        ("LANG", "en_GB.UTF-8"),
        ("SSH_CONNECTION", "10.0.0.1 22 10.0.0.2 5555"),
        ("SSH_TTY", "/dev/pts/1"),
        ("SSH_AUTH_SOCK", "/tmp/agent.sock"),
        ("ssh_client", "lowercase"),
        ("NO_BROWSER", "1"),
        ("CLAUDE_CODE_REMOTE", "true"),
        ("CLAUDE_CONFIG_DIR", "/home/lawyer/.claude-alt"),
        ("CLAUDE_SECURESTORAGE_CONFIG_DIR", "/home/lawyer/.store"),
        ("CODEX_HOME", "/home/lawyer/.codex-alt"),
        ("CLAUSEO_SIGNIN_URL_FILE", "/tmp/stale"),
        ("anthropic_api_key", "sk-stray"),
        ("CODEX_API_KEY", "sk-stray"),
        ("BROWSER", "firefox"),
    ]
    .into_iter()
    .map(|(name, value)| (OsString::from(name), OsString::from(value)))
    .collect()
}

fn inputs<'a>(
    base: &'a BTreeMap<OsString, OsString>,
    vendor: Vendor,
    kind: RunKind<'a>,
) -> EnvInputs<'a> {
    EnvInputs {
        base,
        vendor,
        kind,
        os: Os::Linux,
        proxy: None,
        ca_file: None,
        shim: None,
    }
}

fn get<'a>(env: &'a BTreeMap<OsString, OsString>, name: &str) -> Option<&'a str> {
    env.get(&OsString::from(name))
        .and_then(|value| value.to_str())
}

fn names(env: &BTreeMap<OsString, OsString>) -> Vec<String> {
    env.keys()
        .map(|name| name.to_string_lossy().into_owned())
        .collect()
}

/// The names no run ever passes on from the base, whatever their case.
const NEVER: [&str; 5] = [
    "SSH_CONNECTION",
    "SSH_TTY",
    "SSH_AUTH_SOCK",
    "NO_BROWSER",
    "CLAUDE_CODE_REMOTE",
];

fn assert_stripped(env: &BTreeMap<OsString, OsString>) {
    for name in names(env) {
        let upper = name.to_ascii_uppercase();
        assert!(!upper.starts_with("SSH_"), "{name} reached the child");
        assert!(!NEVER.contains(&upper.as_str()), "{name} reached the child");
    }
    assert_eq!(get(env, "PATH"), Some("/usr/bin"));
    assert_eq!(get(env, "LANG"), Some("en_GB.UTF-8"));
}

#[test]
fn every_run_strips_the_remote_hints() {
    let base = base();
    let home = Path::new("/data/agents/x/home");
    for vendor in [Vendor::Claude, Vendor::Codex] {
        for kind in [
            RunKind::Detect,
            RunKind::Own { home },
            RunKind::Reused { home },
        ] {
            assert_stripped(&run_env(&inputs(&base, vendor, kind)));
        }
    }
}

#[test]
fn detection_runs_have_no_home_variables() {
    let base = base();
    for vendor in [Vendor::Claude, Vendor::Codex] {
        let env = run_env(&inputs(&base, vendor, RunKind::Detect));
        for name in [
            "CLAUDE_CONFIG_DIR",
            "CLAUDE_SECURESTORAGE_CONFIG_DIR",
            "CODEX_HOME",
            "CLAUSEO_SIGNIN_URL_FILE",
        ] {
            assert_eq!(
                get(&env, name),
                None,
                "{name} in a {vendor:?} detection run"
            );
        }
        assert_eq!(get(&env, "HOME"), Some("/home/lawyer"), "HOME moved");
    }
    assert_eq!(codex_store_args(RunKind::Detect), Vec::<OsString>::new());
}

#[test]
fn claude_runs_get_their_home_and_never_a_new_home_folder() {
    let base = base();
    let home = Path::new("/data/agents/claude-acp/home");
    let own = run_env(&inputs(&base, Vendor::Claude, RunKind::Own { home }));
    assert_eq!(
        get(&own, "CLAUDE_CONFIG_DIR"),
        Some("/data/agents/claude-acp/home")
    );
    assert_eq!(get(&own, "CLAUDE_SECURESTORAGE_CONFIG_DIR"), None);
    assert_eq!(get(&own, "HOME"), Some("/home/lawyer"));
    let reused = run_env(&inputs(&base, Vendor::Claude, RunKind::Reused { home }));
    assert_eq!(
        get(&reused, "CLAUDE_CONFIG_DIR"),
        Some("/data/agents/claude-acp/home")
    );
    assert_eq!(get(&reused, "CLAUDE_SECURESTORAGE_CONFIG_DIR"), Some(""));
    assert_eq!(get(&reused, "HOME"), Some("/home/lawyer"));
    for env in [&own, &reused] {
        assert_eq!(get(env, "CODEX_HOME"), None);
        // The stray key in another case is replaced by one empty spelling.
        assert_eq!(get(env, "ANTHROPIC_API_KEY"), Some(""));
        assert_eq!(get(env, "anthropic_api_key"), None);
    }
}

#[test]
fn codex_runs_but_detection_get_their_home_and_the_file_store() {
    let base = base();
    let home = Path::new("/data/agents/codex-acp/home");
    for kind in [RunKind::Own { home }, RunKind::Reused { home }] {
        for os in [Os::Linux, Os::Darwin] {
            let env = run_env(&EnvInputs {
                os,
                ..inputs(&base, Vendor::Codex, kind)
            });
            assert_eq!(get(&env, "CODEX_HOME"), Some("/data/agents/codex-acp/home"));
            assert_eq!(get(&env, "HOME"), Some("/data/agents/codex-acp/home"));
            assert_eq!(get(&env, "CODEX_API_KEY"), Some(""));
            assert_eq!(get(&env, "CLAUDE_CONFIG_DIR"), None);
        }
        let windows = run_env(&EnvInputs {
            os: Os::Win32,
            ..inputs(&base, Vendor::Codex, kind)
        });
        assert_eq!(
            get(&windows, "HOME"),
            Some("/home/lawyer"),
            "HOME moved on Windows"
        );
        assert_eq!(
            codex_store_args(kind),
            [
                OsString::from("-c"),
                OsString::from("cli_auth_credentials_store=\"file\"")
            ]
        );
    }
    let detect = run_env(&inputs(&base, Vendor::Codex, RunKind::Detect));
    assert_eq!(get(&detect, "CODEX_API_KEY"), Some(""));
}

#[test]
fn proxy_and_ca_go_to_each_vendor_s_own_variable() {
    let base = base();
    let ca = Path::new("/firm/ca.pem");
    let with = |vendor| {
        run_env(&EnvInputs {
            proxy: Some("http://proxy.firm:8080"),
            ca_file: Some(ca),
            ..inputs(&base, vendor, RunKind::Detect)
        })
    };
    let claude = with(Vendor::Claude);
    assert_eq!(get(&claude, "NODE_EXTRA_CA_CERTS"), Some("/firm/ca.pem"));
    assert_eq!(get(&claude, "SSL_CERT_FILE"), None);
    assert_eq!(get(&claude, "HTTPS_PROXY"), Some("http://proxy.firm:8080"));
    let codex = with(Vendor::Codex);
    assert_eq!(get(&codex, "SSL_CERT_FILE"), Some("/firm/ca.pem"));
    assert_eq!(get(&codex, "NODE_EXTRA_CA_CERTS"), None);
    assert_eq!(get(&codex, "HTTPS_PROXY"), Some("http://proxy.firm:8080"));
    let none = run_env(&inputs(&base, Vendor::Codex, RunKind::Detect));
    assert_eq!(get(&none, "SSL_CERT_FILE"), None);
    assert_eq!(get(&none, "HTTPS_PROXY"), None);
}

#[test]
fn the_shim_is_claude_s_alone() {
    let base = base();
    let home = Path::new("/data/agents/claude-acp/home");
    let shim = Shim {
        browser: Path::new("/data/agents/signin/browser"),
        url_file: Path::new("/data/agents/signin/url-1"),
    };
    let claude = run_env(&EnvInputs {
        shim: Some(shim),
        ..inputs(&base, Vendor::Claude, RunKind::Own { home })
    });
    assert_eq!(get(&claude, "BROWSER"), Some("/data/agents/signin/browser"));
    assert_eq!(
        get(&claude, "CLAUSEO_SIGNIN_URL_FILE"),
        Some("/data/agents/signin/url-1")
    );
    let codex = run_env(&EnvInputs {
        shim: Some(shim),
        ..inputs(&base, Vendor::Codex, RunKind::Own { home })
    });
    assert_eq!(
        get(&codex, "BROWSER"),
        Some("firefox"),
        "Codex keeps the lawyer's own"
    );
    assert_eq!(get(&codex, "CLAUSEO_SIGNIN_URL_FILE"), None);
}
