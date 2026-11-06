//! The account hash: normalised, hex, never shown; and what a status's email reads as.

use super::{AccountHash, AccountRead, named};

#[test]
fn the_hash_is_of_the_trimmed_lower_cased_identity() {
    let one = AccountHash::of("  Lawyer@Example.COM \n").unwrap();
    let two = AccountHash::of("lawyer@example.com").unwrap();
    assert_eq!(one, two);
    assert_ne!(one, AccountHash::of("other@example.com").unwrap());
    assert_eq!(one.as_hex().len(), 64);
    assert_eq!(
        one.as_hex(),
        "468ba474328fa8bc7d3e708f9c8562f488110c3c249e0cbb343a5012f9a7747d",
        "SHA-256 of lawyer@example.com"
    );
    assert_eq!(AccountHash::from_hex(one.as_hex()), Some(one));
    assert_eq!(AccountHash::of("   "), None);
}

#[test]
fn only_64_lowercase_hex_digits_read_back() {
    let hex = "a".repeat(64);
    assert!(AccountHash::from_hex(&hex).is_some());
    assert!(AccountHash::from_hex(&"A".repeat(64)).is_none());
    assert!(AccountHash::from_hex(&"a".repeat(63)).is_none());
    assert!(AccountHash::from_hex(&"g".repeat(64)).is_none());
}

#[test]
fn debug_shows_no_digit_of_the_hash() {
    let hash = AccountHash::of("lawyer@example.com").unwrap();
    let shown = format!("{:?}", AccountRead::Account(hash.clone()));
    assert!(!shown.contains(&hash.as_hex()[..8]), "{shown}");
}

#[test]
fn a_status_with_no_email_names_no_account() {
    assert_eq!(named(None), AccountRead::NoAccount);
    assert_eq!(named(Some(" ")), AccountRead::NoAccount);
    assert_eq!(
        named(Some("a@b.c")),
        AccountRead::Account(AccountHash::of("a@b.c").unwrap())
    );
}
