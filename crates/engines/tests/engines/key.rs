//! The compiled-in updater key: the build's `CLAUSEO_UPDATER_PUBKEY`, else tauri.conf.json's.

use clauseo_engines::{Error, TAURI_CONF, UPDATER_KEY_FROM_ENV, load, updater_key};

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
