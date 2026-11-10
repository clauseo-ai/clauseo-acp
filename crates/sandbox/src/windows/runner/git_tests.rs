//! The agent's git trust list: `safe.directory` for the run's matter roots
//! (`LaunchMessage::git_roots`), in git's config environment, after the OpenSSL entry.

use super::env::agent_env;
use super::tests::{DATA, STATE, get, message, profile};
use super::*;

const MATTER: &str = r"D:\Matters\Smith";
const PRECEDENTS: &str = r"E:\Shared\Precedents";
const JONES: &str = r"\\fs01\Firm\Matters\Jones";

/// `message` with `roots` as its git roots.
fn with_roots(roots: &[&str]) -> LaunchMessage {
    let mut spec = message();
    spec.git_roots = roots.iter().map(|root| (*root).to_owned()).collect();
    spec
}

/// The environment's git config entries, by name, as the environment lists them (sorted).
fn git_entries(env: &[(String, String)]) -> Vec<(&str, &str)> {
    env.iter()
        .filter(|(name, _)| name.starts_with("GIT_CONFIG"))
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect()
}

#[test]
fn a_matter_only_launch_has_this_exact_environment() {
    let env = agent_env(&with_roots(&[MATTER]), &profile()).unwrap();
    let expected = [
        ("APPDATA", format!(r"{STATE}\AppData\Roaming")),
        (
            "CODEX_CA_CERTIFICATE",
            format!(r"{DATA}\sandbox-ca\roots.pem"),
        ),
        ("CODEX_HOME", STATE.to_owned()),
        ("GIT_CONFIG_COUNT", "3".to_owned()),
        ("GIT_CONFIG_KEY_0", "http.sslBackend".to_owned()),
        ("GIT_CONFIG_KEY_1", "safe.directory".to_owned()),
        ("GIT_CONFIG_KEY_2", "safe.directory".to_owned()),
        ("GIT_CONFIG_VALUE_0", "openssl".to_owned()),
        ("GIT_CONFIG_VALUE_1", "D:/Matters/Smith".to_owned()),
        ("GIT_CONFIG_VALUE_2", "D:/Matters/Smith/*".to_owned()),
        ("HOME", STATE.to_owned()),
        ("HOMEDRIVE", "C:".to_owned()),
        (
            "HOMEPATH",
            r"\Users\Lawyer\AppData\Local\Clauseo\agent-state\m1\codex-acp".to_owned(),
        ),
        ("LOCALAPPDATA", format!(r"{STATE}\AppData\Local")),
        ("SSL_CERT_FILE", format!(r"{DATA}\sandbox-ca\roots.pem")),
        ("SystemRoot", r"C:\Windows".to_owned()),
        ("TEMP", format!(r"{STATE}\tmp")),
        ("TMP", format!(r"{STATE}\tmp")),
        ("USERPROFILE", STATE.to_owned()),
    ];
    let expected: Vec<(&str, &str)> = expected
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    let actual: Vec<(&str, &str)> = env
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn a_matter_with_two_granted_roots_trusts_all_three_in_order() {
    let spec = with_roots(&[MATTER, PRECEDENTS, JONES]);
    spec.check().unwrap();
    let env = agent_env(&spec, &profile()).unwrap();
    assert_eq!(
        git_entries(&env),
        [
            ("GIT_CONFIG_COUNT", "7"),
            ("GIT_CONFIG_KEY_0", "http.sslBackend"),
            ("GIT_CONFIG_KEY_1", "safe.directory"),
            ("GIT_CONFIG_KEY_2", "safe.directory"),
            ("GIT_CONFIG_KEY_3", "safe.directory"),
            ("GIT_CONFIG_KEY_4", "safe.directory"),
            ("GIT_CONFIG_KEY_5", "safe.directory"),
            ("GIT_CONFIG_KEY_6", "safe.directory"),
            ("GIT_CONFIG_VALUE_0", "openssl"),
            ("GIT_CONFIG_VALUE_1", "D:/Matters/Smith"),
            ("GIT_CONFIG_VALUE_2", "D:/Matters/Smith/*"),
            ("GIT_CONFIG_VALUE_3", "E:/Shared/Precedents"),
            ("GIT_CONFIG_VALUE_4", "E:/Shared/Precedents/*"),
            ("GIT_CONFIG_VALUE_5", "//fs01/Firm/Matters/Jones"),
            ("GIT_CONFIG_VALUE_6", "//fs01/Firm/Matters/Jones/*"),
        ]
    );
}

#[test]
fn an_inherited_git_config_is_dropped_and_replaced() {
    let mut spec = with_roots(&[MATTER, PRECEDENTS]);
    spec.env.extend(
        [
            ("GIT_CONFIG_COUNT", "5"),
            ("GIT_CONFIG_KEY_0", "core.sshCommand"),
            ("GIT_CONFIG_VALUE_0", "evil"),
            ("GIT_CONFIG_KEY_4", "safe.directory"),
            ("GIT_CONFIG_VALUE_4", r"C:\Users\Other"),
            ("git_config_key_9", "core.pager"),
        ]
        .map(|(name, value)| (name.to_owned(), value.to_owned())),
    );
    let env = agent_env(&spec, &profile()).unwrap();
    assert_eq!(
        git_entries(&env),
        [
            ("GIT_CONFIG_COUNT", "5"),
            ("GIT_CONFIG_KEY_0", "http.sslBackend"),
            ("GIT_CONFIG_KEY_1", "safe.directory"),
            ("GIT_CONFIG_KEY_2", "safe.directory"),
            ("GIT_CONFIG_KEY_3", "safe.directory"),
            ("GIT_CONFIG_KEY_4", "safe.directory"),
            ("GIT_CONFIG_VALUE_0", "openssl"),
            ("GIT_CONFIG_VALUE_1", "D:/Matters/Smith"),
            ("GIT_CONFIG_VALUE_2", "D:/Matters/Smith/*"),
            ("GIT_CONFIG_VALUE_3", "E:/Shared/Precedents"),
            ("GIT_CONFIG_VALUE_4", "E:/Shared/Precedents/*"),
        ]
    );
}

#[test]
fn paths_become_forward_slashed_with_the_drive_as_given() {
    let cases = [
        (r"D:\Matters\Smith\", "D:/Matters/Smith"),
        (r"d:\matters\smith", "d:/matters/smith"),
        ("D:/Matters/Smith", "D:/Matters/Smith"),
        (r"\\?\E:\Shared\Precedents", "E:/Shared/Precedents"),
        (r"\\fs01\Firm\Matters\Jones", "//fs01/Firm/Matters/Jones"),
        (r"\\fs01\Firm\Jones\\Sub\", "//fs01/Firm/Jones/Sub"),
        (r"\\?\UNC\fs01\Firm\Jones", "//fs01/Firm/Jones"),
    ];
    for (root, expected) in cases {
        let env = agent_env(&with_roots(&[root]), &profile()).unwrap();
        assert_eq!(get(&env, "GIT_CONFIG_VALUE_1"), Some(expected), "{root}");
        assert_eq!(get(&env, "GIT_CONFIG_COUNT"), Some("3"), "{root}");
    }
}

#[test]
fn a_root_git_cannot_express_is_skipped_and_a_repeat_counts_once() {
    let roots = [
        r"D:\",
        "D:\\Matters\\New\nLine",
        r"D:\Matters\Star*",
        r"\\*\share\Matters",
        MATTER,
        MATTER,
    ];
    let spec = with_roots(&roots);
    let env = agent_env(&spec, &profile()).unwrap();
    assert_eq!(
        git_entries(&env),
        [
            ("GIT_CONFIG_COUNT", "3"),
            ("GIT_CONFIG_KEY_0", "http.sslBackend"),
            ("GIT_CONFIG_KEY_1", "safe.directory"),
            ("GIT_CONFIG_KEY_2", "safe.directory"),
            ("GIT_CONFIG_VALUE_0", "openssl"),
            ("GIT_CONFIG_VALUE_1", "D:/Matters/Smith"),
            ("GIT_CONFIG_VALUE_2", "D:/Matters/Smith/*"),
        ]
    );
    // No entry is git's bare wildcard, whatever the roots were: each is a root or `<root>/*`.
    for (name, value) in git_entries(&env) {
        assert!(value != "*", "{name}={value}");
    }
}

#[test]
fn a_relative_git_root_is_refused_by_the_check() {
    assert!(with_roots(&["Matters/Smith"]).check().is_err());
}
