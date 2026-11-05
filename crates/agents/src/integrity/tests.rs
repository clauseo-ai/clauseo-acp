use super::{Expected, short_hash, to_hex};

#[test]
fn reads_a_lock_integrity_and_names_its_file() {
    // The SHA-512 of the empty input.
    let integrity = "sha512-z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysP+DGNKHfuwvY7kxvUdBeoGlODJ6+SfaPg==";
    let expected = Expected::from_integrity(integrity).unwrap();
    assert_eq!(&expected.hex()[..16], "cf83e1357eefb8bd");
    assert!(expected.hasher().matches(&expected));
    let mut other = expected.hasher();
    other.update(b"x");
    assert!(!other.matches(&expected));
}

#[test]
fn refuses_what_is_not_one_sha512() {
    for bad in [
        "sha1-2jmj7l5rSw0yVb/vlWAYkK/YBwk=",
        "sha512-AAAA",
        "sha512-z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysP+DGNKHfuwvY7kxvUdBeoGlODJ6+SfaPg== sha1-2jmj7l5rSw0yVb/vlWAYkK/YBwk=",
        "",
    ] {
        assert!(Expected::from_integrity(bad).is_err(), "{bad}");
    }
}

#[test]
fn sha256_from_hex() {
    let hex = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    let expected = Expected::sha256_hex(hex).unwrap();
    assert_eq!(expected.hex(), hex);
    assert!(expected.hasher().matches(&expected));
    assert!(Expected::sha256_hex("e3b0").is_none());
    assert!(Expected::sha256_hex(&hex.replace('e', "g")).is_none());
}

#[test]
fn short_hash_is_16_hex_and_separates_parts() {
    let one = short_hash(&[b"ab", b"c"]);
    let two = short_hash(&[b"a", b"bc"]);
    assert_eq!(one.len(), 16);
    assert_ne!(one, two);
    assert_eq!(to_hex(&[0x0f, 0xa0]), "0fa0");
}
