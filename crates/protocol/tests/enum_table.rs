//! 0002's table of logged enums (under Freeze rules) against the code. Every row names an enum
//! of `support/enums.rs`, the one list of the enums a log entry can hold, every one has a row,
//! and each row's class is what the type does: an open enum decodes an unknown kind to its
//! catch-all, a closed one refuses it (`RawPayload` holds it as plain data).
//!
//! With `--features specta`, the list is also checked against the specta type graph walked from
//! the logged roots, `Envelope` and `DomainEvent`. Specta sees an enum only where the export
//! describes one: [`EXPORTED_AS`] lists the enums the export describes as another type
//! (`RawPayload`, `StreamId`). A new enum described that way, or one hidden behind a field's
//! `specta(type = ..)`, is not seen.
#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]

#[cfg(test)]
#[path = "support/markdown.rs"]
pub mod markdown;

#[cfg(test)]
#[macro_use]
#[path = "support/enums.rs"]
mod enums;

use std::collections::BTreeSet;
use std::fmt::Debug;

use clauseo_protocol as p;
use serde::de::DeserializeOwned;
#[cfg(feature = "specta")]
use specta::Types;
#[cfg(feature = "specta")]
use specta::datatype::{DataType, Reference};

use helpers::landing;

/// The decision record that holds the table.
const RECORD: &str = include_str!("../../../docs/decisions/0002-event-schema.md");

/// A kind no version of Clauseo writes.
const LATER: &str = "zz_later_kind";

/// A row's class, as the table's second column starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Open,
    Closed,
}

/// What an unknown kind decodes to, the same in JSON and in `MessagePack`. The kind is [`LATER`],
/// as a plain string and as the key of a one-key map; a form that lands in the catch-all wins
/// (`DomainEvent` takes only the map, `ErrorCode` only the string).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Landing {
    /// An error, in every form.
    Refused,
    /// A value that is no catch-all (only `RawPayload`, which holds any value as data).
    Data,
    /// The catch-all.
    CatchAll,
}

// In a `#[cfg(test)]` module, so the helpers may unwrap and panic as the tests do.
#[cfg(test)]
mod helpers {
    use super::*;

    /// Decodes [`LATER`] in both forms as `T`; `catch_all` says which values are its catch-all.
    pub(super) fn landing<T: DeserializeOwned + Debug>(catch_all: fn(&T) -> bool) -> Landing {
        let mut landed = Landing::Refused;
        for json in [
            format!(r#""{LATER}""#),
            format!(r#"{{"{LATER}":{{"a":[1]}}}}"#),
        ] {
            let raw: p::RawPayload = serde_json::from_str(&json).unwrap();
            let packed = rmp_serde::to_vec_named(&raw).unwrap();
            let decoded = [
                serde_json::from_str::<T>(&json).map_err(|e| e.to_string()),
                rmp_serde::from_slice::<T>(&packed).map_err(|e| e.to_string()),
            ];
            let [from_json, from_packed] = decoded.each_ref().map(|got| match got {
                Ok(value) if catch_all(value) => Landing::CatchAll,
                Ok(_) => Landing::Data,
                Err(_) => Landing::Refused,
            });
            assert_eq!(
                from_json, from_packed,
                "{json}: JSON and MessagePack differ: {decoded:?}"
            );
            landed = landed.max(from_json);
        }
        landed
    }

    /// The table's rows: each name in the first column with the class in the second. The names
    /// are the first cell's code spans; an open row gives no reason, a closed one does.
    pub(super) fn table_rows() -> Vec<(String, Class)> {
        let table = markdown::table(RECORD, "Freeze rules", "Enum");
        let (names, class, why) = (
            table.column("Enum"),
            table.column("Open or closed"),
            table.column("Why closed"),
        );
        let mut rows = Vec::new();
        for row in &table.rows {
            let (class, why) = (row[class].text.as_str(), row[why].text.as_str());
            let class = if class.starts_with("open") {
                assert!(why.is_empty(), "an open row gives no reason: {row:?}");
                Class::Open
            } else {
                assert!(
                    class.starts_with("closed"),
                    "neither open nor closed: {row:?}"
                );
                assert!(!why.is_empty(), "a closed row gives its reason: {row:?}");
                Class::Closed
            };
            assert!(!row[names].code.is_empty(), "a row names no enum: {row:?}");
            rows.extend(row[names].code.iter().map(|name| (name.clone(), class)));
        }
        rows
    }
}

/// One row: the enum's name, its class, and where an unknown kind lands.
type Row = (&'static str, Class, fn() -> Landing);

/// The rows of `logged_enums!`'s list, each with its class and its catch-all.
macro_rules! enum_rows {
    (
        $event:ident($event_catch:ident);
        $($data:ident($catch:ident)),+;
        $($unit:ident),+;
        $($closed:ident),+
    ) => {
        &[
            (stringify!($event), Class::Open, || {
                landing::<p::$event>(|value| matches!(value, p::$event::$event_catch { .. }))
            }),
            $((stringify!($data), Class::Open, || {
                landing::<p::$data>(|value| matches!(value, p::$data::$catch { .. }))
            }),)+
            $((stringify!($unit), Class::Open, || {
                landing::<p::$unit>(|value| matches!(value, p::$unit::Unknown))
            }),)+
            $((stringify!($closed), Class::Closed, || landing::<p::$closed>(|_| false)),)+
        ]
    };
}

/// Every enum reachable from a logged type, with its class.
const ROWS: &[Row] = logged_enums!(rows);

/// Enums the export describes as another named type: the enum, then that type.
#[cfg(feature = "specta")]
const EXPORTED_AS: &[(&str, &str)] = &[
    // Described by a private `JsonValue`, the shape of any JSON value.
    ("RawPayload", "JsonValue"),
    // One string, `<kind>:<uuid>`.
    ("StreamId", "String"),
];

/// Unions the export adds that are no Rust enum of their own: `DomainEvent`'s known kinds, for
/// the webview to narrow to.
#[cfg(feature = "specta")]
const EXPORT_ONLY: &[&str] = &["KnownDomainEvent"];

/// The enums the export reaches from the logged roots, each under its Rust name.
#[cfg(feature = "specta")]
fn reachable_enums() -> BTreeSet<String> {
    let types = Types::default()
        .register::<p::Envelope>()
        .register::<p::DomainEvent>();
    let mut found = BTreeSet::new();
    for ndt in types.into_unsorted_iter() {
        let name = ndt.name.to_string();
        match &ndt.ty {
            Some(DataType::Enum(_)) if EXPORT_ONLY.contains(&name.as_str()) => {}
            Some(DataType::Enum(_)) => {
                let described = EXPORTED_AS.iter().find(|(_, target)| *target == name);
                found.insert(described.map_or(name, |(rust, _)| (*rust).to_owned()));
            }
            Some(DataType::Reference(Reference::Named(target))) => {
                let target = types.get(target).map(|t| t.name.to_string());
                let listed = EXPORTED_AS.iter().find(|(rust, _)| *rust == name);
                if let Some((rust, expected)) = listed {
                    assert_eq!(target.as_deref(), Some(*expected), "{rust} is exported as");
                    found.insert(name);
                }
            }
            _ => {}
        }
    }
    found
}

/// Each name once, sorted.
fn unique(names: impl IntoIterator<Item = String>, what: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for name in names {
        assert!(set.insert(name.clone()), "{name} is twice in {what}");
    }
    set
}

#[cfg(feature = "specta")]
#[test]
fn rows_are_the_reachable_enums() {
    let listed = unique(ROWS.iter().map(|(name, ..)| (*name).to_owned()), "ROWS");
    assert_eq!(
        reachable_enums(),
        listed,
        "the enums reachable from Envelope and DomainEvent (left) and ROWS (right)"
    );
}

#[test]
fn the_table_has_one_row_per_enum_with_its_class() {
    let table = helpers::table_rows();
    let named = unique(table.iter().map(|(name, _)| name.clone()), "0002's table");
    let listed = unique(ROWS.iter().map(|(name, ..)| (*name).to_owned()), "ROWS");
    assert_eq!(named, listed, "0002's table (left) and ROWS (right)");
    for (name, class) in &table {
        let row = ROWS.iter().find(|(listed, ..)| listed == name);
        assert_eq!(
            row.map(|(_, class, _)| class),
            Some(class),
            "{name}'s class in 0002"
        );
    }
}

#[test]
fn each_class_is_what_the_type_does() {
    for (name, class, landing) in ROWS {
        let landing = landing();
        let expected: &[Landing] = match class {
            Class::Open => &[Landing::CatchAll],
            Class::Closed if *name == "RawPayload" => &[Landing::Data],
            Class::Closed => &[Landing::Refused],
        };
        assert!(
            expected.contains(&landing),
            "{name} is {class:?} but an unknown kind lands as {landing:?}"
        );
    }
}
