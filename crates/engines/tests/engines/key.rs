//! The compiled-in updater key: the build's `CLAUSEO_UPDATER_PUBKEY`, else tauri.conf.json's.

use clauseo_engines::{
    Error, RELEASE_KEY_ID, TAURI_CONF, UPDATER_KEY_FROM_ENV, key_id, load, release_guard,
    updater_key,
};

use crate::fixtures::{GOOD, KEY_A, KEY_B, bundled, context};

/// tauri.conf.json's `plugins.updater.pubkey`, read here on its own.
fn conf_pubkey() -> String {
    let conf: serde_json::Value =
        serde_json::from_str(include_str!("../../../desktop/tauri.conf.json")).unwrap();
    conf["plugins"]["updater"]["pubkey"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn the_compiled_in_key_is_tauri_conf_s_when_the_variable_is_unset() {
    assert_eq!(TAURI_CONF, include_str!("../../../desktop/tauri.conf.json"));
    let set = UPDATER_KEY_FROM_ENV.is_some_and(|key| !key.trim().is_empty());
    let expected = match UPDATER_KEY_FROM_ENV {
        Some(key) if set => key.split_ascii_whitespace().collect::<String>(),
        _ => conf_pubkey(),
    };
    assert_eq!(updater_key().as_deref(), Some(expected.as_str()));
    if !set {
        assert_eq!(updater_key(), Some(conf_pubkey()));
    }
}

#[test]
fn the_compiled_in_key_is_not_a_test_key() {
    let key = updater_key().unwrap();
    assert_ne!(key, KEY_A.trim());
    assert_ne!(key, KEY_B.trim());
    // So the fixtures, signed by test keys, are refused by the app's own check.
    let bundled = bundled();
    assert_eq!(
        load(GOOD.json, GOOD.sig, &context(&bundled)).map(|_| ()),
        Err(Error::Signature)
    );
}

#[test]
fn the_compiled_in_key_decodes() {
    // Garbage in CLAUSEO_UPDATER_PUBKEY fails here, not silently in the app.
    let key = updater_key().expect("an updater key is compiled in");
    assert!(
        key_id(&key).is_some(),
        "the compiled-in key has no key id: {key}"
    );
    // The real check gets past the key to the signature file.
    let bundled = bundled();
    assert_eq!(
        load(GOOD.json, b"", &context(&bundled)).map(|_| ()),
        Err(Error::SignatureFile)
    );
}

#[test]
fn key_ids_read_as_minisign_prints_them() {
    assert_eq!(RELEASE_KEY_ID, 0xF6A2_5D15_11B9_2EBE);
    assert_eq!(key_id(&conf_pubkey()), Some(0xF6A2_5D15_11B9_2EBE));
    for key in [KEY_A, KEY_B] {
        let id = key_id(key).unwrap();
        let file = String::from_utf8(base64_decode(key)).unwrap();
        assert!(file.starts_with(&format!(
            "untrusted comment: minisign public key: {id:016X}\n"
        )));
    }
    assert_eq!(key_id(""), None);
    assert_eq!(key_id("not base64!"), None);
    // A file whose second line isn't a key.
    assert_eq!(key_id("dW50cnVzdGVkIGNvbW1lbnQ6IHgKUlc="), None);
}

fn base64_decode(text: &str) -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .unwrap()
}

#[test]
fn the_release_guard_needs_the_release_key_in_a_release_build() {
    let release = conf_pubkey();
    assert_eq!(release_guard(None, None), Ok(()));
    assert_eq!(release_guard(Some("test"), Some(KEY_A)), Ok(()));
    assert_eq!(release_guard(Some("dry"), None), Ok(()));
    assert_eq!(release_guard(Some("release"), Some(&release)), Ok(()));
    assert!(release_guard(Some("release"), Some(KEY_A)).is_err());
    assert!(release_guard(Some("release"), None).is_err());
    assert!(release_guard(Some("release"), Some("")).is_err());
    assert!(release_guard(Some("release"), Some("garbage")).is_err());
    assert!(release_guard(Some(""), None).is_err());
    assert!(release_guard(Some("Release"), Some(&release)).is_err());
}
