//! The Tauri updater signature: good, tampered, wrong file name, another key, malformed.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use clauseo_engines::{Error, MAX_JSON, MAX_SIGNATURE, load_with_key};

use crate::fixtures::{
    APP_VERSION_SIG, BY_B_SIG, GOOD, KEY_A, KEY_B, NEWER, OTHER_NAME_SIG, bundled, check, context,
};

fn load(json: &[u8], sig: &[u8], key: &str) -> Result<(), Error> {
    let bundled = bundled();
    load_with_key(json, sig, &context(&bundled), key).map(|_| ())
}

/// The minisign file inside a Tauri `.sig`.
fn sig_file(sig: &[u8]) -> String {
    String::from_utf8(STANDARD.decode(sig).unwrap()).unwrap()
}

#[test]
fn the_fixture_is_what_tauri_signer_sign_writes() {
    let file = sig_file(GOOD.sig);
    let lines: Vec<&str> = file.lines().collect();
    assert_eq!(lines.len(), 4, "{file}");
    assert_eq!(
        lines[0],
        "untrusted comment: signature from tauri secret key"
    );
    // `ED`: prehashed.
    assert!(STANDARD.decode(lines[1]).unwrap().starts_with(b"ED"));
    let comment = lines[2]
        .strip_prefix("trusted comment: timestamp:")
        .unwrap();
    let (digits, file_name) = comment.split_once('\t').unwrap();
    assert!(digits.bytes().all(|b| b.is_ascii_digit()));
    assert_eq!(file_name, "file:engines.json");
}

#[test]
fn a_good_list_loads() {
    let list = check(&GOOD).unwrap();
    assert_eq!(list.seq(), 2);
    assert_eq!(list.generated(), "2026-11-24T06:00:00Z");
    assert_eq!(list.apps(), ["0.1.0", "0.0.0-dev.1a2b3c4"]);
    assert_eq!(list.json(), GOOD.json);
    assert_eq!(list.sizes().len(), 12);
    let live: serde_json::Value =
        serde_json::from_str(include_str!("../../../limits/tests/live/package-lock.json")).unwrap();
    assert_eq!(list.package_lock(), &live);
    assert_eq!(list.package_json(), &bundled());
    assert_eq!(check(&NEWER).unwrap().seq(), 3);
}

#[test]
fn a_changed_byte_is_refused() {
    let mut json = GOOD.json.to_vec();
    let at = json.len() / 2;
    json[at] ^= 1;
    assert_eq!(load(&json, GOOD.sig, KEY_A), Err(Error::Signature));
    // A trailing line break is a changed file too.
    let mut longer = GOOD.json.to_vec();
    longer.push(b'\n');
    assert_eq!(load(&longer, GOOD.sig, KEY_A), Err(Error::Signature));
}

#[test]
fn a_new_list_beside_the_old_signature_is_refused() {
    assert_eq!(load(NEWER.json, GOOD.sig, KEY_A), Err(Error::Signature));
}

#[test]
fn a_signature_by_another_key_is_refused() {
    assert_eq!(load(GOOD.json, BY_B_SIG, KEY_A), Err(Error::Signature));
    assert_eq!(load(GOOD.json, GOOD.sig, KEY_B), Err(Error::Signature));
    assert_eq!(load(GOOD.json, BY_B_SIG, KEY_B), Ok(()));
}

#[test]
fn a_signature_naming_another_file_is_refused() {
    // Signed by the right key over the right bytes, for `update.tar.gz`.
    assert!(sig_file(OTHER_NAME_SIG).contains("\tfile:update.tar.gz\n"));
    assert_eq!(load(GOOD.json, OTHER_NAME_SIG, KEY_A), Err(Error::Comment));
}

#[test]
fn a_trusted_comment_with_more_than_the_file_is_refused() {
    assert!(sig_file(APP_VERSION_SIG).contains("\tfile:engines.json\tversion:0.1.0\n"));
    assert_eq!(load(GOOD.json, APP_VERSION_SIG, KEY_A), Err(Error::Comment));
}

#[test]
fn a_tampered_trusted_comment_is_refused() {
    let file = sig_file(GOOD.sig);
    let tampered = file.replace("timestamp:", "timestamp:1");
    assert_ne!(tampered, file);
    let sig = STANDARD.encode(tampered);
    assert_eq!(
        load(GOOD.json, sig.as_bytes(), KEY_A),
        Err(Error::Signature)
    );
}

#[test]
fn a_legacy_signature_is_refused() {
    // The same signature relabelled `Ed` (legacy, not prehashed): never accepted.
    let file = sig_file(GOOD.sig);
    let mut lines: Vec<String> = file.lines().map(str::to_owned).collect();
    let mut bin = STANDARD.decode(&lines[1]).unwrap();
    bin[1] = b'd';
    lines[1] = STANDARD.encode(bin);
    let sig = STANDARD.encode(lines.join("\n") + "\n");
    assert_eq!(
        load(GOOD.json, sig.as_bytes(), KEY_A),
        Err(Error::Signature)
    );
}

#[test]
fn a_signature_that_is_not_tauri_s_form_is_refused() {
    // The bare minisign file, not wrapped in base64.
    let bare = sig_file(GOOD.sig);
    assert_eq!(
        load(GOOD.json, bare.as_bytes(), KEY_A),
        Err(Error::SignatureFile)
    );
    assert_eq!(load(GOOD.json, b"", KEY_A), Err(Error::SignatureFile));
    assert_eq!(
        load(GOOD.json, b"aGVsbG8=", KEY_A),
        Err(Error::SignatureFile)
    );
    let big = vec![b'A'; MAX_SIGNATURE + 1];
    assert_eq!(load(GOOD.json, &big, KEY_A), Err(Error::SignatureFile));
    // A trailing line break around the base64 is fine.
    let mut padded = GOOD.sig.to_vec();
    padded.extend_from_slice(b"\r\n");
    assert_eq!(load(GOOD.json, &padded, KEY_A), Ok(()));
}

#[test]
fn a_key_that_is_not_tauri_s_form_verifies_nothing() {
    let bare_line = String::from_utf8(STANDARD.decode(KEY_A.trim()).unwrap()).unwrap();
    let bare_line = bare_line.lines().nth(1).unwrap().to_owned();
    assert_eq!(load(GOOD.json, GOOD.sig, &bare_line), Err(Error::NoKey));
    assert_eq!(load(GOOD.json, GOOD.sig, ""), Err(Error::NoKey));
}

#[test]
fn oversize_json_is_refused_before_anything_else() {
    let big = vec![b' '; MAX_JSON + 1];
    assert_eq!(load(&big, b"", ""), Err(Error::TooBig));
    let mut padded = GOOD.json.to_vec();
    padded.resize(MAX_JSON + 1, b' ');
    assert_eq!(load(&padded, GOOD.sig, KEY_A), Err(Error::TooBig));
}
