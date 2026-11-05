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
    let zero_width = format!("{KEY}\u{200b}");
    let byte_order_mark = format!("{KEY}\u{feff}");
    let accented = format!("{KEY}é");
    for key in [
        "",
        long.as_str(),
        pasted.as_str(),
        spaced.as_str(),
        "a\u{0}b",
        zero_width.as_str(),
        byte_order_mark.as_str(),
        accented.as_str(),
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
fn printable_ascii_keys_pass_including_dash_and_underscore() {
    let keychain = Keychain::memory();
    let keys = [
        "sk-ant-api03_AbC-123!~",
        "a",
        "~!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}",
    ];
    for key in keys {
        keychain
            .store(&claude(), key)
            .expect("printable ASCII is taken");
        let loaded = keychain.load(&claude()).expect("loaded").expect("a key");
        assert_eq!(loaded.as_str(), key);
    }
}

/// Plants `bytes` as `agent`'s item, as another app or Keychain Access would.
fn plant(keychain: &Keychain, agent: &AgentId, bytes: &[u8]) {
    let super::Backend::Memory(items, _) = &keychain.backend else {
        panic!("plant needs the memory keychain");
    };
    super::lock(items).insert(account(agent), super::Zeroizing::new(bytes.to_vec()));
}

#[test]
fn load_checks_a_stored_key_as_store_does() {
    let keychain = Keychain::memory();
    plant(&keychain, &claude(), b"sk-LEAKED\nTAIL");
    let err = keychain.load(&claude()).expect_err("an invalid stored key");
    assert_eq!(err, KeyError::StoredInvalid { agent: claude() });
    let text = err.to_string();
    assert!(text.contains("claude-acp"), "{text}");
    assert!(text.contains("the stored key is not valid"), "{text}");
    assert!(!text.contains("LEAKED") && !text.contains("TAIL"), "{text}");
    assert!(!format!("{err:?}").contains("LEAKED"), "{err:?}");

    plant(&keychain, &claude(), "sk-ant-\u{200b}x".as_bytes());
    assert!(keychain.load(&claude()).is_err());
    plant(&keychain, &claude(), b"");
    assert!(keychain.load(&claude()).is_err());

    plant(&keychain, &claude(), KEY.as_bytes());
    let loaded = keychain.load(&claude()).expect("loaded").expect("a key");
    assert_eq!(loaded.as_str(), KEY);
}

#[test]
fn into_key_refuses_a_key_with_a_line_break_and_keeps_a_valid_one() {
    let err = super::into_key(&claude(), b"k\nx".to_vec()).expect_err("a line break");
    assert_eq!(err, KeyError::StoredInvalid { agent: claude() });
    assert!(!err.to_string().contains("k\nx"), "{err}");
    assert!(!format!("{err:?}").contains("k\nx"), "{err:?}");
    let key = super::into_key(&claude(), KEY.as_bytes().to_vec()).expect("a valid key loads");
    assert_eq!(key.as_str(), KEY);
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

#[test]
fn store_if_absent_stores_only_when_no_key_is_there() {
    let keychain = Keychain::memory();
    assert_eq!(keychain.store_if_absent(&claude(), KEY), Ok(true));
    assert_eq!(keychain.store_if_absent(&claude(), "second"), Ok(false));
    let loaded = keychain.load(&claude()).expect("loaded").expect("a key");
    assert_eq!(loaded.as_str(), KEY, "the first key stays");
    assert_eq!(keychain.store_if_absent(&codex(), "codex-key"), Ok(true));
}

#[test]
fn store_if_absent_refuses_as_store_does_and_with_no_keychain() {
    let keychain = Keychain::memory();
    assert!(matches!(
        keychain.store_if_absent(&claude(), "has space"),
        Err(KeyError::Invalid { .. })
    ));
    assert_eq!(keychain.load(&claude()).expect("loaded"), None);
    assert_eq!(
        Keychain::none().store_if_absent(&claude(), KEY),
        Err(KeyError::NoKeychain { agent: claude() })
    );
}

#[test]
fn store_if_absent_racing_on_two_threads_stores_exactly_one() {
    for round in 0..200 {
        let keychain = Keychain::memory();
        let barrier = std::sync::Barrier::new(2);
        let (first, second) = std::thread::scope(|scope| {
            let run = |key: &'static str| {
                let keychain = keychain.clone();
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    keychain.store_if_absent(&claude(), key).expect("no error")
                })
            };
            let first = run("first-key");
            let second = run("second-key");
            (
                first.join().expect("first thread"),
                second.join().expect("second thread"),
            )
        });
        assert!(first ^ second, "round {round}: exactly one stores");
        let kept = keychain.load(&claude()).expect("loaded").expect("a key");
        let winner = if first { "first-key" } else { "second-key" };
        assert_eq!(
            kept.as_str(),
            winner,
            "round {round}: the winner's key is kept"
        );
    }
}

#[test]
fn a_memory_keychain_failing_reads_refuses_store_if_absent_and_writes_nothing() {
    let keychain = Keychain::memory();
    keychain.fail_reads(true);
    assert!(matches!(
        keychain.load(&claude()),
        Err(KeyError::NoAccess { .. })
    ));
    assert!(matches!(
        keychain.store_if_absent(&claude(), "sk-first"),
        Err(KeyError::NoAccess { .. })
    ));
    keychain.fail_reads(false);
    assert_eq!(keychain.load(&claude()).unwrap(), None, "nothing written");
}
