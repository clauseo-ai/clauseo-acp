//! The frozen corpus of log entries that must keep decoding. `corpus/v1/` holds entries written
//! at `SCHEMA_VERSION` 1 that hold every kind and every variant of every open enum then known,
//! data and unit alike, in JSON (`events.jsonl`) and in `MessagePack` (`events.msgpack`). A kind
//! added after v1 brings its own frozen entry under `corpus/added/`, listed in
//! [`ADDED_AFTER_V1`]; the ignored test `write_the_added_pairs` writes those files into
//! `target/` from `support/examples.rs`, for the author to move in. Every entry must keep
//! decoding with the current types, so a new required field in any variant, a renamed field or a
//! removed kind fails here. Nothing writes into `corpus/`; the files' fingerprints are pinned
//! below, so an edit to them shows up as a change to this test.

#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]

#[cfg(test)]
#[path = "support/fixtures.rs"]
pub mod fixtures;

#[cfg(test)]
#[path = "support/examples.rs"]
pub mod examples;

#[cfg(test)]
#[path = "support/markdown.rs"]
pub mod markdown;

#[cfg(test)]
#[macro_use]
#[path = "support/enums.rs"]
mod enums;

#[cfg(test)]
#[path = "corpus/writer.rs"]
mod writer;

use std::collections::BTreeSet;

use clauseo_protocol::{self as p, DomainEvent, Envelope, MAX_MSGPACK_DEPTH};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use helpers::{json_entries, json_lines, kinds, missing, packed_entries, table_open_enums, values};

/// A frozen set of log entries: its name under `corpus/`, its JSON (one `Envelope` per line,
/// as `serde_json::to_string` wrote it), its `MessagePack` (one array of the same entries, as
/// `rmp_serde::to_vec_named` wrote it), and the pinned `(byte length, FNV-1a 64)` of each.
type Corpus = (
    &'static str,
    &'static [u8],
    &'static [u8],
    (usize, u64),
    (usize, u64),
);

/// The entries written at `SCHEMA_VERSION` 1.
const V1: Corpus = (
    "v1/events",
    include_bytes!("corpus/v1/events.jsonl"),
    include_bytes!("corpus/v1/events.msgpack"),
    (49_904, 0x56b4_6013_47c7_5b7f),
    (34_613, 0x383c_376f_5604_7c2e),
);

/// What was added after v1, each with its own frozen entries: a new kind in
/// `corpus/added/<kind>.jsonl` and `.msgpack`, a new variant of an open enum (a unit value too)
/// in `corpus/added/<kind>-<variant>.jsonl` and `.msgpack`, `<kind>` being the event that holds it.
/// Each is written `("added/<name>", include_bytes!(..jsonl), include_bytes!(..msgpack),
/// (length, hash), (length, hash))`. Empty: nothing has been added since v1.
const ADDED_AFTER_V1: &[Corpus] = &[];

/// V1, then every file in [`ADDED_AFTER_V1`].
fn every_corpus() -> Vec<Corpus> {
    [V1].into_iter()
        .chain(ADDED_AFTER_V1.iter().copied())
        .collect()
}

/// The 64-bit FNV-1a hash of `bytes`.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The decision record whose enum table classifies every logged enum.
const RECORD: &str = include_str!("../../../docs/decisions/0002-event-schema.md");

// In a `#[cfg(test)]` module, so the helpers may unwrap and panic as the tests do.
#[cfg(test)]
mod helpers {
    use super::*;

    /// The lines of a corpus's JSON, without the newline that ends the last one.
    pub(super) fn json_lines(corpus: &Corpus) -> Vec<&'static [u8]> {
        let text = corpus.1.strip_suffix(b"\n").unwrap_or_else(|| {
            panic!("{}.jsonl does not end with a newline", corpus.0);
        });
        text.split(|byte| *byte == b'\n').collect()
    }

    /// Every entry of a corpus's JSON, decoded with the current types.
    pub(super) fn json_entries(corpus: &Corpus) -> Vec<Envelope> {
        json_lines(corpus)
            .into_iter()
            .enumerate()
            .map(|(i, line)| {
                serde_json::from_slice(line).unwrap_or_else(|error| {
                    panic!(
                        "{}.jsonl line {} no longer decodes: {error}",
                        corpus.0,
                        i + 1
                    )
                })
            })
            .collect()
    }

    /// Every entry of a corpus's `MessagePack`, decoded the way a store reads a row.
    pub(super) fn packed_entries(corpus: &Corpus) -> Vec<Envelope> {
        let mut deserializer = rmp_serde::Deserializer::from_read_ref(corpus.2);
        deserializer.set_max_depth(MAX_MSGPACK_DEPTH);
        Vec::<Envelope>::deserialize(&mut deserializer)
            .unwrap_or_else(|error| panic!("{}.msgpack no longer decodes: {error}", corpus.0))
    }

    /// The kinds the entries of `corpus` hold.
    pub(super) fn kinds(corpus: &Corpus) -> BTreeSet<String> {
        json_entries(corpus)
            .iter()
            .map(|entry| entry.event.kind().to_owned())
            .collect()
    }

    /// The kind a value names: a plain string (a unit variant) or the key of a one-key map.
    pub(super) fn kind_of(value: &Value) -> Option<&str> {
        match value {
            Value::String(kind) => Some(kind),
            Value::Object(map) if map.len() == 1 => map.keys().next().map(String::as_str),
            _ => None,
        }
    }

    /// `value` and every value inside it, at any depth.
    pub(super) fn walk<'a>(value: &'a Value, found: &mut Vec<&'a Value>) {
        found.push(value);
        match value {
            Value::Array(items) => items.iter().for_each(|item| walk(item, found)),
            Value::Object(map) => map.values().for_each(|item| walk(item, found)),
            _ => {}
        }
    }

    /// The variants named in `names` (the enum's variant names, catch-all `Unknown` aside) that
    /// no value in `values` holds: a value counts for a variant when it decodes as `T` to that
    /// variant and names the variant's own kind, so a catch-all such as
    /// `ContentBlock::Unsupported`, which takes any unknown kind, counts only for an
    /// `"unsupported"` key.
    pub(super) fn missing<T: Serialize + DeserializeOwned>(
        names: &[&'static str],
        name: fn(&T) -> &'static str,
        values: &[&Value],
    ) -> Vec<&'static str> {
        let mut found = BTreeSet::new();
        for value in values {
            let Ok(decoded) = serde_json::from_value::<T>((*value).clone()) else {
                continue;
            };
            let written = serde_json::to_value(&decoded).unwrap();
            if kind_of(&written).is_some() && kind_of(&written) == kind_of(value) {
                found.insert(name(&decoded));
            }
        }
        names
            .iter()
            .copied()
            .filter(|variant| *variant != "Unknown" && !found.contains(variant))
            .collect()
    }

    /// The enums 0002's table lists as open with the shape `shape` (`data` or `unit`),
    /// `DomainEvent` aside.
    pub(super) fn table_open_enums(shape: &str) -> BTreeSet<String> {
        let table = markdown::table(RECORD, "Freeze rules", "Enum");
        let (names, class, shapes) = (
            table.column("Enum"),
            table.column("Open or closed"),
            table.column("Shape"),
        );
        let mut found = BTreeSet::new();
        for row in &table.rows {
            if row[class].text.starts_with("open") && row[shapes].text == shape {
                found.extend(row[names].code.iter().cloned());
            }
        }
        found.remove("DomainEvent");
        found
    }

    /// Every value of the JSON lines of `corpora`, at any depth.
    pub(super) fn values(corpora: &[Corpus]) -> Vec<Value> {
        let lines = corpora.iter().flat_map(json_lines);
        lines
            .flat_map(|line| inside(&serde_json::from_slice(line).unwrap()))
            .collect()
    }

    /// `value` and every value inside it, at any depth, each cloned.
    pub(super) fn inside(value: &Value) -> Vec<Value> {
        let mut found = Vec::new();
        walk(value, &mut found);
        found.into_iter().cloned().collect()
    }
}

/// What [`missing`] finds for one enum in the corpus's values.
type Coverage = fn(&[&Value]) -> Vec<&'static str>;

/// `(name, coverage)` for each open enum with data in `logged_enums!`'s list, `DomainEvent`
/// aside, read through its `variant_name` and `VARIANT_NAMES`.
macro_rules! data_coverage {
    ($($name:ident),+) => {
        &[$((
            stringify!($name),
            (|v| missing(&p::$name::VARIANT_NAMES, p::$name::variant_name, v)) as Coverage,
        )),+]
    };
}

/// `(name, coverage)` for each open unit-only enum in `logged_enums!`'s list, read through its
/// `variant_name` and the names of its `ALL`.
macro_rules! unit_coverage {
    ($($name:ident),+) => {
        &[$((
            stringify!($name),
            (|v| {
                let names = p::$name::ALL.each_ref().map(p::$name::variant_name);
                missing(&names, p::$name::variant_name, v)
            }) as Coverage,
        )),+]
    };
}

/// Every open enum with data a log entry can hold, `DomainEvent` aside. 0002's enum table must
/// list the same names.
const OPEN_DATA_ENUMS: &[(&str, Coverage)] = logged_enums!(data_coverage);

/// Every open unit-only enum a log entry can hold. 0002's enum table must list the same names.
const OPEN_UNIT_ENUMS: &[(&str, Coverage)] = logged_enums!(unit_coverage);

#[test]
fn every_entry_decodes_to_a_known_kind_in_both_formats() {
    for corpus in every_corpus() {
        for (format, entries) in [
            ("JSON", json_entries(&corpus)),
            ("MessagePack", packed_entries(&corpus)),
        ] {
            for (i, entry) in entries.iter().enumerate() {
                let at = format!("{} {format} entry {}", corpus.0, i + 1);
                if corpus.0 == V1.0 {
                    assert_eq!(entry.schema_version(), 1, "{at}");
                }
                assert!(
                    !matches!(entry.event, DomainEvent::Unknown { .. }),
                    "{at} decodes to Unknown: {:?}",
                    entry.event
                );
            }
        }
    }
}

#[test]
fn json_and_msgpack_hold_the_same_entries() {
    for corpus in every_corpus() {
        let json = json_entries(&corpus);
        let packed = packed_entries(&corpus);
        assert_eq!(json.len(), packed.len(), "{}", corpus.0);
        for (i, (from_json, from_packed)) in json.iter().zip(&packed).enumerate() {
            assert_eq!(from_json, from_packed, "{} entry {}", corpus.0, i + 1);
        }
    }
}

/// A kind is never removed: every v1 kind is still known. And none is added without its own
/// frozen entry: the known kinds are v1's and those of the files added after it.
#[test]
fn the_known_kinds_are_v1_and_the_added_ones() {
    let known: BTreeSet<String> = DomainEvent::KINDS
        .iter()
        .map(|kind| (*kind).to_owned())
        .collect();
    let v1 = kinds(&V1);
    let removed: Vec<&String> = v1.difference(&known).collect();
    assert!(removed.is_empty(), "v1 kinds no longer known: {removed:?}");
    let mut held = v1;
    for added in ADDED_AFTER_V1 {
        let name = added
            .0
            .strip_prefix("added/")
            .expect("an added file lives in added/");
        let kind = name.split('-').next().unwrap_or(name);
        let found = kinds(added);
        assert!(
            found.contains(kind),
            "{} holds no {kind} entry: {found:?}",
            added.0
        );
        held.extend(found);
    }
    let unfrozen: Vec<&String> = known.difference(&held).collect();
    assert!(
        unfrozen.is_empty(),
        "kinds with no frozen entry: {unfrozen:?}; add corpus/added/<kind>.jsonl and .msgpack \
         and list them in ADDED_AFTER_V1 (see corpus/v1/README.md)"
    );
}

/// The variants of the open enums that no value in `values` holds, each as `(enum, variant)`,
/// those in [`WRITTEN_AS_ABSENT`] aside.
fn gaps(values: &[Value]) -> Vec<(&'static str, &'static str)> {
    let values: Vec<&Value> = values.iter().collect();
    OPEN_DATA_ENUMS
        .iter()
        .chain(OPEN_UNIT_ENUMS)
        .flat_map(|(name, missing)| missing(&values).into_iter().map(|variant| (*name, variant)))
        .filter(|gap| !WRITTEN_AS_ABSENT.contains(gap))
        .collect()
}

/// A field added to a variant without a default fails only where the corpus holds that variant,
/// so v1, with the files added after it, holds every variant of every open enum, each under its
/// own kind. A unit value counts for every enum its string decodes as, wherever it sits, so a
/// value spelled as one of another enum (`completed` is a `ToolCallStatus`, a `PlanStatus` and a
/// `CompactionStatus`) counts for each: a new unit variant spelled like a value the corpus holds
/// passes unseen here. The corpus README says so.
#[test]
fn the_corpus_holds_every_variant_of_every_open_enum() {
    for (shape, list) in [("data", OPEN_DATA_ENUMS), ("unit", OPEN_UNIT_ENUMS)] {
        let listed: BTreeSet<String> = list.iter().map(|(name, _)| (*name).to_owned()).collect();
        assert_eq!(
            listed,
            table_open_enums(shape),
            "open {shape} enums against 0002's table"
        );
    }
    let gaps = gaps(&values(&every_corpus()));
    assert!(
        gaps.is_empty(),
        "variants missing from the corpus: {gaps:?}"
    );
}

/// The variants written as a missing field, never as a value, so no value in the corpus holds
/// them: `Delivery::Queue` (a `user_message` without `delivery`).
/// [`a_variant_written_as_absent_decodes_from_an_entry_without_its_field`] checks each instead.
const WRITTEN_AS_ABSENT: [(&str, &str); 1] = [("Delivery", "Queue")];

/// `Delivery::Queue` is written as no `delivery` field, and the corpus holds a `user_message`
/// without one that decodes to it.
#[test]
fn a_variant_written_as_absent_decodes_from_an_entry_without_its_field() {
    let mut queued = 0;
    for corpus in every_corpus() {
        for entry in json_entries(&corpus) {
            if let DomainEvent::UserMessage(message) = &entry.event {
                let written = serde_json::to_value(&entry.event).unwrap();
                let has_field = written["user_message"].get("delivery").is_some();
                assert_eq!(
                    has_field,
                    message.delivery != p::Delivery::Queue,
                    "{written}"
                );
                queued += usize::from(!has_field);
            }
        }
    }
    assert!(
        queued > 0,
        "the corpus holds no user_message without delivery"
    );
}

/// True when every key of `old`, at any depth, is in `new` with a value that holds it in turn.
/// `new` may have keys `old` lacks: a field added after v1.
fn holds(new: &Value, old: &Value) -> bool {
    match (new, old) {
        (Value::Object(new), Value::Object(old)) => old
            .iter()
            .all(|(key, value)| new.get(key).is_some_and(|kept| holds(kept, value))),
        (Value::Array(new), Value::Array(old)) => {
            new.len() == old.len() && new.iter().zip(old).all(|(kept, value)| holds(kept, value))
        }
        _ => new == old,
    }
}

/// A field renamed or removed since it was frozen that was optional still decodes, but drops what
/// the entry stored; writing the entry back shows it.
#[test]
fn every_stored_field_survives_a_round_trip() {
    for corpus in every_corpus() {
        for (i, (line, entry)) in json_lines(&corpus)
            .into_iter()
            .zip(json_entries(&corpus))
            .enumerate()
        {
            let old: Value = serde_json::from_slice(line).unwrap();
            let new = serde_json::to_value(&entry).unwrap();
            assert!(
                holds(&new, &old),
                "{} entry {} lost a field:\n{old}\n{new}",
                corpus.0,
                i + 1
            );
        }
    }
}

/// What is wrong with a corpus file's pin, if anything: a new `(0, 0)` pin for a file in
/// `corpus/added/` is to be set to the file's length and hash; a `(0, 0)` pin anywhere else is
/// refused, since a frozen file is pinned from the start; any other mismatch means the frozen
/// file changed.
fn pin_problem(path: &str, bytes: &[u8], frozen: (usize, u64)) -> Option<String> {
    let found = (bytes.len(), fnv1a(bytes));
    if found == frozen {
        None
    } else if frozen == (0, 0) && path.starts_with("added/") {
        Some(format!(
            "corpus/{path}: set this pin to ({}, {:#018x})",
            found.0, found.1
        ))
    } else if frozen == (0, 0) {
        Some(format!(
            "corpus/{path} is frozen, so its pin is its length and hash, never (0, 0)"
        ))
    } else {
        Some(format!(
            "corpus/{path} changed: it is frozen, never edit or regenerate it; a new schema adds \
             corpus/v2 beside v1, a new kind a file in corpus/added/"
        ))
    }
}

#[test]
fn the_corpus_files_are_unchanged() {
    for corpus in every_corpus() {
        let (name, json, packed, json_pin, packed_pin) = corpus;
        for (file, bytes, frozen) in [("jsonl", json, json_pin), ("msgpack", packed, packed_pin)] {
            let problem = pin_problem(&format!("{name}.{file}"), bytes, frozen);
            assert!(problem.is_none(), "{}", problem.unwrap_or_default());
        }
    }
}

#[test]
fn a_new_pin_says_what_to_set_it_to() {
    let set = pin_problem("added/x.jsonl", b"a", (0, 0)).unwrap();
    assert_eq!(
        set,
        "corpus/added/x.jsonl: set this pin to (1, 0xaf63dc4c8601ec8c)"
    );
    let changed = pin_problem("added/x.jsonl", b"a", (1, 7)).unwrap();
    assert!(changed.contains("changed: it is frozen"), "{changed}");
    assert_eq!(
        pin_problem("added/x.jsonl", b"a", (1, 0xaf63_dc4c_8601_ec8c)),
        None
    );
}

/// A `(0, 0)` pin is the friendly "set this pin" message only for a new file in `corpus/added/`;
/// a frozen file's pin is never `(0, 0)`.
#[test]
fn a_zero_pin_is_refused_outside_added() {
    for path in ["v1/events.jsonl", "v1/events.msgpack"] {
        assert_eq!(
            pin_problem(path, b"a", (0, 0)),
            Some(format!(
                "corpus/{path} is frozen, so its pin is its length and hash, never (0, 0)"
            ))
        );
    }
}

#[test]
fn fnv1a_matches_the_reference_values() {
    assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
}
