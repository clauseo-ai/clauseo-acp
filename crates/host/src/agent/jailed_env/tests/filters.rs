//! Round 3b's filters (reviewer M2, lows 1-2): the credential rule shared with the firm plan,
//! the setup hook's route credentials, the blocked names and the non-ASCII names.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

use super::{Kind, has, jailed, launch, lawyers_env, os};
use crate::agent::jailed_env::filters::{BLOCKED, Pattern, ROUTES, is_blocked};
use crate::agent::jailed_env::is_credential;

fn hook(pairs: &[(&str, &str)]) -> BTreeMap<OsString, OsString> {
    os(pairs).into_iter().collect()
}

/// M2(a): a `_`-separated part that is a secret word drops a settings name; a count such as
/// `TOKENS` does not.
#[test]
fn a_secret_word_part_drops_a_settings_name_and_a_token_count_passes() {
    let settings = launch(&[
        ("DB_PASSWD", "x"),
        ("SERVICE_AUTH", "x"),
        ("SESSION_COOKIE", "x"),
        ("MY_CREDENTIALS", "x"),
        ("acme_secret", "x"),
        ("CLAUDE_CODE_MAX_OUTPUT_TOKENS", "32000"),
        ("MAX_THINKING_TOKENS", "8000"),
        ("MAX_MCP_OUTPUT_TOKENS", "25000"),
    ]);
    for kind in [Kind::Claude, Kind::Codex] {
        let env = jailed(kind, Vec::new(), &settings, BTreeMap::new());
        for name in [
            "DB_PASSWD",
            "SERVICE_AUTH",
            "SESSION_COOKIE",
            "MY_CREDENTIALS",
            "acme_secret",
        ] {
            assert!(!has(&env, name), "{kind:?}: {name}");
        }
        assert_eq!(env["CLAUDE_CODE_MAX_OUTPUT_TOKENS"], "32000", "{kind:?}");
        assert_eq!(env["MAX_THINKING_TOKENS"], "8000", "{kind:?}");
        assert_eq!(env["MAX_MCP_OUTPUT_TOKENS"], "25000", "{kind:?}");
    }
}

/// M2(a): the host and the firm plan's check use one rule: every name the plan calls
/// secret-shaped is a credential here, and the counts pass both.
#[test]
fn the_host_and_the_firm_plan_agree_on_secret_shaped_names() {
    for word in clauseo_settings::secret_words() {
        for name in [
            word.to_owned(),
            format!("ACME_{word}"),
            format!("{}_here", word.to_ascii_lowercase()),
        ] {
            assert!(clauseo_settings::secret_shaped(&name), "{name}");
            assert!(is_credential(OsStr::new(&name)), "{name}");
        }
    }
    for name in [
        "OPENAI_APIKEY",
        "ACCESSKEY",
        "PRIVATEKEY",
        "CLIENTSECRET",
        "PGPASSWORD",
        "MYSQL_PWD",
        "GITHUB_PAT",
        "SENTRY_DSN",
        "AZURE_STORAGE_SAS",
        "DB_CONNECTION_STRING",
        "FOO_PASSPHRASE",
    ] {
        assert!(clauseo_settings::secret_shaped(name), "{name}");
        assert!(is_credential(OsStr::new(name)), "{name}");
    }
    for name in [
        "PWD",
        "OLDPWD",
        "KEYBOARD_LAYOUT",
        "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
        "MAX_THINKING_TOKENS",
        "MAX_MCP_OUTPUT_TOKENS",
        "ANTHROPIC_MODEL",
        "LANG",
    ] {
        assert!(!clauseo_settings::secret_shaped(name), "{name}");
        assert!(!is_credential(OsStr::new(name)), "{name}");
    }
}

/// M2(b): the plan's variables through the hook get the credential filter; the selected route's
/// own credentials pass.
#[test]
fn the_hook_keeps_only_its_route_s_credentials() {
    let env = jailed(
        Kind::Claude,
        Vec::new(),
        &launch(&[]),
        hook(&[
            ("GITHUB_TOKEN", "ghp_x"),
            ("ANTHROPIC_AUTH_TOKEN", "tok"),
            ("AWS_REGION", "eu-west-2"),
            ("ANTHROPIC_API_KEY", "clauseo-stored-key"),
            ("ANTHROPIC_BASE_URL", "https://gateway.firm.example"),
            ("ANTHROPIC_MODEL", "claude-opus"),
        ]),
    );
    assert!(!has(&env, "GITHUB_TOKEN"));
    assert!(!has(&env, "ANTHROPIC_AUTH_TOKEN"));
    // No Bedrock switch, so Bedrock's names are not this route's.
    assert!(!has(&env, "AWS_REGION"));
    assert_eq!(env["ANTHROPIC_API_KEY"], "clauseo-stored-key");
    assert_eq!(env["ANTHROPIC_BASE_URL"], "https://gateway.firm.example");
    assert_eq!(env["ANTHROPIC_MODEL"], "claude-opus");

    let env = jailed(
        Kind::Claude,
        Vec::new(),
        &launch(&[]),
        hook(&[
            ("CLAUDE_CODE_USE_BEDROCK", "1"),
            ("AWS_REGION", "eu-west-2"),
            ("AWS_PROFILE", "firm"),
            ("AWS_SECRET_ACCESS_KEY", "secret"),
            ("GITHUB_TOKEN", "ghp_x"),
        ]),
    );
    assert_eq!(env["CLAUDE_CODE_USE_BEDROCK"], "1");
    assert_eq!(env["AWS_REGION"], "eu-west-2");
    assert_eq!(env["AWS_PROFILE"], "firm");
    assert!(!has(&env, "AWS_SECRET_ACCESS_KEY"));
    assert!(!has(&env, "GITHUB_TOKEN"));

    let env = jailed(
        Kind::Claude,
        Vec::new(),
        &launch(&[]),
        hook(&[
            ("CLAUDE_CODE_USE_VERTEX", "1"),
            ("CLOUD_ML_REGION", "europe-west1"),
            ("ANTHROPIC_VERTEX_PROJECT_ID", "firm-project"),
            ("GOOGLE_APPLICATION_CREDENTIALS", "/home/ada/key.json"),
        ]),
    );
    assert_eq!(env["CLAUDE_CODE_USE_VERTEX"], "1");
    assert_eq!(env["CLOUD_ML_REGION"], "europe-west1");
    assert_eq!(env["ANTHROPIC_VERTEX_PROJECT_ID"], "firm-project");
    assert!(!has(&env, "GOOGLE_APPLICATION_CREDENTIALS"));

    // Every route's needs are credentials (else it needs no line) and none is blocked.
    for route in ROUTES {
        for name in route.needs {
            assert!(is_credential(OsStr::new(name)), "{name}");
            assert!(!is_blocked(OsStr::new(name)), "{name}");
        }
    }
}

/// Low 2: a name that runs code or moves a tool's config is dropped from every source, in any
/// case.
#[test]
fn a_blocked_name_is_dropped_from_every_source() {
    let blocked = [
        ("BASH_ENV", "/tmp/x.sh"),
        ("ENV", "/tmp/x.sh"),
        ("ZDOTDIR", "/tmp"),
        ("PROMPT_COMMAND", "curl x"),
        ("PYTHONSTARTUP", "/tmp/x.py"),
        ("PYTHONPATH", "/tmp"),
        ("PYTHONHOME", "/tmp"),
        ("PERL5OPT", "-Mx"),
        ("RUBYOPT", "-rx"),
        ("GIT_SSH_COMMAND", "x"),
        ("git_config_global", "/tmp/gitconfig"),
        ("DYLD_INSERT_LIBRARIES", "/tmp/x.dylib"),
        ("LD_PRELOAD", "/tmp/x.so"),
        ("LD_LIBRARY_PATH", "/tmp"),
        ("NPM_CONFIG_SCRIPT_SHELL", "/tmp/sh"),
        ("npm_config_registry", "http://x"),
        ("NODE_OPTIONS", "--require /tmp/x.js"),
        ("NODE_PATH", "/tmp"),
    ];
    let mut inherited = lawyers_env();
    inherited.extend(os(&blocked));
    for kind in [Kind::Claude, Kind::Codex] {
        let env = jailed(kind, inherited.clone(), &launch(&blocked), hook(&blocked));
        for (name, _) in blocked {
            assert!(!has(&env, name), "{kind:?}: {name}");
        }
    }
    // Every entry has an example.
    for pattern in BLOCKED {
        let covered = blocked.iter().any(|(name, _)| match pattern {
            Pattern::Name(known) => name.eq_ignore_ascii_case(known),
            Pattern::Prefix(prefix) => name
                .to_ascii_lowercase()
                .starts_with(&prefix.to_ascii_lowercase()),
        });
        assert!(covered, "{pattern:?}");
    }
}

/// Low 1: a name that is not ASCII is dropped from every source (Windows folds its case).
#[test]
fn a_non_ascii_name_is_dropped_from_every_source() {
    let odd = [("LC_ÉTÉ", "x"), ("MODÈLE", "x"), ("PATH\u{212A}", "x")];
    let mut inherited = lawyers_env();
    inherited.extend(os(&odd));
    let env = jailed(Kind::Claude, inherited, &launch(&odd), hook(&odd));
    assert!(env.keys().all(|name| name.is_ascii()), "{:?}", env.keys());
    assert_eq!(env["LC_ALL"], "en_GB.UTF-8");
}

/// Round 3d: a value with a password in a web address is dropped from the settings and the
/// hook, whatever the name; one without passes; the key helper's cache time and the OpenTelemetry
/// headers follow the settings' rule.
#[test]
fn a_password_web_address_is_dropped_and_the_settings_rule_s_names_agree() {
    let secret = [("DATABASE_URL", "postgres://u:p@h/db")];
    let plain = [("SITE_URL", "https://user@example.com")];
    let settings = launch(&[
        secret[0],
        plain[0],
        ("CLAUDE_CODE_API_KEY_HELPER_TTL_MS", "60000"),
    ]);
    let env = jailed(
        Kind::Claude,
        Vec::new(),
        &settings,
        hook(&[("REDIS_URL", "redis://:pw@cache:6379")]),
    );
    assert!(!has(&env, "DATABASE_URL"));
    assert!(!has(&env, "REDIS_URL"));
    assert_eq!(env["SITE_URL"], "https://user@example.com");
    assert_eq!(env["CLAUDE_CODE_API_KEY_HELPER_TTL_MS"], "60000");
    assert_eq!(
        crate::agent::jailed_env::ignored(&settings.env),
        ["DATABASE_URL"]
    );
    for name in [
        "OTEL_EXPORTER_OTLP_HEADERS",
        "OTEL_EXPORTER_OTLP_TRACES_HEADERS",
    ] {
        assert!(is_credential(OsStr::new(name)), "{name}");
    }
    assert!(!is_credential(OsStr::new(
        "CLAUDE_CODE_API_KEY_HELPER_TTL_MS"
    )));
}
