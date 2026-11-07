//! The updater key's id, the base64 decoder `build.rs` shares, and the release guard.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use super::key::*;

/// The throwaway test key a test release signs with.
const TEST_PUB: &str = include_str!("../../../../desktop/release/updater-test.pub");

#[test]
fn the_ids_are_minisigns() {
    assert_eq!(key_id(PLACEHOLDER_KEY), Ok(PLACEHOLDER_KEY_ID));
    assert_eq!(key_id(TEST_PUB), Ok(TEST_KEY_ID));
    // A trailing line break, as a file or an env var may carry, is fine.
    assert_eq!(key_id(&format!("{TEST_PUB}\n")), Ok(TEST_KEY_ID));
    assert_eq!(format!("{PLACEHOLDER_KEY_ID:016X}"), "AC986E7C6BA67AB9");
    assert_eq!(format!("{TEST_KEY_ID:016X}"), "B142942ACEC5F521");
}

#[test]
fn the_built_in_key_is_a_key() {
    // Whatever this build embedded (the placeholder, here, unless CLAUSEO_UPDATER_PUBKEY or
    // tauri.conf.json gives one), `build.rs` checked it, and it reads back.
    assert!(key_id(super::UPDATER_KEY).is_ok());
    assert!(super::OWN_VERSION.split('.').count() >= 3);
}

#[test]
fn a_malformed_key_is_refused() {
    let file = |text: &str| STANDARD.encode(text);
    let short = file("untrusted comment: x\nRWQ=\n");
    let not_ed = file(&format!(
        "untrusted comment: x\n{}\n",
        STANDARD.encode([b'X'; 42])
    ));
    for bad in ["", "%%%", "QQ==", &file("one line only"), &short, &not_ed] {
        assert!(key_id(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn base64_decodes_as_rfc_4648_says() {
    for (text, bytes) in [
        ("", &b""[..]),
        ("Zg==", b"f"),
        ("YWI=", b"ab"),
        ("Zm9v", b"foo"),
        ("Zm9vYg==", b"foob"),
        ("Zm9vYmE=", b"fooba"),
        ("Zm9vYmFy", b"foobar"),
        ("+/+/", &[0xfb, 0xff, 0xbf]),
    ] {
        assert_eq!(decode_base64(text).as_deref(), Some(bytes), "{text}");
    }
    for bad in [
        "Zg=", "Z===", "Zg==Zg==", "Zm9v!", "Zm9", "Zm 9v", "Zm-_", "=Zg=",
    ] {
        assert_eq!(decode_base64(bad), None, "{bad}");
    }
    // Against the base64 crate, over every length up to three blocks.
    let bytes: Vec<u8> = (0..=255).collect();
    for len in 0..bytes.len() {
        let text = STANDARD.encode(&bytes[..len]);
        assert_eq!(decode_base64(&text).as_deref(), Some(&bytes[..len]));
    }
}

#[test]
fn the_release_guard_refuses_only_a_release_with_a_key_that_signs_no_release() {
    let real = 0x0123_4567_89AB_CDEF;
    for (mode, profile, id, refused) in [
        (Some("release"), "release", PLACEHOLDER_KEY_ID, true),
        (Some("release"), "release", TEST_KEY_ID, true),
        (Some("release"), "release", real, false),
        // A dry or test run, or no mode: any key.
        (None, "release", PLACEHOLDER_KEY_ID, false),
        (Some("dry"), "release", PLACEHOLDER_KEY_ID, false),
        (Some("test"), "release", TEST_KEY_ID, false),
        (Some(""), "release", TEST_KEY_ID, false),
        // A debug build in release mode (a local check): any key.
        (Some("release"), "debug", PLACEHOLDER_KEY_ID, false),
        (Some("release"), "debug", TEST_KEY_ID, false),
    ] {
        let verdict = release_guard(mode, profile, id);
        assert_eq!(verdict.is_err(), refused, "{mode:?} {profile} {id:016X}");
        if let Err(why) = verdict {
            assert!(why.contains(&format!("{id:016X}")), "{why}");
        }
    }
}
