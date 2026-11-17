//! Claude Code's helper keys in a lower policy tier, the profiles and registry values read past
//! one that cannot be read, and what a read that is unsure names.

use std::ffi::OsStr;
use std::path::Path;

use serde_json::{Map, Value, json};

use super::tiers::os_tier;
use super::{SettingsEnv, Temp, pair, pairs, plist, policy_doc, reaches};
use crate::own_signin::unreadable::REGISTRY_VALUES;
use crate::own_signin::{Refusal, Unreadable, UnreadableKind, own_signin_refusal};

const GATEWAY: &str = "https://gateway.firm.example";

/// A property list with `entries` beside an `env` holding `env`.
fn env_plist(entries: &str, env: &[(&str, &str)]) -> Vec<u8> {
    let env = env
        .iter()
        .map(|(name, value)| format!("<key>{name}</key><string>{value}</string>"))
        .collect::<Vec<_>>()
        .concat();
    plist(&format!("{entries}<key>env</key><dict>{env}</dict>"))
}

/// The OS tier holding only the property list `bytes`.
fn profile(bytes: Vec<u8>) -> SettingsEnv {
    let mut os = SettingsEnv::default();
    os.add_plist_reads([Ok(Some(bytes))]);
    os
}

/// The OS tier holding only HKLM's `Settings` value `text`.
fn hklm(text: &str) -> SettingsEnv {
    let mut os = SettingsEnv::default();
    os.add_registry_tiers(vec![Ok(Some(text.to_owned()))], false);
    os
}

/// In a policy tier after the first with content, an AWS helper key drops that tier's Bedrock
/// switch, so a profile's or HKLM's gateway above it gets the sign-in: refused with no key.
/// The first tier with content keeps its switch, a blank helper key drops nothing, and
/// `gcpAuthRefresh` drops the Vertex switch.
#[test]
fn a_helper_key_drops_its_names_from_a_lower_tier() {
    let gateway = json!({"env": {"ANTHROPIC_BASE_URL": GATEWAY}}).to_string();
    let switched = |key: &str, value: &str, switch: &str| {
        let env = Map::from_iter([(switch.to_owned(), json!("1"))]);
        let doc = Map::from_iter([
            (key.to_owned(), json!(value)),
            ("env".to_owned(), env.into()),
        ]);
        policy_doc(&Value::Object(doc).to_string())
    };
    let bedrock = || switched("awsAuthRefresh", "aws sso login", "CLAUDE_CODE_USE_BEDROCK");
    for os in [
        profile(env_plist("", &[("ANTHROPIC_BASE_URL", GATEWAY)])),
        hklm(&gateway),
    ] {
        let read = SettingsEnv::policy([SettingsEnv::default(), os, bedrock()]);
        assert!(reaches(&[], &read), "{read:?}");
        assert!(!reaches(&[("ANTHROPIC_API_KEY", "stored")], &read));
    }
    let under =
        |file: SettingsEnv| SettingsEnv::policy([SettingsEnv::default(), hklm(&gateway), file]);
    let export = switched("awsCredentialExport", "x", "CLAUDE_CODE_USE_BEDROCK");
    let vertex = switched(
        "gcpAuthRefresh",
        "gcloud auth login",
        "CLAUDE_CODE_USE_VERTEX",
    );
    assert!(reaches(&[], &under(export)));
    assert!(reaches(&[], &under(vertex)));
    let blank = switched("awsAuthRefresh", " \u{FEFF}\t", "CLAUDE_CODE_USE_BEDROCK");
    assert!(!reaches(&[], &under(blank)));
    let other = switched(
        "gcpAuthRefresh",
        "gcloud auth login",
        "CLAUDE_CODE_USE_BEDROCK",
    );
    assert!(!reaches(&[], &under(other)));
    let alone = SettingsEnv::policy([SettingsEnv::default(), SettingsEnv::default(), bedrock()]);
    assert!(!reaches(&[("ANTHROPIC_BASE_URL", GATEWAY)], &alone));
}

/// With an applied remote cache that has content, a profile's `apiKeyHelper` drops its own
/// default address, so a managed gateway below gets the sign-in: refused with no key. Without
/// the helper key, or without the cache's content (the profile is then the first tier), the
/// profile's default address wins.
#[test]
fn an_api_key_helper_drops_a_lower_tier_s_address_and_hides_nothing() {
    let cache = || policy_doc(r#"{"permissions": {}}"#);
    let default = ("ANTHROPIC_BASE_URL", "https://api.anthropic.com");
    let helper = "<key>apiKeyHelper</key><string>key-helper</string>";
    let file = || policy_doc(&json!({"env": {"ANTHROPIC_BASE_URL": GATEWAY}}).to_string());
    let read = SettingsEnv::policy([cache(), profile(env_plist(helper, &[default])), file()]);
    assert_eq!(pairs(&read), [pair("ANTHROPIC_BASE_URL", GATEWAY)]);
    assert!(reaches(&[], &read));
    assert!(!reaches(&[("ANTHROPIC_API_KEY", "stored")], &read));
    let read = SettingsEnv::policy([cache(), profile(env_plist("", &[default])), file()]);
    assert!(!reaches(&[], &read), "{read:?}");
    let first = profile(env_plist(helper, &[default]));
    let read = SettingsEnv::policy([SettingsEnv::default(), first, file()]);
    assert!(!reaches(&[], &read), "{read:?}");
}

/// After a profile that cannot be read, the next one is read for the pairs that can only refuse:
/// a computer gateway and a blank key there blank a stored key, so the launch is refused. A key
/// of the computer's is not counted.
#[test]
fn past_an_unreadable_profile_the_next_one_s_refusing_pairs_count() {
    let date = || plist("<key>since</key><date>2026-01-01T00:00:00Z</date>");
    let blank = env_plist(
        "",
        &[("ANTHROPIC_BASE_URL", GATEWAY), ("ANTHROPIC_API_KEY", "")],
    );
    let read = os_tier(Some(date()), Some(blank));
    assert!(read.unsure());
    assert!(
        reaches(&[("ANTHROPIC_API_KEY", "stored")], &read),
        "{read:?}"
    );
    let keyed = env_plist(
        "",
        &[("ANTHROPIC_BASE_URL", GATEWAY), ("ANTHROPIC_API_KEY", "k")],
    );
    let read = os_tier(Some(date()), Some(keyed));
    assert_eq!(pairs(&read), [pair("ANTHROPIC_BASE_URL", GATEWAY)]);
    assert!(reaches(&[], &read));
}

/// Under an HKLM value Claude Code cannot parse, HKCU's blank key is kept with its gateway, so a
/// stored key is blanked and the launch refused; a blank key alone is refused too.
#[test]
fn past_an_unreadable_hklm_value_hkcu_s_blank_key_counts() {
    let hklm = format!("\u{FEFF}{}", json!({"env": {"ANTHROPIC_API_KEY": "k"}}));
    for hkcu in [
        json!({"env": {"ANTHROPIC_BASE_URL": GATEWAY, "ANTHROPIC_API_KEY": ""}}),
        json!({"env": {"ANTHROPIC_API_KEY": " "}}),
    ] {
        let mut os = SettingsEnv::default();
        os.add_registry_tiers(
            vec![Ok(Some(hklm.clone())), Ok(Some(hkcu.to_string()))],
            false,
        );
        let read = SettingsEnv::policy([SettingsEnv::default(), os, SettingsEnv::default()]);
        assert!(
            reaches(&[("ANTHROPIC_API_KEY", "stored")], &read),
            "{hkcu}: {read:?}"
        );
    }
}

/// What an unsure read names: the file's path, or the variable's or helper key's name as
/// Clauseo spells it, with the trouble's kind; the refusal is then its own, unless the launch
/// names an endpoint (that refusal's words hold) or has a key.
#[test]
fn an_unsure_read_names_the_setting_it_could_not_read() {
    use UnreadableKind::{EnvNotObject, NotParsed, NotText, PolicyHelper};
    let cases: [(&str, &str, UnreadableKind, Option<&str>); 6] = [
        ("config/settings.json", "{ not json", NotParsed, None),
        (
            "managed/managed-settings.json",
            r#"{"env": "x"}"#,
            EnvNotObject,
            None,
        ),
        (
            "config/.claude.json",
            r#"{"env": {"anthropic_base_url": ["x"]}}"#,
            NotText,
            Some("ANTHROPIC_BASE_URL"),
        ),
        (
            "managed/managed-settings.json",
            r#"{"policyHelper": {}}"#,
            PolicyHelper,
            None,
        ),
        (
            "config/remote-settings.json",
            r#"{"apiKeyHelper": 1}"#,
            NotText,
            Some("apiKeyHelper"),
        ),
        (
            "managed/managed-settings.json/inside",
            "",
            UnreadableKind::Unreadable,
            None,
        ),
    ];
    for (index, (path, text, kind, name)) in cases.into_iter().enumerate() {
        let temp = Temp::new(&format!("named-{index}"));
        temp.put(path, text);
        let file = path.strip_suffix("/inside").unwrap_or(path);
        // Joined a component at a time, as the read does: Windows keeps a `/` given to `join`.
        let at = || {
            file.split('/')
                .fold(temp.0.clone(), |path, part| path.join(part))
        };
        let setting = name.map_or_else(|| at().display().to_string(), str::to_owned);
        let read = temp.read(true);
        let why = Unreadable {
            kind,
            setting: Some(setting),
        };
        assert_eq!(read.unreadable(), Some(&why), "{path}: {text}");
        assert_eq!(
            own_signin_refusal([], &read),
            Some(Refusal::Unreadable(&why))
        );
        let gateway = [(OsStr::new("ANTHROPIC_BASE_URL"), OsStr::new(GATEWAY))];
        assert_eq!(own_signin_refusal(gateway, &read), Some(Refusal::Endpoint));
        let key = [(OsStr::new("ANTHROPIC_API_KEY"), OsStr::new("k"))];
        assert_eq!(own_signin_refusal(key, &read), None);
    }
    let named = |read: &SettingsEnv| {
        read.unreadable()
            .cloned()
            .map(|why| (why.kind, why.setting))
    };
    let mut os = SettingsEnv::default();
    os.add_registry_tiers(vec![Ok(Some("\u{FEFF}{}".to_owned())), Err(())], false);
    assert_eq!(
        named(&os),
        Some((NotParsed, Some(REGISTRY_VALUES[0].to_owned())))
    );
    let mut os = SettingsEnv::default();
    os.add_registry_tiers(vec![Ok(None), Err(())], false);
    let hkcu = Some(REGISTRY_VALUES[1].to_owned());
    assert_eq!(named(&os), Some((UnreadableKind::Unreadable, hkcu)));
    let date = plist("<key>since</key><date>2026-01-01T00:00:00Z</date>");
    let (kind, setting) = named(&os_tier(Some(date), None)).unwrap();
    assert_eq!(kind, UnreadableKind::DateOrData);
    assert!(
        setting.is_some_and(|path| Path::new(&path).ends_with("com.anthropic.claudecode.plist"))
    );
    assert_eq!(
        named(&SettingsEnv::unknown()),
        Some((UnreadableKind::NoHome, None))
    );
}
