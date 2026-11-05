//! Sign-in and installs on the wire (decision record 0048): `SetupCommand` decodes from its JSON
//! and refuses what it does not know, a `Secret` never shows its text, `SetupEvent` and the
//! states in it round-trip, and the install and sign-in codes decode. The tests are in
//! `setup/wire.rs`.

#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]

#[cfg(test)]
#[path = "support/fixtures.rs"]
pub mod fixtures;

#[cfg(test)]
#[path = "setup/wire.rs"]
mod wire;
