//! The TypeScript export: every type `wire_types()` registers exports with
//! specta-typescript's defaults (which refuse an unannotated 64-bit integer) and `WireFormat`,
//! and the output has the same shape as the JSON.
#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]
#![cfg(feature = "specta")]

use std::collections::BTreeSet;

use clauseo_protocol as p;
use p::WireFormat;
use specta::datatype::{DataType, Fields};
use specta::{Format, Types};
use specta_typescript::Typescript;

/// The text of `export type <name> = ...;` up to the blank line after it.
fn block<'a>(ts: &'a str, name: &str) -> &'a str {
    let start = ts.find(&format!("export type {name} = "));
    assert!(start.is_some(), "{name} is not exported");
    let rest = &ts[start.unwrap_or_default()..];
    &rest[..rest.find("\n\n").unwrap_or(rest.len())]
}

/// Lines that are TypeScript, not doc comments.
fn code_lines(ts: &str) -> impl Iterator<Item = &str> {
    ts.lines().filter(|line| {
        let line = line.trim_start();
        !(line.starts_with("/*") || line.starts_with('*') || line.starts_with("//"))
    })
}

/// Every object key in a line: the identifier before each `:` or `?:`.
fn keys(line: &str) -> Vec<&str> {
    let mut found = Vec::new();
    for (at, _) in line.match_indices(':') {
        let before = line[..at].strip_suffix('?').unwrap_or(&line[..at]);
        let start = before
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .map_or(0, |i| i + 1);
        if start < before.len() {
            found.push(&before[start..]);
        }
    }
    found
}

/// No empty or duplicate entry of this crate in the collection.
fn assert_collection(types: &Types) {
    let mut names = BTreeSet::new();
    for ndt in types.into_unsorted_iter() {
        // Inlined types (`String`, `BTreeMap`) keep an entry without a definition, by design.
        if ndt.module_path.starts_with("clauseo_protocol") {
            assert!(ndt.ty.is_some(), "{} has no definition", ndt.name);
        }
        let name = ndt.name.to_string();
        assert!(names.insert(name), "{} is collected twice", ndt.name);
    }
}

/// Panics, printing the block, when `name`'s block fails `check`.
fn assert_block(ts: &str, name: &str, check: impl Fn(&str) -> bool) {
    let text = block(ts, name);
    assert!(check(text), "{name} is not in the expected form:\n{text}");
}

/// Every type of this crate in the collection, which `wire_types()` fills, is exported once.
fn assert_exported(types: &Types, ts: &str) {
    let mut exported = BTreeSet::new();
    for line in ts.lines() {
        if let Some(rest) = line.strip_prefix("export type ") {
            let name = rest.split(' ').next().unwrap_or_default();
            assert!(exported.insert(name), "{name} is exported twice");
        }
    }
    let ours: BTreeSet<String> = types
        .into_unsorted_iter()
        .filter(|ndt| ndt.module_path.starts_with("clauseo_protocol"))
        .map(|ndt| ndt.name.to_string())
        .collect();
    for name in [
        "Envelope",
        "DomainEvent",
        "KnownDomainEvent",
        "Finite",
        "LimitsView",
        "ViewEntry",
    ] {
        assert!(ours.contains(name), "wire_types() lacks {name}");
    }
    for name in &ours {
        assert!(exported.contains(name.as_str()), "{name} is not exported");
    }
}

/// External tagging: unions of one-key objects; `DomainEvent` has one per known kind and an
/// unknown arm, any key holding a `RawPayload`, so the export says the set is open. It has no
/// `unknown` key. `KnownDomainEvent` is the same union without the unknown arm.
fn assert_tagging(ts: &str) {
    let event = block(ts, "DomainEvent");
    for kind in p::DomainEvent::KINDS {
        let key = format!("({{ {kind}: ");
        assert!(event.contains(&key), "DomainEvent lacks {kind}");
    }
    for arm in [
        "({ turn_started: TurnStarted })",
        "({ thread_archived: ThreadArchived })",
    ] {
        assert!(event.contains(arm), "DomainEvent lacks {arm}: {event}");
    }
    assert!(
        event.ends_with("{ [key in string]: RawPayload };"),
        "DomainEvent has no unknown arm: {event}"
    );
    assert!(
        !event.contains("unknown"),
        "DomainEvent has an `unknown` key: {event}"
    );
    let known = block(ts, "KnownDomainEvent");
    let arms = known.strip_prefix("export type KnownDomainEvent = ");
    let arms = arms
        .and_then(|arms| arms.strip_suffix(';'))
        .unwrap_or_default();
    let open = event.strip_prefix("export type DomainEvent = ");
    assert!(
        !arms.is_empty() && open.is_some_and(|open| open.starts_with(&format!("{arms} | "))),
        "KnownDomainEvent is not DomainEvent's known arms:\n{known}"
    );
    assert!(
        !known.contains("RawPayload"),
        "KnownDomainEvent has an unknown arm: {known}"
    );
    assert_block(ts, "ThreadArchived", |b| {
        b.ends_with("Record<string, never>;")
    });
    assert_block(ts, "ViewEntry", |b| {
        b.contains("({ tool_call: ToolCallEntry })")
    });
    let stop = block(ts, "StopReason");
    let both = stop.contains("\"end_turn\"") && stop.contains("\"unknown\"");
    assert!(both, "StopReason lacks end_turn or unknown: {stop}");
    assert_block(ts, "ErrorCode", |b| b.contains("\"thread.queue_full\""));
}

/// `snake_case` keys and variant names, never the Rust spelling.
fn assert_snake_case(ts: &str) {
    for line in code_lines(ts) {
        for key in keys(line) {
            let snake = !key.chars().any(|c| c.is_ascii_uppercase());
            assert!(snake, "key `{key}` is not snake_case in: {line}");
        }
        let mut literals = line.split('"').skip(1).step_by(2);
        let pascal = literals.any(|s| s.starts_with(|c: char| c.is_ascii_uppercase()));
        assert!(!pascal, "a variant keeps its Rust name in: {line}");
    }
}

/// Left-out optional fields are `field?: T`. The only `null`s are JSON's own, in a JSON value;
/// floats are `Finite`, never `null`.
fn assert_optional(ts: &str) {
    assert_block(ts, "Envelope", |b| {
        b.contains("\tcommand_id?: CommandId,\n")
    });
    assert_block(ts, "AvailableCommand", |b| {
        b.contains("\tinput_hint?: string,\n")
    });
    assert_block(ts, "AgentCapabilities", |b| {
        b.contains("\tsteering?: SteeringCapability,\n")
    });
    for line in code_lines(ts).filter(|line| line.contains("null")) {
        let allowed = line.starts_with("export type JsonValue = null | ")
            || line.starts_with("export type RawPayload = null | ");
        assert!(allowed, "unexpected `null` in: {line}");
    }
}

/// Hand-serialised ids and short strings are `string`; 64-bit integers are `number`.
fn assert_scalars(ts: &str) {
    for name in [
        "StreamId",
        "EntryId",
        "BlobHash",
        "EventId",
        "MatterId",
        "ThreadId",
        "TurnId",
        "MessageId",
        "ToolCallId",
        "PermissionRequestId",
        "ElicitationId",
        "CheckpointId",
        "CommandId",
        "FileRefId",
        "FolderRefId",
        "ViewEpoch",
        "AgentId",
        "AgentSessionId",
        "ContentId",
        "MatterPath",
        "AgentChoiceId",
        "Title",
    ] {
        assert_eq!(block(ts, name), format!("export type {name} = string;"));
    }
    for name in ["Seq", "Cursor", "Count", "Timestamp", "Finite"] {
        assert_eq!(block(ts, name), format!("export type {name} = number;"));
    }
    assert_block(ts, "BlobRef", |b| b.contains("\tsize_bytes: Count,\n"));
    assert_block(ts, "UsageUpdated", |b| {
        b.contains("\tcontext_used?: Count,\n")
    });
    assert_block(ts, "Cost", |b| b.contains("\tamount: Finite,\n"));
    assert_block(ts, "LimitWindowView", |b| {
        b.contains("\tused_percent?: Finite,\n")
    });
    assert_block(ts, "ConfigOption", |b| b.contains("\tid: string,\n"));
    assert!(!ts.contains("bigint") && !ts.contains("BigInt"));
    assert!(!ts.contains("CompactString"));
}

/// Exports `types` as the app does.
fn export(types: &Types) -> Result<String, String> {
    Typescript::default()
        .export(types, WireFormat)
        .map_err(|err| err.to_string())
}

#[test]
fn exports_every_wire_type() {
    let types = p::wire_types();
    assert_collection(&types);

    // Default settings: a `u64`, `i64` or `usize` without `specta(type = ...)` fails here.
    let ts = export(&types).unwrap_or_else(|err| panic!("export failed: {err}"));

    assert_exported(&types, &ts);
    assert_tagging(&ts);
    assert_snake_case(&ts);
    assert_optional(&ts);
    assert_scalars(&ts);
}

/// `known_kinds_typescript()` lists `DomainEvent::KINDS` in order, and its names are the keys of
/// the exported `KnownDomainEvent` union, the set the webview checks an event's kind against.
#[test]
fn known_kinds_line_lists_the_kinds_the_union_keys_are() {
    let types = p::wire_types();
    let line = p::known_kinds_typescript();
    let ts = export(&types).unwrap_or_else(|err| panic!("export failed: {err}")) + &line;
    assert!(ts.ends_with(&line), "the line is not appended last");
    assert_eq!(line.matches('\n').count(), 1, "not one line: {line}");

    let list = line
        .strip_prefix("export const KNOWN_DOMAIN_EVENT_KINDS = [")
        .and_then(|rest| rest.strip_suffix("] as const;\n"))
        .unwrap_or_else(|| panic!("not the known kinds line: {line}"));
    let listed: Vec<&str> = list
        .split(", ")
        .map(|kind| kind.trim_matches('"'))
        .collect();
    assert_eq!(listed, p::DomainEvent::KINDS, "{line}");

    let listed_set: BTreeSet<&str> = listed.iter().copied().collect();
    assert_eq!(
        listed_set.len(),
        listed.len(),
        "a kind is listed twice: {line}"
    );
    let union: BTreeSet<&str> = code_lines(block(&ts, "KnownDomainEvent"))
        .flat_map(keys)
        .collect();
    assert_eq!(union, listed_set, "the union's keys differ from the line");
}

/// A field left out when `None` and filled in on decode.
#[derive(serde::Serialize, specta::Type)]
struct Defaulted {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<u32>,
}

/// A field left out when `None` but required on decode.
#[derive(serde::Serialize, specta::Type)]
struct Required {
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<u32>,
}

/// The definition `T` registers, after `WireFormat::map_type`.
fn mapped<T: specta::Type>() -> Result<DataType, String> {
    let types = Types::default().register::<T>();
    let ndt = types.into_unsorted_iter().next();
    let dt = ndt.and_then(|ndt| ndt.ty.clone());
    let dt = dt.ok_or("no definition registered")?;
    let out = WireFormat.map_type(&types, &dt);
    out.map(std::borrow::Cow::into_owned)
        .map_err(|err| err.to_string())
}

#[test]
fn a_skipped_field_is_optional_and_not_null_and_needs_default() {
    let ts = export(&Types::default().register::<Defaulted>());
    let ts = ts.unwrap_or_else(|err| panic!("export failed: {err}"));
    assert_block(&ts, "Defaulted", |b| {
        b.contains("\tnote?: number,\n") && !b.contains("null")
    });
    let refused = export(&Types::default().register::<Required>());
    let refused = refused.err().unwrap_or_default();
    assert!(
        refused.contains("`skip_serializing_if` without `default`"),
        "{refused}"
    );

    let dt = mapped::<Defaulted>().unwrap_or_else(|err| panic!("map_type failed: {err}"));
    let DataType::Struct(s) = dt else {
        panic!("not a struct: {dt:?}");
    };
    let Fields::Named(fields) = &s.fields else {
        panic!("no named fields: {s:?}");
    };
    for (name, field) in &fields.fields {
        let plain = matches!(field.ty, Some(DataType::Primitive(_)));
        assert!(
            field.optional && plain,
            "{name} is not `?: number`: {field:?}"
        );
    }
    let refused = mapped::<Required>().err().unwrap_or_default();
    assert!(refused.contains("without `default`"), "{refused}");
}
