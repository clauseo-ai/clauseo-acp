//! The signature check: a key made in the test signs good, tampered, foreign and malformed
//! `.sig` files, as `tauri signer` writes them.

use super::super::key::PLACEHOLDER_KEY;
use super::super::test_signer::{TestKey, comment};
use super::*;

const OWN: &str = "1.4.0";

/// Whether a refusal is the one a case expects.
type Expected = fn(&SignatureError) -> bool;

fn exe() -> Vec<u8> {
    // Past a BLAKE2b block many times over, so the prehash covers more than one block.
    (0..200 * 1024).map(|n: u32| (n % 251) as u8).collect()
}

fn good_comment() -> String {
    comment(SIGNED_FILE, OWN)
}

#[test]
fn a_good_signature_verifies() {
    let key = TestKey::new(0x1122_3344_5566_7788);
    let exe = exe();
    let sig = key.sign(&exe, &good_comment());
    let signed = Signed {
        version: OWN.to_owned(),
    };
    assert_eq!(verify(&exe, &sig, &key.public()), Ok(signed.clone()));
    // A trailing line break, as an editor or the updater may leave, changes nothing.
    let mut with_newline = sig;
    with_newline.extend_from_slice(b"\r\n");
    assert_eq!(verify(&exe, &with_newline, &key.public()), Ok(signed));
}

#[test]
fn one_tampered_byte_fails() {
    let key = TestKey::new(0x1122_3344_5566_7788);
    let exe = exe();
    let sig = key.sign(&exe, &good_comment());
    for at in [0, 4096, exe.len() - 1] {
        let mut tampered = exe.clone();
        tampered[at] ^= 0x01;
        assert_eq!(
            verify(&tampered, &sig, &key.public()),
            Err(SignatureError::Invalid),
            "byte {at}"
        );
    }
    let mut longer = exe;
    longer.push(0);
    assert_eq!(
        verify(&longer, &sig, &key.public()),
        Err(SignatureError::Invalid)
    );
}

#[test]
fn a_tampered_trusted_comment_fails() {
    // The comment is the signer's only through the global signature: a newer version written
    // into it afterwards breaks that signature.
    let key = TestKey::new(0x1122_3344_5566_7788);
    let exe = exe();
    let sig = key.sign(&exe, &comment(SIGNED_FILE, "1.0.0"));
    let file =
        String::from_utf8(decode_base64(std::str::from_utf8(&sig).unwrap()).unwrap()).unwrap();
    let edited = file.replace("version:1.0.0", "version:9.0.0");
    assert_ne!(file, edited);
    let edited = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, edited);
    assert_eq!(
        verify(&exe, edited.as_bytes(), &key.public()),
        Err(SignatureError::Invalid)
    );
}

#[test]
fn another_key_fails() {
    let signer = TestKey::new(0x1122_3344_5566_7788);
    let exe = exe();
    let sig = signer.sign(&exe, &good_comment());
    // Another id: refused before the curve math.
    let other = TestKey::new(0x0102_0304_0506_0708);
    assert_eq!(
        verify(&exe, &sig, &other.public()),
        Err(SignatureError::WrongKey)
    );
    // The same id on another key (a forged id): the signature doesn't verify.
    let impostor = TestKey::new(0x1122_3344_5566_7788);
    assert_eq!(
        verify(&exe, &sig, &impostor.public()),
        Err(SignatureError::Invalid)
    );
}

#[test]
fn the_placeholder_key_verifies_nothing() {
    // Even a signature made under the placeholder's id is refused by the id alone.
    let signer = TestKey::new(PLACEHOLDER_KEY_ID);
    let exe = exe();
    let sig = signer.sign(&exe, &good_comment());
    assert_eq!(
        verify(&exe, &sig, PLACEHOLDER_KEY),
        Err(SignatureError::PlaceholderKey)
    );
}

#[test]
fn garbage_is_malformed() {
    let key = TestKey::new(0x1122_3344_5566_7788);
    let exe = exe();
    let b64 = |text: &str| {
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, text).into_bytes()
    };
    for garbage in [
        b"".to_vec(),
        b"not base64 at all!".to_vec(),
        vec![0xff, 0xfe, 0x00],
        b64("untrusted comment: x\nnot a signature\n"),
        b64("just one line"),
        // Valid base64 lines of the wrong lengths.
        b64("untrusted comment: x\nRUQ=\ntrusted comment: y\nRUQ=\n"),
    ] {
        assert!(
            matches!(
                verify(&exe, &garbage, &key.public()),
                Err(SignatureError::Malformed(_))
            ),
            "{garbage:?}"
        );
    }
}

#[test]
fn a_legacy_signature_is_refused() {
    let key = TestKey::new(0x1122_3344_5566_7788);
    let exe = exe();
    let sig = key.sign_legacy(&exe, &good_comment());
    assert_eq!(
        verify(&exe, &sig, &key.public()),
        Err(SignatureError::NotPrehashed)
    );
}

#[test]
fn the_comment_must_name_this_exe() {
    let key = TestKey::new(0x1122_3344_5566_7788);
    let exe = exe();
    for comment_text in [
        comment("Clauseo_1.4.0_x64-setup.exe", OWN),
        comment("clauseo-sandbox-setup.exe.sig", OWN),
        // The name only as a substring, or under another key: not the file.
        comment("xclauseo-sandbox-setup.exe", OWN),
        comment("C:\\out\\clauseo-sandbox-setup.exe", OWN),
        comment("clauseo-sandbox-setup.exe ", OWN),
        comment("Clauseo-Sandbox-Setup.exe", OWN),
        format!("timestamp:1\tnotfile:{SIGNED_FILE}\tversion:{OWN}"),
        format!("timestamp:1\tversion:{OWN}\tfilename:{SIGNED_FILE}"),
        format!("timestamp:1\tversion:{OWN}"),
        String::new(),
    ] {
        let sig = key.sign(&exe, &comment_text);
        assert_eq!(
            verify(&exe, &sig, &key.public()),
            Err(SignatureError::WrongFile),
            "{comment_text:?}"
        );
    }
}

#[test]
fn the_comment_is_matched_by_key_in_any_order_with_extra_fields() {
    let key = TestKey::new(0x1122_3344_5566_7788);
    let exe = exe();
    for comment_text in [
        // tauri-cli 2.12's own form.
        format!("timestamp:1762500000\tfile:{SIGNED_FILE}\tversion:{OWN}"),
        // Reordered, without a timestamp, with an extra field, with a ':' in a value.
        format!("version:{OWN}\tfile:{SIGNED_FILE}\ttimestamp:1"),
        format!("file:{SIGNED_FILE}\tversion:{OWN}"),
        format!("timestamp:1\tfile:{SIGNED_FILE}\tversion:{OWN}\tbuilt-by:ci:windows"),
    ] {
        let sig = key.sign(&exe, &comment_text);
        assert_eq!(
            verify(&exe, &sig, &key.public()).map(|signed| signed.version),
            Ok(OWN.to_owned()),
            "{comment_text:?}"
        );
    }
}

#[test]
fn a_missing_version_or_a_malformed_comment_is_refused() {
    let key = TestKey::new(0x1122_3344_5566_7788);
    let exe = exe();
    let verdict = |comment_text: &str| verify(&exe, &key.sign(&exe, comment_text), &key.public());
    assert_eq!(
        verdict(&format!("timestamp:1\tfile:{SIGNED_FILE}")),
        Err(SignatureError::NoVersion)
    );
    assert_eq!(
        verdict(&format!("timestamp:1\tfile:{SIGNED_FILE}\tversion:")),
        Err(SignatureError::NoVersion)
    );
    // A key given twice can't say which value the signer meant; a field with no ':' isn't one.
    for comment_text in [
        format!("file:{SIGNED_FILE}\tversion:{OWN}\tversion:9.9.9"),
        format!("file:x.exe\tfile:{SIGNED_FILE}\tversion:{OWN}"),
        format!("file:{SIGNED_FILE}\tversion:{OWN}\tstray"),
        format!("file:{SIGNED_FILE}\t\tversion:{OWN}"),
    ] {
        assert!(
            matches!(verdict(&comment_text), Err(SignatureError::BadComment(_))),
            "{comment_text:?}"
        );
    }
}

#[test]
fn a_bad_built_in_key_is_named() {
    let exe = exe();
    let sig = TestKey::new(1).sign(&exe, &good_comment());
    assert!(matches!(
        verify(&exe, &sig, "not a key"),
        Err(SignatureError::BadKey(_))
    ));
}

#[test]
fn the_comment_tauri_signer_writes_for_the_release_verifies_and_near_misses_dont() {
    // What release.yml's `tauri signer sign --app-version "$VERSION"` writes for the setup exe
    // (#648), as a fixed string.
    let key = TestKey::new(0x1122_3344_5566_7788);
    let exe = exe();
    let verdict = |comment_text: &str| verify(&exe, &key.sign(&exe, comment_text), &key.public());
    assert_eq!(
        verdict("timestamp:1762500000\tfile:clauseo-sandbox-setup.exe\tversion:1.4.0")
            .map(|signed| signed.version),
        Ok("1.4.0".to_owned())
    );
    let refusals: [(&str, Expected); 7] = [
        // `=` for `:`: no field is `key:value`.
        (
            "timestamp=1762500000\tfile=clauseo-sandbox-setup.exe\tversion=1.4.0",
            |e| matches!(e, SignatureError::BadComment(_)),
        ),
        // Spaces for tabs: one `timestamp` field, so no `file`.
        (
            "timestamp:1762500000 file:clauseo-sandbox-setup.exe version:1.4.0",
            |e| *e == SignatureError::WrongFile,
        ),
        // A field missing.
        ("timestamp:1762500000\tversion:1.4.0", |e| {
            *e == SignatureError::WrongFile
        }),
        (
            "timestamp:1762500000\tfile:clauseo-sandbox-setup.exe",
            |e| *e == SignatureError::NoVersion,
        ),
        // A field given twice, or a stray one with no `:`.
        (
            "timestamp:1762500000\tfile:clauseo-sandbox-setup.exe\tversion:1.4.0\tversion:9.0.0",
            |e| matches!(e, SignatureError::BadComment(_)),
        ),
        (
            "timestamp:1762500000\tfile:clauseo-sandbox-setup.exe\tversion:1.4.0\textra",
            |e| matches!(e, SignatureError::BadComment(_)),
        ),
        // `file:` with a path: not the bare name.
        (
            "timestamp:1762500000\tfile:target/release/clauseo-sandbox-setup.exe\tversion:1.4.0",
            |e| *e == SignatureError::WrongFile,
        ),
    ];
    for (comment_text, expected) in refusals {
        let error = verdict(comment_text).expect_err(comment_text);
        assert!(expected(&error), "{comment_text:?}: {error:?}");
    }
}
