//! The policy tier as Claude Code joins it, through [`SettingsEnv::read`]: helper keys' names
//! in any case, the files' helper keys merged, the cache used before it is verified, documents
//! with no content, and the cache's own size limit.

use serde_json::{Value, json};

use super::super::{MAX_FILE, OsReads};
use super::{ClaudeSources, SettingsEnv, Temp, reaches};

const GATEWAY: &str = "https://gateway.firm.example";
const DEFAULT: &str = "https://api.anthropic.com";

/// `temp`'s read with HKLM's `Settings` value holding `hklm` (none: not set).
fn read_os(temp: &Temp, hklm: Option<&Value>) -> SettingsEnv {
    let (managed, config) = (temp.0.join("managed"), temp.0.join("config"));
    let sources = ClaudeSources {
        managed: &managed,
        config: Some(&config),
        global: Some(&config),
        user: false,
        os_policy: true,
    };
    let registry = vec![Ok(hklm.map(Value::to_string))];
    let os = OsReads {
        plists: Vec::new(),
        registry,
    };
    SettingsEnv::read_with(&sources, Some(os))
}

/// Writes `value` at `path` under `temp`.
fn put_json(temp: &Temp, path: &str, value: &Value) {
    temp.put(path, &value.to_string());
}

/// Under HKLM's gateway, a managed file's AWS helper key drops its Bedrock switch in any case,
/// as Claude Code upper-cases the name before it looks: the gateway gets the sign-in, so with
/// no key the launch is refused. Names folded as on Windows, where the lower-case switch would
/// turn the sign-in off. With no tier above, the file is the first with content and keeps it.
#[test]
fn a_helper_key_drops_its_names_in_any_case() {
    let temp = Temp::new("helper-case");
    let gateway = json!({"env": {"ANTHROPIC_BASE_URL": GATEWAY}});
    let folded = |read: &SettingsEnv| crate::own_signin::reaches(std::iter::empty(), read, true);
    for switch in ["claude_code_use_bedrock", "Claude_Code_Use_Bedrock"] {
        let env = json!({switch: "1", "ANTHROPIC_BASE_URL": GATEWAY});
        let file = json!({"awsAuthRefresh": "aws sso login", "env": env});
        put_json(&temp, "managed/managed-settings.json", &file);
        assert!(folded(&read_os(&temp, Some(&gateway))), "{switch}");
        assert!(!folded(&read_os(&temp, None)), "{switch}");
    }
}

/// The managed files are one tier: a drop-in's blank or null helper key clears the base file's,
/// so the base file's gateway is not dropped and gets the sign-in; one setting it drops the
/// base file's.
#[test]
fn a_later_managed_file_s_helper_key_replaces_an_earlier_one_s() {
    let temp = Temp::new("helper-merge");
    let above = json!({"permissions": {"allow": []}});
    let base = json!({"apiKeyHelper": "print-firm-key", "env": {"ANTHROPIC_BASE_URL": GATEWAY}});
    put_json(&temp, "managed/managed-settings.json", &base);
    assert!(!reaches(&[], &read_os(&temp, Some(&above))));
    for cleared in [json!(""), json!("  "), Value::Null] {
        let drop_in = json!({"apiKeyHelper": cleared});
        put_json(&temp, "managed/managed-settings.d/50-clear.json", &drop_in);
        assert!(reaches(&[], &read_os(&temp, Some(&above))), "{drop_in}");
    }
    let gateway_only = json!({"env": {"ANTHROPIC_BASE_URL": GATEWAY}});
    put_json(&temp, "managed/managed-settings.json", &gateway_only);
    let drop_in = json!({"apiKeyHelper": "print-firm-key"});
    put_json(&temp, "managed/managed-settings.d/50-clear.json", &drop_in);
    assert!(!reaches(&[], &read_os(&temp, Some(&above))));
}

/// Claude Code may use the cache before verifying it, without its routing names, as the first
/// tier with content: then HKLM's helper key drops HKLM's default address, and a managed file's
/// gateway gets the sign-in. Refused with no key, though neither the cache as read nor no cache
/// at all would offer it.
#[test]
fn the_cache_with_content_and_no_pairs_is_a_reading_too() {
    let temp = Temp::new("cache-stripped");
    let hklm = json!({"apiKeyHelper": "print-firm-key", "env": {"ANTHROPIC_BASE_URL": DEFAULT}});
    let file = json!({"env": {"ANTHROPIC_BASE_URL": GATEWAY}});
    put_json(&temp, "managed/managed-settings.json", &file);
    assert!(!reaches(&[], &read_os(&temp, Some(&hklm))), "no cache");
    let cache = json!({"permissions": {"allow": []}, "env": {"ANTHROPIC_BASE_URL": DEFAULT}});
    put_json(&temp, "config/remote-settings.json", &cache);
    let read = read_os(&temp, Some(&hklm));
    assert_eq!(read.readings().count(), 4);
    assert!(reaches(&[], &read));
}

/// A document whose only keys are `managedSourcesBehavior` or `wslInheritsWindowsSettings` has
/// no content, as Claude Code counts it: the managed file below it is the first with content and
/// keeps its Bedrock switch beside its helper key, so the launch is not refused.
#[test]
fn a_document_of_only_the_sources_keys_has_no_content() {
    let temp = Temp::new("no-content");
    let file = json!({
        "awsAuthRefresh": "aws sso login",
        "env": {"ANTHROPIC_BASE_URL": GATEWAY, "CLAUDE_CODE_USE_BEDROCK": "1"},
    });
    put_json(&temp, "managed/managed-settings.json", &file);
    for hklm in [
        json!({"managedSourcesBehavior": "merge"}),
        json!({"wslInheritsWindowsSettings": true}),
    ] {
        assert!(!reaches(&[], &read_os(&temp, Some(&hklm))), "{hklm}");
    }
    let with_content = json!({"permissions": {"allow": []}});
    assert!(reaches(&[], &read_os(&temp, Some(&with_content))));
}

/// The cache is read up to Claude Code's 8 MiB limit for it; other files stay at 1 MiB.
#[test]
fn the_cache_is_read_up_to_its_own_limit() {
    let temp = Temp::new("cache-size");
    let pad = "x".repeat(usize::try_from(MAX_FILE).unwrap() + 1);
    let cache = json!({"pad": pad, "env": {"ANTHROPIC_BASE_URL": GATEWAY}});
    put_json(&temp, "config/remote-settings.json", &cache);
    let read = temp.read(false);
    assert!(!read.unsure());
    assert!(reaches(&[], &read));
    put_json(&temp, "managed/managed-settings.json", &cache);
    assert!(temp.read(false).unsure());
}
