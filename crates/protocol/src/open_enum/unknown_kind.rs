//! [`UnknownKind`], the kind a catch-all holds.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::Deref;

/// The kind of a catch-all variant (`Unknown { kind }` of `DomainEvent` and of each enum with
/// data), as read: a kind this version of `protocol` does not know.
///
/// Only `protocol` builds one, when it decodes a kind it does not know. The type stops a crate
/// from building one by accident; it does not stop a crate from decoding one, which any crate
/// can do with `serde_json::from_str::<DetachReason>("\"crashed\"")`, and a decoded one clones.
/// It reads as the `str` it holds (it derefs to `str` and compares with one) and is written as
/// that string. `T` is the enum it belongs to, whose own kinds the
/// writer checks it against (writing an `Unknown` under a known kind is an error).
///
/// No other crate can build one:
///
/// ```compile_fail,E0624
/// let kind = clauseo_protocol::UnknownKind::<clauseo_protocol::DetachReason>::new("crashed");
/// ```
pub struct UnknownKind<T> {
    kind: String,
    of: PhantomData<fn() -> T>,
}

impl<T> UnknownKind<T> {
    /// The kind `kind`. Only the decoders call it, with a kind `T` does not know.
    pub(crate) fn new(kind: &str) -> Self {
        Self {
            kind: kind.to_owned(),
            of: PhantomData,
        }
    }

    /// The kind as read.
    pub fn as_str(&self) -> &str {
        &self.kind
    }
}

impl<T> Deref for UnknownKind<T> {
    type Target = str;

    fn deref(&self) -> &str {
        &self.kind
    }
}

// Written by hand: a derive would bound each on `T`, which the enums need not meet.
impl<T> Clone for UnknownKind<T> {
    fn clone(&self) -> Self {
        Self {
            kind: self.kind.clone(),
            of: PhantomData,
        }
    }
}

impl<T> fmt::Debug for UnknownKind<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.kind, f)
    }
}

impl<T> PartialEq for UnknownKind<T> {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

impl<T> Eq for UnknownKind<T> {}

impl<T> PartialEq<str> for UnknownKind<T> {
    fn eq(&self, other: &str) -> bool {
        self.kind == other
    }
}

impl<T> Hash for UnknownKind<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.kind.hash(state);
    }
}
