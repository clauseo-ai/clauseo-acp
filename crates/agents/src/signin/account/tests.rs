//! The account hash: normalised, hex, never shown; and what a status's email reads as.

use super::{AccountHash, AccountRead, named};
use crate::status::Identity;

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

fn identity(email: Option<&str>, id: Option<&str>) -> Identity {
    Identity {
        email: email.map(str::to_owned),
        plan: None,
        account_id: id.map(str::to_owned),
    }
}

#[test]
fn the_email_names_the_account_else_the_id_else_none() {
    let hash = |text: &str| AccountRead::Account(AccountHash::of(text).unwrap());
    assert_eq!(
        named(&identity(Some("a@b.c"), Some("uuid-1"))),
        hash("a@b.c")
    );
    assert_eq!(
        named(&identity(None, Some("uuid-1"))),
        hash("uuid-1"),
        "an id is enough"
    );
    assert_eq!(named(&identity(Some(" "), Some("uuid-1"))), hash("uuid-1"));
    assert_eq!(named(&identity(None, None)), AccountRead::NoAccount);
    assert_eq!(
        named(&identity(Some(" "), Some(""))),
        AccountRead::NoAccount
    );
}
