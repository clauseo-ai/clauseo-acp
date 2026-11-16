//! Unit tests of the updater rules' key decoding. The keys checked here come from the committed
//! `.pub` files, so the ids this check refuses stay those files' ids; the rules themselves are
//! planted cases (`tests/planted.rs`).

use super::*;

/// A committed `.pub` file of desktop's `release` folder, as Tauri's form of a key.
pub(in crate::checks::tauri_config) fn release_pub(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../crates/desktop/release")
        .join(name);
    std::fs::read_to_string(path)
        .expect("read the committed key")
        .trim()
        .to_owned()
}

/// Standard base64 with padding.
fn encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (at, byte)| {
            n | (u32::from(*byte) << (16 - 8 * at))
        });
        for at in 0..4 {
            if at <= chunk.len() {
                let index = usize::try_from((n >> (18 - 6 * at)) & 63).unwrap();
                out.push(char::from(ALPHABET[index]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Tauri's form of a minisign public key whose key bytes carry id `id` (then 32 bytes of 0x11)
/// and whose comment names `named`. Made for these tests; no key pair exists.
fn made_key(id: u64, named: &str) -> String {
    let mut key = b"Ed".to_vec();
    key.extend_from_slice(&id.to_le_bytes());
    key.extend_from_slice(&[0x11; 32]);
    encode(
        format!(
            "untrusted comment: minisign public key: {named}\n{}\n",
            encode(&key)
        )
        .as_bytes(),
    )
}

/// A well-formed key that is neither the placeholder nor the test key: id 0123456789ABCDEF.
pub(in crate::checks::tauri_config) fn other_key() -> String {
    made_key(0x0123_4567_89AB_CDEF, "0123456789ABCDEF")
}

#[test]
fn the_committed_keys_have_the_ids_the_check_refuses() {
    assert_eq!(
        key_id(&release_pub("updater-placeholder.pub")).as_deref(),
        Ok(PLACEHOLDER_KEY_ID)
    );
    assert_eq!(
        key_id(&release_pub("updater-test.pub")).as_deref(),
        Ok(TEST_KEY_ID)
    );
    assert_eq!(key_id(&other_key()).as_deref(), Ok("0123456789ABCDEF"));
}

#[test]
fn a_key_whose_comment_names_another_id_is_refused() {
    let renamed = made_key(0x0123_4567_89AB_CDEF, "FEDCBA9876543210");
    let why = key_id(&renamed).unwrap_err();
    assert!(
        why.contains("FEDCBA9876543210") && why.contains("0123456789ABCDEF"),
        "{why}"
    );
}

#[test]
fn a_secret_key_and_text_that_is_no_key_are_refused() {
    // The first line of a secret key file as minisign and `tauri signer` write it.
    let secret = encode(b"untrusted comment: rsign encrypted secret key\nRWRTY0Iy\n");
    assert!(key_id(&secret).unwrap_err().contains("SECRET"));
    assert!(key_id("not base64!").unwrap_err().contains("not base64"));
    // "hello\n": base64, but no minisign comment.
    assert!(key_id("aGVsbG8K").unwrap_err().contains("comment"));
}

#[test]
fn base64_decodes_with_and_without_padding() {
    assert_eq!(base64("aGk=").as_deref(), Some(b"hi".as_slice()));
    assert_eq!(base64("aGk").as_deref(), Some(b"hi".as_slice()));
    assert_eq!(base64("a-b"), None);
}
