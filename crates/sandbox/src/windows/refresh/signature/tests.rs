//! The signature check: a key made in the test signs good, tampered, foreign and malformed
//! `.sig` files, as `tauri signer` writes them.

use super::super::key::PLACEHOLDER_KEY;
use super::super::test_signer::{TestKey, comment};
use super::*;

const OWN: &str = "1.4.0";

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
    assert_eq!(verify(&exe, &sig, &key.public()), Ok(()));
    // A trailing line break, as an editor or the updater may leave, changes nothing.
    let mut with_newline = sig;
    with_newline.extend_from_slice(b"\r\n");
    assert_eq!(verify(&exe, &with_newline, &key.public()), Ok(()));
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
fn a_bad_built_in_key_is_named() {
    let exe = exe();
    let sig = TestKey::new(1).sign(&exe, &good_comment());
    assert!(matches!(
        verify(&exe, &sig, "not a key"),
        Err(SignatureError::BadKey(_))
    ));
}
