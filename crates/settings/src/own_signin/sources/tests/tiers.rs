//! The OS tier's property lists in Claude Code's order, a name's spellings across the policy
//! tiers, and values Bun's `trim` reads as blank.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::json;

use super::{SettingsEnv, Temp, pair, pairs, plist, plist_paths_for, policy_doc, reaches};

const GATEWAY: &str = "https://gateway.firm.example";

/// A property list whose `env` sets `name` to `value`, with `other` entries beside it.
fn env_plist(name: &str, value: &str, other: &str) -> Vec<u8> {
    plist(&format!(
        "{other}<key>env</key><dict><key>{name}</key><string>{value}</string></dict>"
    ))
}

/// The policy tier when the login user `lawyer`'s profile is `user` and the computer's is
/// `device` (`None`: not there), read in [`plist_paths_for`]'s order.
pub(super) fn os_tier(user: Option<Vec<u8>>, device: Option<Vec<u8>>) -> SettingsEnv {
    let paths = plist_paths_for(Some("lawyer"));
    let mut files: BTreeMap<PathBuf, Vec<u8>> = BTreeMap::new();
    let by_user = paths
        .iter()
        .find(|path| path.to_string_lossy().contains("lawyer"));
    let by_device = paths
        .iter()
        .find(|path| !path.to_string_lossy().contains("lawyer"));
    for (path, bytes) in [(by_user, user), (by_device, device)] {
        if let (Some(path), Some(bytes)) = (path, bytes) {
            files.insert(path.clone(), bytes);
        }
    }
    let mut os = SettingsEnv::default();
    os.add_plists(
        paths
            .iter()
            .map(|path| (Some(path.as_path()), Ok(files.get(path).cloned()))),
    );
    SettingsEnv::policy([SettingsEnv::default(), os, SettingsEnv::default()])
}

/// Claude Code reads the login user's profile first, and the first with content is the tier.
/// A computer profile with any content does not hide the user's gateway; a user profile with
/// content hides the computer's gateway, so Claude Code goes to the default address and the
/// launch goes ahead. Either profile alone with a gateway is refused.
#[test]
fn the_login_user_s_profile_comes_before_the_computer_s() {
    let content = || plist("<key>permissions</key><dict/>");
    let gateway = || env_plist("ANTHROPIC_BASE_URL", GATEWAY, "");
    let read = os_tier(Some(gateway()), Some(content()));
    assert_eq!(pairs(&read), [pair("ANTHROPIC_BASE_URL", GATEWAY)]);
    assert!(reaches(&[], &read));
    let read = os_tier(Some(content()), Some(gateway()));
    assert_eq!(read.pairs(), []);
    assert!(!read.unsure() && !reaches(&[], &read));
    for (user, device) in [(None, Some(gateway())), (Some(gateway()), None)] {
        let read = os_tier(user, device);
        assert!(reaches(&[], &read), "{read:?}");
        assert!(!reaches(&[("ANTHROPIC_API_KEY", "k")], &read));
    }
}

/// A profile holding a date or data value anywhere, which plutil cannot turn into JSON, is
/// unsure and keeps none of its pairs: its key does not count, so with a computer gateway (read
/// past it) and no key of the launch's own the launch is refused.
#[test]
fn a_profile_with_a_date_or_data_value_is_unsure() {
    let device = || env_plist("ANTHROPIC_BASE_URL", GATEWAY, "");
    for other in [
        "<key>since</key><date>2026-01-01T00:00:00Z</date>",
        "<key>blob</key><data>AAAA</data>",
        "<key>list</key><array><dict><key>at</key><date>2026-01-01T00:00:00Z</date></dict></array>",
    ] {
        let user = env_plist("ANTHROPIC_API_KEY", "k", other);
        let read = os_tier(Some(user), Some(device()));
        assert!(read.unsure(), "{other}: {read:?}");
        assert_eq!(
            pairs(&read),
            [pair("ANTHROPIC_BASE_URL", GATEWAY)],
            "{other}"
        );
        assert!(reaches(&[], &read), "{other}");
        assert!(
            !reaches(&[("ANTHROPIC_API_KEY", "stored")], &read),
            "{other}"
        );
    }
    let plain = env_plist("ANTHROPIC_API_KEY", "k", "<key>n</key><integer>1</integer>");
    assert!(!reaches(&[], &os_tier(Some(plain), Some(device()))));
}

/// A higher tier's lower-case spelling of a name wins, and Claude Code writes its value under a
/// lower tier's upper-case spelling: the gateway is seen, from a JSON tier or a profile.
#[test]
fn a_higher_tier_s_spelling_gives_its_value_to_a_lower_tier_s() {
    let lower = r#"{"env": {"anthropic_base_url": "https://gateway.firm.example"}}"#;
    let default = r#"{"env": {"ANTHROPIC_BASE_URL": "https://api.anthropic.com"}}"#;
    let read = SettingsEnv::policy([
        policy_doc(lower),
        SettingsEnv::default(),
        policy_doc(default),
    ]);
    assert_eq!(
        pairs(&read),
        [
            pair("anthropic_base_url", GATEWAY),
            pair("ANTHROPIC_BASE_URL", GATEWAY)
        ]
    );
    assert!(reaches(&[], &read));
    let mut os = SettingsEnv::default();
    os.add_plist_reads([Ok(Some(env_plist("anthropic_base_url", GATEWAY, "")))]);
    let read = SettingsEnv::policy([SettingsEnv::default(), os, policy_doc(default)]);
    assert!(reaches(&[], &read));
    // The same spelling below the winner adds nothing.
    let read = SettingsEnv::policy([
        policy_doc(default),
        SettingsEnv::default(),
        policy_doc(default),
    ]);
    assert_eq!(
        pairs(&read),
        [pair("ANTHROPIC_BASE_URL", "https://api.anthropic.com")]
    );
}

/// The policy tier when the remote settings are `text` and no other tier is there.
fn only_tier(text: &str) -> SettingsEnv {
    SettingsEnv::policy([
        policy_doc(text),
        SettingsEnv::default(),
        SettingsEnv::default(),
    ])
}

/// A key of only U+FEFF, or U+FEFF with spaces, is blank, as Bun's `trim` removes U+FEFF: it is
/// kept as empty and turns nothing off. An endpoint of the same is still an endpoint, which
/// errs toward refusing.
#[test]
fn a_managed_key_of_only_a_byte_order_mark_is_no_key() {
    for blank in ["\u{FEFF}", "\u{FEFF} \u{FEFF}", " \u{FEFF}\t"] {
        let key = json!({"env": {"ANTHROPIC_API_KEY": blank, "ANTHROPIC_BASE_URL": GATEWAY}});
        let read = only_tier(&key.to_string());
        assert_eq!(
            pairs(&read),
            [
                pair("ANTHROPIC_API_KEY", ""),
                pair("ANTHROPIC_BASE_URL", GATEWAY)
            ],
            "{blank:?}"
        );
        assert!(reaches(&[], &read), "{blank:?}");
        let mut os = SettingsEnv::default();
        os.add_plist_reads([Ok(Some(env_plist("ANTHROPIC_API_KEY", blank, "")))]);
        assert!(
            reaches(&[("ANTHROPIC_BASE_URL", GATEWAY)], &os),
            "{blank:?}"
        );
        let read = only_tier(&json!({"env": {"ANTHROPIC_BASE_URL": blank}}).to_string());
        assert!(reaches(&[], &read), "{blank:?}");
        assert!(!reaches(&[("ANTHROPIC_API_KEY", "k")], &read), "{blank:?}");
    }
}

/// A registry value starting with U+FEFF is unsure, its key not counted: with HKCU's gateway
/// and no key of the launch's own, the launch is refused. Without the mark the key counts.
#[test]
fn a_registry_value_with_a_byte_order_mark_is_unsure() {
    let key = r#"{"env": {"ANTHROPIC_API_KEY": "k"}}"#;
    let hkcu = Ok(Some(
        json!({"env": {"ANTHROPIC_BASE_URL": GATEWAY}}).to_string(),
    ));
    let mut marked = SettingsEnv::default();
    marked.add_registry_tiers(
        vec![Ok(Some(format!("\u{FEFF}{key}"))), hkcu.clone()],
        false,
    );
    let read = SettingsEnv::policy([SettingsEnv::default(), marked, SettingsEnv::default()]);
    assert!(read.unsure(), "{read:?}");
    assert!(reaches(&[], &read));
    assert!(!reaches(&[("ANTHROPIC_API_KEY", "stored")], &read));
    let mut plain = SettingsEnv::default();
    plain.add_registry_tiers(vec![Ok(Some(key.to_owned())), hkcu], false);
    let read = SettingsEnv::policy([SettingsEnv::default(), plain, SettingsEnv::default()]);
    assert!(!read.unsure() && !reaches(&[], &read), "{read:?}");
}

/// Claude Code may not apply the cached remote settings, so they can add an endpoint but never
/// take away one a lower tier sets: their default address over a managed gateway is refused with
/// no key, and so is their key over it. A key of the launch's own lets either go, and with no
/// endpoint below them the cache changes nothing.
#[test]
fn the_remote_cache_never_takes_an_endpoint_away() {
    let default = r#"{"env": {"ANTHROPIC_BASE_URL": "https://api.anthropic.com"}}"#;
    let key = r#"{"env": {"ANTHROPIC_API_KEY": "k"}}"#;
    let gateway = json!({"env": {"ANTHROPIC_BASE_URL": GATEWAY}}).to_string();
    for (index, remote) in [default, key].into_iter().enumerate() {
        let temp = Temp::new(&format!("remote-cache-{index}"));
        temp.put("config/remote-settings.json", remote);
        temp.put("managed/managed-settings.json", &gateway);
        let read = temp.read(true);
        assert!(reaches(&[], &read), "{remote}");
        assert!(
            !reaches(&[("ANTHROPIC_API_KEY", "stored")], &read),
            "{remote}"
        );
    }
    let temp = Temp::new("remote-cache-alone");
    temp.put("config/remote-settings.json", default);
    let read = temp.read(true);
    assert!(!reaches(&[], &read), "{read:?}");
}
