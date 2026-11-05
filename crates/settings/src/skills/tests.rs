//! The pack id rule, held to `profile`'s, and the hash form.

use super::{hash_ok, is_pack_id};

/// The ids `profile`'s `tests/packs.rs` refuses in `a_bad_or_reserved_id_is_refused` and
/// `a_rendered_name_never_equals_a_builtin` (with its `BUILTIN` and `STETDOC` written out), then
/// the rule's edges. A change to either crate's rule fails one of the two.
const REFUSED: [&str; 22] = [
    "clauseo",
    "clauseo-extra",
    "builtin",
    "stetdoc",
    "a",
    "a--b",
    "Nda",
    "nul",
    "legal-drafting",
    "legal-research",
    "matter-brief",
    "",
    "-ab",
    "ab-",
    "nda_kit",
    "nda kit",
    "nda.kit",
    "com1",
    "lpt9",
    "con",
    "abcdefghijklmnopqrstuvwxyz12345",
    "nda-kit\n",
];

/// The ids `profile`'s tests accept (`nda-kit`, `my-kit`, `legal`, `a1`), then the rule's
/// edges: 2 and 30 bytes, digits only, a name only near a reserved one.
const ACCEPTED: [&str; 10] = [
    "nda-kit",
    "my-kit",
    "legal",
    "a1",
    "ab",
    "abcdefghijklmnopqrstuvwxyz1234",
    "2024",
    "com10",
    "clauseokit",
    "legal-drafting-uk",
];

#[test]
fn the_pack_id_rule_matches_profile() {
    for id in REFUSED {
        assert!(!is_pack_id(id), "{id:?} should be refused");
    }
    for id in ACCEPTED {
        assert!(is_pack_id(id), "{id:?} should be accepted");
    }
}

#[test]
fn a_hash_is_sha256_and_64_lowercase_hex_digits() {
    let hex = "0123456789abcdef".repeat(4);
    assert!(hash_ok(&format!("sha256:{hex}")));
    for bad in [
        hex.clone(),
        format!("sha256:{}", hex.to_uppercase()),
        format!("sha256:{}", &hex[1..]),
        format!("sha256:{hex}0"),
        format!("SHA256:{hex}"),
        format!("sha512:{hex}"),
        format!("sha256:{}g", &hex[1..]),
        String::new(),
    ] {
        assert!(!hash_ok(&bad), "{bad}");
    }
}
