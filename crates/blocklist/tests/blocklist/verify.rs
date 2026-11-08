//! The minisign check: good, bad, wrong key, rotated key, tampered comment, legacy.

use clauseo_blocklist::{
    Error, PUBLIC_KEYS, STAGE_F_KEY_FILE, key_line, load, trusted_comment, verify,
};

use crate::fixtures::{
    BY_B, BY_C, LEGACY, LIST, MISCOUNTED, NEWER, Signed, generated, key_a, key_b, key_c,
};

fn check(signed: &Signed, keys: &[&str]) -> Result<u32, Error> {
    load(signed.gz, signed.minisig, keys).map(|list| u32::try_from(list.len()).unwrap())
}

#[test]
fn a_good_signature_verifies_and_the_comment_gives_the_header() {
    let header = verify(LIST.gz, LIST.minisig, &[key_a()]).unwrap();
    assert_eq!(header.generated, generated("2026-11-07T06:00:00Z"));
    assert_eq!(header.entries, 6);
    assert_eq!(check(&LIST, &[key_a()]), Ok(6));
}

#[test]
fn the_signature_part_alone_gives_the_trusted_comment_unparsed() {
    assert_eq!(
        trusted_comment(LIST.gz, LIST.minisig, &[key_a()]).as_deref(),
        Ok("clauseo-blocklist v1 generated=2026-11-07T06:00:00Z entries=6")
    );
    assert_eq!(
        trusted_comment(BY_C.gz, BY_C.minisig, &[key_a(), key_b()]),
        Err(Error::Signature)
    );
    assert_eq!(
        trusted_comment(LIST.gz, LIST.minisig, &["not a key"]),
        Err(Error::NoKey)
    );
    assert_eq!(
        trusted_comment(LIST.gz, b"hello", &[key_a()]),
        Err(Error::SignatureFile)
    );
}

#[test]
fn a_changed_byte_in_the_list_is_refused() {
    let mut gz = LIST.gz.to_vec();
    let last = gz.len() - 9;
    gz[last] ^= 1;
    assert_eq!(
        load(&gz, LIST.minisig, &[key_a()]).map(|_| ()),
        Err(Error::Signature)
    );
}

#[test]
fn a_new_list_beside_the_old_signature_is_refused() {
    // The release's assets are not replaced at once: for a moment the new .gz sits beside the
    // old .minisig.
    assert_eq!(
        load(NEWER.gz, LIST.minisig, &[key_a()]).map(|_| ()),
        Err(Error::Signature)
    );
}

#[test]
fn a_key_not_in_the_set_is_refused() {
    assert_eq!(check(&BY_C, &[key_a(), key_b()]), Err(Error::Signature));
    assert_eq!(check(&LIST, &[key_b()]), Err(Error::Signature));
}

#[test]
fn a_list_signed_by_the_second_of_two_keys_verifies() {
    assert_eq!(check(&BY_B, &[key_a(), key_b()]), Ok(6));
}

#[test]
fn a_tampered_trusted_comment_is_refused() {
    let text = std::str::from_utf8(LIST.minisig).unwrap();
    for (from, to) in [
        ("entries=6", "entries=5"),
        (
            "generated=2026-11-07T06:00:00Z",
            "generated=2027-11-07T06:00:00Z",
        ),
    ] {
        let tampered = text.replace(from, to);
        assert_ne!(tampered, text);
        assert_eq!(
            load(LIST.gz, tampered.as_bytes(), &[key_a()]).map(|_| ()),
            Err(Error::Signature),
            "{to}"
        );
    }
}

#[test]
fn a_comment_that_is_not_the_v1_form_is_refused_even_when_signed() {
    // The miscounted fixture is well formed but disagrees with its list.
    assert_eq!(
        check(&MISCOUNTED, &[key_a()]),
        Err(Error::EntriesMismatch { said: 7, found: 6 })
    );
}

#[test]
fn a_legacy_signature_is_refused() {
    assert_eq!(check(&LEGACY, &[key_a()]), Err(Error::Signature));
}

#[test]
fn a_signature_file_that_is_not_minisign_or_too_big_is_refused() {
    assert_eq!(
        verify(LIST.gz, b"hello", &[key_a()]),
        Err(Error::SignatureFile)
    );
    let big = vec![b'a'; clauseo_blocklist::MAX_SIGNATURE + 1];
    assert_eq!(verify(LIST.gz, &big, &[key_a()]), Err(Error::SignatureFile));
}

#[test]
fn a_set_with_no_key_line_verifies_nothing() {
    assert_eq!(
        verify(LIST.gz, LIST.minisig, &["not a key"]),
        Err(Error::NoKey)
    );
    assert_eq!(verify(LIST.gz, LIST.minisig, &[]), Err(Error::NoKey));
}

#[test]
fn stage_f_s_key_file_gives_the_expected_key_line() {
    assert_eq!(
        key_line(STAGE_F_KEY_FILE),
        Some("RWTakAlW8Ifn6GnQ/16tBGUkBaP429oE+MboPzMwVk23KxJ+ICwPjUj7")
    );
    assert!(
        STAGE_F_KEY_FILE.starts_with("untrusted comment: minisign public key E8E787F0560990DA\n")
    );
}

#[test]
fn a_key_file_and_its_bare_key_line_verify_alike() {
    let file = include_str!("../data/key-a.pub");
    assert_eq!(key_line(file), Some(key_a()));
    assert_eq!(check(&LIST, &[file]), Ok(6));
    assert_eq!(key_line("untrusted comment: no key line\n"), None);
    assert_eq!(key_line(""), None);
}

#[test]
fn the_compiled_in_keys_are_key_files_and_hold_no_test_key() {
    assert_eq!(PUBLIC_KEYS, &[STAGE_F_KEY_FILE]);
    for file in PUBLIC_KEYS {
        let key = key_line(file).unwrap();
        assert!(key.starts_with("RW") && key.len() == 56, "{key}");
        assert!(![key_a(), key_b(), key_c()].contains(&key));
    }
    // The fixtures are signed by test keys only.
    assert_eq!(check(&LIST, PUBLIC_KEYS), Err(Error::Signature));
}
