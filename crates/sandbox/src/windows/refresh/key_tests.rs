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
fn a_release_build_needs_the_pinned_production_key() {
    let real = 0x0123_4567_89AB_CDEF;
    let pinned = Some("0123456789ABCDEF");
    for (mode, id, pin, refused) in [
        // A release: only the pinned key, in either case; never with no pin.
        (Some("release"), real, pinned, false),
        (Some("release"), real, Some("0123456789abcdef"), false),
        (Some("release"), PLACEHOLDER_KEY_ID, pinned, true),
        (Some("release"), TEST_KEY_ID, pinned, true),
        (Some("release"), 0x0123_4567_89AB_CDEE, pinned, true),
        (Some("release"), real, None, true),
        (Some("release"), TEST_KEY_ID, None, true),
        // A dry or test run, a local build, or no mode: any key.
        (None, PLACEHOLDER_KEY_ID, None, false),
        (Some("dry"), PLACEHOLDER_KEY_ID, pinned, false),
        (Some("test"), TEST_KEY_ID, pinned, false),
        // Any other mode, a typo or the empty string, fails: it must not turn the guard off.
        (Some(""), TEST_KEY_ID, None, true),
        (Some("Release"), real, pinned, true),
        (Some("release "), TEST_KEY_ID, pinned, true),
        (Some("local"), PLACEHOLDER_KEY_ID, None, true),
    ] {
        let verdict = release_guard_with(mode, id, pin);
        assert_eq!(verdict.is_err(), refused, "{mode:?} {id:016X} {pin:?}");
        if let Err(why) = verdict {
            assert!(why.contains(&format!("{id:016X}")), "{why}");
        }
    }
}

#[test]
fn the_pinned_key_is_never_one_that_signs_no_release() {
    match PRODUCTION_KEY_ID {
        // Until #527 pins it, a release build fails whatever its key.
        None => assert!(release_guard(Some("release"), PRODUCTION_KEY_SAMPLE).is_err()),
        Some(pinned) => {
            assert_eq!(pinned.len(), 16, "{pinned}");
            for never in [PLACEHOLDER_KEY_ID, TEST_KEY_ID] {
                assert!(
                    !pinned.eq_ignore_ascii_case(&format!("{never:016X}")),
                    "{pinned}"
                );
            }
        }
    }
    assert!(release_guard(Some("release"), TEST_KEY_ID).is_err());
    assert!(release_guard(Some("dry"), PLACEHOLDER_KEY_ID).is_ok());
    assert_eq!(KEY_ID_VERSION_STRING, "UpdaterKeyId");
}

/// Any key id that isn't the placeholder's or the test key's.
const PRODUCTION_KEY_SAMPLE: u64 = 0x0123_4567_89AB_CDEF;
