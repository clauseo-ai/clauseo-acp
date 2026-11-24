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

use base64::Engine as _;

fn base64_decode(text: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .unwrap()
}

#[test]
fn the_release_guard_needs_the_release_key_in_a_release_build() {
    fn guard(mode: Option<&str>, key: Option<&str>) -> Result<(), &'static str> {
        release_guard(mode, key, TAURI_CONF)
    }
    let release = conf_pubkey();
    assert_eq!(guard(None, None), Ok(()));
    assert_eq!(guard(Some("test"), Some(KEY_A)), Ok(()));
    assert_eq!(guard(Some("dry"), None), Ok(()));
    assert_eq!(guard(Some("release"), Some(&release)), Ok(()));
    // A trailing line break, as an environment variable may carry, is not part of the key.
    assert_eq!(
        guard(Some("release"), Some(&format!("{release}\n"))),
        Ok(())
    );
    assert!(guard(Some("release"), Some(KEY_A)).is_err());
    assert!(guard(Some("release"), None).is_err());
    assert!(guard(Some("release"), Some("")).is_err());
    assert!(guard(Some("release"), Some("garbage")).is_err());
    assert!(guard(Some(""), None).is_err());
    assert!(guard(Some("Release"), Some(&release)).is_err());
    // The release key's id with other bytes (another untrusted comment): not the config's key.
    let file = String::from_utf8(base64_decode(&release)).unwrap();
    let renamed = file.replacen("minisign public key", "minisign  public key", 1);
    let same_id = base64::engine::general_purpose::STANDARD.encode(renamed);
    assert_eq!(key_id(&same_id), Some(RELEASE_KEY_ID));
    assert!(guard(Some("release"), Some(&same_id)).is_err());
    // A config with no pubkey, or two.
    assert!(release_guard(Some("release"), Some(&release), "{}").is_err());
    let twice = format!("{{\"pubkey\": \"{release}\", \"pubkey\": \"{release}\"}}");
    assert!(release_guard(Some("release"), Some(&release), &twice).is_err());
}

#[test]
fn key_ids_need_canonical_base64_as_the_runtime_does() {
    use base64::engine::general_purpose::STANDARD;
    const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let file = String::from_utf8(base64_decode(&conf_pubkey())).unwrap();
    // The release key's file with line breaks added until its base64 ends in padding.
    let padded = (0..3)
        .map(|extra| STANDARD.encode(format!("{file}{}", "\n".repeat(extra))))
        .find(|key| key.ends_with('='))
        .unwrap();
    assert_eq!(key_id(&padded), Some(RELEASE_KEY_ID));
    // Padding dropped, or one too many.
    assert_eq!(key_id(padded.trim_end_matches('=')), None);
    assert_eq!(key_id(&format!("{padded}=")), None);
    // Non-zero bits under the padding, which the runtime's decoder refuses too.
    let at = padded.rfind(|c| c != '=').unwrap();
    let value = ALPHABET.find(&padded[at..=at]).unwrap();
    assert_eq!(
        value & 1,
        0,
        "canonical base64 leaves the dropped bits zero"
    );
    let mut changed = padded;
    changed.replace_range(at..=at, &ALPHABET[value | 1..=value | 1]);
    assert!(STANDARD.decode(&changed).is_err());
    assert_eq!(key_id(&changed), None);
    // A space inside the key line.
    let spaced = file.replacen("\nRW", "\nR W", 1);
    assert_eq!(key_id(&STANDARD.encode(spaced)), None);
}

#[test]
fn a_config_pubkey_with_a_json_escape_is_refused_cleanly() {
    let release = conf_pubkey();
    // The same key, its first character written as a `\u00XX` escape as JSON allows: the
    // const scan refuses an escape rather than misread it.
    let first = release.chars().next().unwrap();
    let written = format!("\\u{:04x}{}", u32::from(first), &release[1..]);
    let escaped = TAURI_CONF.replacen(release.as_str(), &written, 1);
    assert_ne!(escaped, TAURI_CONF);
    let reread: serde_json::Value = serde_json::from_str(&escaped).unwrap();
    assert_eq!(reread["plugins"]["updater"]["pubkey"], release.as_str());
    assert!(release_guard(Some("release"), Some(&release), &escaped).is_err());
}
