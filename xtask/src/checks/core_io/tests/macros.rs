//! The core-io planted cases about macros: attributes a macro forwards whole, definitions that
//! another macro fills in, and brackets a `#` passed as a `tt` can join.

use super::cfgs::with_file;
use crate::checks::Inspection;

/// The review's case: the protocol crate's way to forward doc comments and other attributes into
/// a macro's items, `$(#[$meta:meta])*` in the matcher and `$(#[$meta])*` in the transcriber, at
/// every depth of repetition, beside `#[doc = $doc]` and an attribute with a metavariable value.
pub(in crate::checks::core_io) fn forwarded_attributes_pass() -> Inspection {
    with_file("forwarded-attributes", FORWARDED)
}

/// A trimmed copy of the protocol crate's macros that forward attributes, and two calls.
const FORWARDED: &str = r#"macro_rules! open_unit_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $($(#[$variant_meta:meta])* $variant:ident,)+
        }
    ) => {
        $(#[$meta])*
        #[cfg_attr(feature = "specta", derive(specta::Type))]
        $vis enum $name {
            $($(#[$variant_meta])* $variant,)+
            Unknown,
        }
    };
}
macro_rules! enum_with_all {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident $({
                    $($(#[$field_meta:meta])* $field:ident: $ty:ty = $placeholder:expr),+ $(,)?
                })?
            ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant $({ $($(#[$field_meta])* $field: $ty),+ })?,
            )+
        }
    };
}
macro_rules! domain_events {
    ($($(#[$doc:meta])* $variant:ident($ty:ty) = $kind:literal,)+) => {
        pub enum DomainEvent {
            $($(#[$doc])* #[serde(rename = $kind)] $variant($ty),)+
        }
    };
}
macro_rules! error_codes {
    ($($(#[doc = $doc:literal])* $variant:ident = $code:literal;)+) => {
        pub enum ErrorCode {
            $($(#[doc = $doc])* #[serde(rename = $code)] $variant,)+
        }
    };
}
macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident, $check:path) => {
        $(#[$meta])*
        pub struct $name(String);
    };
}
open_unit_enum! {
    /// A colour.
    pub enum Colour {
        /// Red.
        Red,
    }
}
string_id! {
    /// A file reference.
    #[derive(Debug)]
    FileRefId, check
}
"#;

/// The review's case with a matcher that binds the forwarded attribute as `$m:tt`, which a
/// caller can fill with `cfg(not(clippy))` and no brackets of its own.
pub(in crate::checks::core_io) fn forwarded_attribute_as_tt() -> Inspection {
    with_file(
        "forwarded-tt",
        "macro_rules! fwd {\n    ($(#[$m:tt])* $name:ident) => {\n        $(#[$m])*\n        \
         pub struct $name;\n    };\n}\n",
    )
}

/// A matcher that binds the forwarded attribute's name as `$m:ident`.
pub(in crate::checks::core_io) fn forwarded_attribute_as_ident() -> Inspection {
    with_file(
        "forwarded-ident",
        "macro_rules! fwd {\n    (#[$m:ident] $name:ident) => {\n        #[$m]\n        \
         pub struct $name;\n    };\n}\n",
    )
}

/// A `macro_rules!` inside a transcriber: the outer macro fills in both the inner matcher's
/// `#[$m:meta]` and the inner `#[$m]`, so the caller never writes brackets.
pub(in crate::checks::core_io) fn forwarded_meta_in_a_nested_definition() -> Inspection {
    with_file(
        "forwarded-nested",
        r#"macro_rules! outer {
    ($m:meta $(, $k:tt)?) => {
        macro_rules! inner {
            ($(#[$m:meta] $k)?) => { #[$m] pub fn g() { let _ = std::fs::read("x"); } }
        }
        inner!();
    };
}
outer!(cfg(any()));
"#,
    )
}

/// A `macro_rules!` in a macro call's arguments is no definition, so its "matcher" gets the
/// full rules.
pub(in crate::checks::core_io) fn definition_shape_in_call_arguments() -> Inspection {
    with_file(
        "definition-in-call",
        r#"macro_rules! emit { (macro_rules! $n:ident { ($($c:tt)*) => {} }) => { $($c)* }; }
macro_rules! a { ($($p:tt)*) => { emit! { macro_rules! fake {
    (#[$($p)*] pub fn h() { let _ = std::fs::read("x"); }) => {} } } }; }
a!(cfg(any()));
"#,
    )
}

/// The review's case: a `#` passed as a `tt` joins brackets the transcriber writes.
pub(in crate::checks::core_io) fn hash_joins_written_brackets() -> Inspection {
    with_file(
        "hash-joins-brackets",
        "macro_rules! a { ($h:tt $($p:tt)*) => { $h [$($p)*] pub fn k() {} }; }\n\
         a!(# cfg(not(clippy)));\n",
    )
}

/// The same with the `#` passed through a repetition.
pub(in crate::checks::core_io) fn repetition_joins_written_brackets() -> Inspection {
    with_file(
        "repetition-joins-brackets",
        "macro_rules! r { ($($h:tt)?; $($p:tt)*) => { $($h)? [$($p)*] pub fn k() {} }; }\n\
         r!(#; cfg(not(clippy)));\n",
    )
}

/// The review's case: a definition inside a macro call's arguments gets no bindings, so the
/// `$h` before the brackets counts as unbound.
pub(in crate::checks::core_io) fn joined_brackets_in_call_arguments() -> Inspection {
    with_file(
        "joined-in-call",
        "macro_rules! id { ($($t:tt)*) => { $($t)* }; }\n\
         id! { macro_rules! fake { ($h:tt $($p:tt)*) => { $h [$($p)*] pub fn j() {} }; } }\n\
         fake!(# cfg(not(clippy)));\n",
    )
}

/// Indexing after a metavariable bound as `ident`, which cannot stand for a `#`.
pub(in crate::checks::core_io) fn indexing_after_an_ident_passes() -> Inspection {
    with_file(
        "indexing-ident",
        "macro_rules! at { ($v:ident, $i:expr) => { $v[$i] }; }\n\
         pub fn f(v: &[u8]) -> u8 {\n    at!(v, 0)\n}\n",
    )
}

/// The review's case: a forwarding macro that passes, called with a cfg clippy never sees, which
/// fails where the caller writes it.
pub(in crate::checks::core_io) fn forwarded_cfg_at_the_call() -> Inspection {
    with_file(
        "forwarded-cfg-call",
        "macro_rules! fwd {\n    ($(#[$m:meta])* $name:ident) => {\n        $(#[$m])*\n        \
         pub struct $name;\n    };\n}\nfwd!(#[cfg(not(clippy))] S);\n",
    )
}
