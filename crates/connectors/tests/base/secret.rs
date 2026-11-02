//! `Secret` never prints: `Debug` is `Secret(..)`, and it has no `Display` and no `Serialize`.

use clauseo_connectors::{Bearer, Secret};

/// Fails to compile when `T` implements `Trait`: with an impl, the two blanket impls below make
/// the call ambiguous (the trick of the `static_assertions` crate's `assert_not_impl_any!`).
macro_rules! assert_not_impl {
    ($t:ty, $trait:path) => {{
        trait Ambiguous<A> {
            fn some_item() {}
        }
        impl<T: ?Sized> Ambiguous<()> for T {}
        struct Invalid;
        impl<T: ?Sized + $trait> Ambiguous<Invalid> for T {}
        let _ = <$t as Ambiguous<_>>::some_item;
    }};
}

#[test]
fn debug_prints_no_secret() {
    let secret = Secret::from("at_super_secret_value");
    assert_eq!(format!("{secret:?}"), "Secret(..)");
    assert_eq!(format!("{secret:#?}"), "Secret(..)");
    let bearer = Bearer(secret.clone());
    assert_eq!(format!("{bearer:?}"), "Bearer(Secret(..))");
    assert_eq!(secret.expose(), "at_super_secret_value");
}

#[test]
fn secret_has_no_display_and_no_serialize() {
    assert_not_impl!(Secret, std::fmt::Display);
    assert_not_impl!(Secret, serde::Serialize);
    assert_not_impl!(Bearer, std::fmt::Display);
    assert_not_impl!(Bearer, serde::Serialize);
}

#[test]
fn relay_tokens_are_64_hex_characters() {
    let token = clauseo_connectors::new_relay_token();
    assert_eq!(token.len(), 64);
    assert!(token.expose().bytes().all(|b| b.is_ascii_hexdigit()));
}
