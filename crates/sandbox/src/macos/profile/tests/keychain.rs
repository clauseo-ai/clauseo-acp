//! No allow rule names the keychain, for any policy, and none comes after its denies (0038).

use super::*;

/// The keychain's names: its services, the prefixes of the services behind it, and its folders.
/// No allow rule may name one, here or in the base, for any policy (0038).
const KEYCHAIN_NAMES: [&str; 11] = [
    "com.apple.SecurityServer",
    "com.apple.security",
    "com.apple.secd",
    "com.apple.ctkd.",
    "com.apple.CoreAuthentication",
    "com.apple.authd",
    "com.apple.keychainsharingmessagingd",
    "com.apple.trustd",
    "Keychains",
    "(param \"KEYCHAINS\")",
    "keychain",
];

/// The keychain's two denies, given the home or none.
fn keychain_denies(home: bool) -> String {
    let home = if home {
        "\n  (subpath (param \"KEYCHAINS\"))"
    } else {
        ""
    };
    let names = [
        "com.apple.SecurityServer",
        "com.apple.securityd.xpc",
        "com.apple.securityd",
    ]
    .map(|name| format!("\n  (global-name \"{name}\")"));
    let prefixes = [
        "com.apple.security",
        "com.apple.secd",
        "com.apple.ctkd.",
        "com.apple.CoreAuthentication",
        "com.apple.authd",
        "com.apple.keychainsharingmessagingd",
    ]
    .map(|prefix| format!("\n  (global-name-prefix \"{prefix}\")"));
    let services = format!("{}{}", names.concat(), prefixes.concat());
    format!(
        "(deny file-read* file-write*{home}\n  (subpath \"/Library/Keychains\")\n  \
         (with message \"{TAG}\"))\n(deny mach-lookup{services}\n  (with message \"{TAG}\"))\n"
    )
}

/// A keychain deny: the folders' rule or the services'.
fn is_keychain_deny(rule: &str) -> bool {
    rule.starts_with("(deny")
        && (rule.contains("(subpath \"/Library/Keychains\")")
            || rule.contains("(global-name \"com.apple.SecurityServer\")"))
}

/// Every policy shape, one with the home's keychain folder as a read path and one with the
/// native code deny: no allow names the keychain, and no allow comes after its denies, so none
/// reaches it (Seatbelt applies the last match).
#[test]
fn no_policy_allows_the_keychain() {
    let w = World::new("profile-no-keychain");
    w.dir.dir("home/.claude");
    w.dir.dir("home/Library/Keychains");
    w.dir.dir("data/agent-state/m1/codex/config");
    w.dir.dir("browser");
    let full = w
        .builder()
        .runtime(w.at("data/agents/codex-acp"))
        .read(w.at("home/Library/Keychains"))
        .allowed_folder(w.at("elsewhere"))
        .notes(w.at("data/agent-state/m1/notes"))
        .credential_link(w.at("data/agent-state/m1/codex/auth.json"))
        .claude_store(w.at("home/.claude"))
        .read_only_in_state(w.at("data/agent-state/m1/codex/config"))
        .proxy_port(41_234)
        .loopback(Loopback::Any)
        .browser_socket_path(w.at("browser"))
        .build()
        .unwrap();
    let plain = w.builder().build().unwrap();
    let native = w.builder().native_code_deny(true).build().unwrap();
    // The home `Policy::builder` starts from: this user's.
    let real_home = Policy::builder(w.at("matter"), w.at("data/agent-state/m1/codex"))
        .build()
        .unwrap();
    for name in KEYCHAIN_NAMES {
        assert!(!BASE.contains(name), "the base names {name}");
    }
    for policy in [&full, &plain, &native, &real_home] {
        let profile = SeatbeltProfile::new(policy, TAG).unwrap();
        let text = profile.text();
        for rule in rules(text).iter().filter(|r| r.starts_with("(allow")) {
            for name in KEYCHAIN_NAMES {
                assert!(!rule.contains(name), "an allow names {name}: {rule}");
            }
        }
        assert!(
            text.contains(&keychain_denies(policy.home().is_some())),
            "{text}"
        );
        assert_no_allow_after(text, is_keychain_deny);
        let keychains = profile
            .params()
            .iter()
            .find(|(name, _)| name == "KEYCHAINS");
        assert_eq!(
            keychains.map(|(_, path)| path.clone()),
            policy.home().map(|h| h.join("Library").join("Keychains"))
        );
    }
}
