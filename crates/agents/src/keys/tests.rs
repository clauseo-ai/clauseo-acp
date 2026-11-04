use clauseo_protocol::AgentId;

use super::{KEYCHAIN_SERVICE, KeyError, Keychain, MAX_KEY_BYTES, account};

const KEY: &str = "sk-ant-api03-TESTKEY-not-real-0123456789";

fn claude() -> AgentId {
    AgentId::new("claude-acp").expect("a valid agent id")
}

fn codex() -> AgentId {
    AgentId::new("codex-acp").expect("a valid agent id")
}

#[test]
fn the_item_is_per_agent_under_one_service() {
    assert_eq!(KEYCHAIN_SERVICE, "ai.clauseo.desktop");
    assert_eq!(account(&claude()), "agent-key/claude-acp");
    assert_eq!(account(&codex()), "agent-key/codex-acp");
}

#[test]
fn store_load_and_clear_round_trip_per_agent() {
    let keychain = Keychain::memory();
    keychain.store(&claude(), KEY).expect("stored");
    keychain.store(&codex(), "codex-key").expect("stored");
    let loaded = keychain.load(&claude()).expect("loaded").expect("a key");
    assert_eq!(loaded.as_str(), KEY);

    keychain.store(&claude(), "replaced").expect("replaced");
    let loaded = keychain.load(&claude()).expect("loaded").expect("a key");
    assert_eq!(loaded.as_str(), "replaced");

    keychain.clear(&claude()).expect("cleared");
    assert_eq!(keychain.load(&claude()).expect("loaded"), None);
    let other = keychain
        .load(&codex())
        .expect("loaded")
        .expect("codex's key is kept");
    assert_eq!(other.as_str(), "codex-key");
}

#[test]
fn load_of_a_missing_item_is_none_and_clear_of_one_is_ok() {
    let keychain = Keychain::memory();
    assert_eq!(keychain.load(&claude()).expect("loaded"), None);
    keychain.clear(&claude()).expect("clearing nothing is ok");
    keychain.clear(&claude()).expect("twice too");
}

#[test]
fn a_refused_key_writes_nothing_and_its_error_holds_no_key() {
    let keychain = Keychain::memory();
    let long = "k".repeat(MAX_KEY_BYTES + 1);
    let pasted = format!("{KEY}\n");
    let spaced = format!("{KEY} x");
    for key in [
        "",
        long.as_str(),
        pasted.as_str(),
        spaced.as_str(),
        "a\u{0}b",
    ] {
        let err = keychain.store(&claude(), key).expect_err("refused");
        assert!(matches!(err, KeyError::Invalid { .. }), "{err:?}");
        if !key.is_empty() {
            assert!(!err.to_string().contains(key) && !format!("{err:?}").contains(key));
        }
        assert_eq!(keychain.load(&claude()).expect("loaded"), None);
    }
    keychain
        .store(&claude(), &"k".repeat(MAX_KEY_BYTES))
        .expect("the longest key is taken");
}

#[test]
fn debug_and_errors_never_show_the_key() {
    let keychain = Keychain::memory();
    keychain.store(&claude(), KEY).expect("stored");
    let debug = format!("{keychain:?}");
    assert!(debug.contains("agent-key/claude-acp"), "{debug}");
    assert!(
        !debug.contains(KEY) && !debug.contains("TESTKEY"),
        "{debug}"
    );

    let err = Keychain::none()
        .store(&claude(), KEY)
        .expect_err("no keychain");
    for text in [err.to_string(), format!("{err:?}")] {
        assert!(text.contains("claude-acp"), "{text}");
        assert!(!text.contains("TESTKEY"), "{text}");
    }
    let errors = [
        KeyError::NoAccess { agent: claude() },
        KeyError::Keychain {
            agent: claude(),
            kind: "a platform failure",
        },
    ];
    for err in errors {
        assert!(err.to_string().contains("claude-acp"), "{err}");
    }
}

#[test]
fn an_unreadable_item_is_an_error_naming_the_agent() {
    let err = super::into_key(&claude(), vec![0xff, 0xfe, b'k']).expect_err("not utf-8");
    assert_eq!(
        err.to_string(),
        "the keychain failed on claude-acp's API key: an unreadable item"
    );
}

#[test]
fn no_keychain_is_unavailable_stores_nothing_and_finds_nothing() {
    let keychain = Keychain::none();
    assert!(!keychain.available());
    assert_eq!(
        keychain.store(&claude(), KEY),
        Err(KeyError::NoKeychain { agent: claude() })
    );
    assert_eq!(keychain.load(&claude()).expect("no error"), None);
    keychain.clear(&claude()).expect("nothing to clear");
    assert!(Keychain::memory().available());
}

/// The platform rule, checked without touching the real keychain: off macOS and Windows the OS
/// keychain is none. (On macOS and Windows, `available` would open the store, so it is not
/// called here; by-hand row 2 covers it.)
#[test]
fn the_os_keychain_follows_the_platform_rule() {
    let debug = format!("{:?}", Keychain::os());
    if cfg!(any(target_os = "macos", windows)) {
        assert_eq!(debug, "Keychain(os)");
    } else {
        assert_eq!(debug, "Keychain(none)");
        assert!(!super::available());
        assert!(super::load(&claude()).expect("no error").is_none());
        assert!(super::clear(&claude()).is_ok());
        assert!(matches!(
            super::store(&claude(), KEY),
            Err(KeyError::NoKeychain { .. })
        ));
    }
}
