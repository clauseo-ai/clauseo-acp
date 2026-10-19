//! The TypeScript export: every wire type exports with specta-typescript's defaults (which refuse
//! an unannotated 64-bit integer), and the output has the same shape as the JSON.
//!
//! `WireFormat` applies the serde attributes through `specta_serde::Format`. Its unified mode
//! rejects two attributes this crate uses, so `WireFormat` first rewrites them to what the
//! encoder writes:
//! - `skip_serializing_if` (always with `default`): the field is left out instead of being
//!   `null`, so it exports as `field?: T`, not `field?: T | null`;
//! - `#[serde(other)] Unknown`: the variant stays, written back as the string `"unknown"`.
//!
//! An app that exports these types must use the same rewrite.
#![cfg(feature = "specta")]

use std::borrow::Cow;
use std::collections::BTreeSet;

use clauseo_protocol as p;
use specta::datatype::{DataType, Field, Fields};
use specta::{Format, FormatError, Types};
use specta_typescript::Typescript;

const SKIP_IF: &str = "serde:field:skip_serializing_if";
const DEFAULT: &str = "serde:field:default";
const OTHER: &str = "serde:variant:other";

/// `specta_serde::Format` after the two rewrites in the module docs.
struct WireFormat;

impl Format for WireFormat {
    fn map_types(&self, types: &Types) -> Result<Cow<'_, Types>, FormatError> {
        let mut types = types.clone();
        let mut failed = None;
        types.iter_mut(|ndt| {
            if let Err(err) = ndt.ty.as_mut().map_or(Ok(()), rewrite) {
                failed.get_or_insert(format!("{}: {err}", ndt.name));
            }
        });
        if let Some(err) = failed {
            return Err(err.into());
        }
        Ok(Cow::Owned(
            specta_serde::Format.map_types(&types)?.into_owned(),
        ))
    }

    fn map_type(&self, types: &Types, dt: &DataType) -> Result<Cow<'_, DataType>, FormatError> {
        let mut dt = dt.clone();
        rewrite(&mut dt)?;
        Ok(Cow::Owned(
            specta_serde::Format.map_type(types, &dt)?.into_owned(),
        ))
    }
}

fn rewrite(dt: &mut DataType) -> Result<(), FormatError> {
    match dt {
        DataType::Struct(s) => rewrite_fields(&mut s.fields),
        DataType::Enum(e) => e.variants.iter_mut().try_for_each(|(_, variant)| {
            variant.attributes.remove(OTHER);
            rewrite_fields(&mut variant.fields)
        }),
        DataType::List(list) => rewrite(&mut list.ty),
        DataType::Map(map) => {
            rewrite(map.key_ty_mut())?;
            rewrite(map.value_ty_mut())
        }
        DataType::Tuple(tuple) => tuple.elements.iter_mut().try_for_each(rewrite),
        DataType::Nullable(inner) => rewrite(inner),
        DataType::Intersection(parts) => parts.iter_mut().try_for_each(rewrite),
        DataType::Primitive(_) | DataType::Generic(_) | DataType::Reference(_) => Ok(()),
    }
}

fn rewrite_fields(fields: &mut Fields) -> Result<(), FormatError> {
    match fields {
        Fields::Unit => Ok(()),
        Fields::Unnamed(f) => f.fields.iter_mut().try_for_each(rewrite_field),
        Fields::Named(f) => f.fields.iter_mut().try_for_each(|(_, f)| rewrite_field(f)),
    }
}

fn rewrite_field(field: &mut Field) -> Result<(), FormatError> {
    if field.attributes.remove(SKIP_IF) {
        // Without `default` the field is required on decode, and `field?:` would be wrong.
        if !field.attributes.contains_key(DEFAULT) {
            return Err("`skip_serializing_if` without `default`".into());
        }
        field.optional = true;
        if let Some(DataType::Nullable(inner)) = field.ty.take() {
            field.ty = Some(*inner);
        }
    }
    field.ty.as_mut().map_or(Ok(()), rewrite)
}

/// The roots; everything they reach is collected with them.
fn wire_types() -> Types {
    Types::default()
        .register::<p::Envelope>()
        .register::<p::DomainEvent>()
        .register::<p::CommandEnvelope>()
        .register::<p::HostCommand>()
        .register::<p::CommandOutcome>()
        .register::<p::HostQuery>()
        .register::<p::QueryResult>()
        .register::<p::ViewPatch>()
        .register::<p::PatchBatch>()
        .register::<p::HostEvent>()
        .register::<p::Subscribe>()
        .register::<p::SubscribeEvents>()
        .register::<p::Ack>()
        .register::<p::HostError>()
        .register::<p::ErrorCode>()
        .register::<p::AccountLimits>()
        .register::<p::LimitsView>()
}

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

/// Each type is exported once, and every root is there.
fn assert_exported(ts: &str) {
    let mut exported = BTreeSet::new();
    for line in ts.lines() {
        if let Some(rest) = line.strip_prefix("export type ") {
            let name = rest.split(' ').next().unwrap_or_default();
            assert!(exported.insert(name), "{name} is exported twice");
        }
    }
    for root in [
        "Envelope",
        "DomainEvent",
        "CommandEnvelope",
        "HostCommand",
        "CommandOutcome",
        "HostQuery",
        "QueryResult",
        "ViewPatch",
        "PatchBatch",
        "HostEvent",
        "Subscribe",
        "SubscribeEvents",
        "Ack",
        "HostError",
        "ErrorCode",
        "AccountLimits",
        "LimitsView",
    ] {
        assert!(exported.contains(root), "{root} is not exported");
    }
}

/// External tagging: unions of one-key objects; `DomainEvent` has one per known kind and no
/// `unknown` key.
fn assert_tagging(ts: &str) {
    let event = block(ts, "DomainEvent");
    for kind in p::DomainEvent::KINDS {
        let key = format!("({{ {kind}: ");
        assert!(event.contains(&key), "DomainEvent lacks {kind}");
    }
    assert!(event.contains("({ turn_started: TurnStarted })"));
    assert!(event.contains("({ thread_archived: ThreadArchived })"));
    assert!(
        !event.contains("unknown"),
        "DomainEvent has an `unknown` key"
    );
    assert!(block(ts, "ThreadArchived").ends_with("Record<string, never>;"));
    assert!(block(ts, "ViewEntry").contains("({ tool_call: ToolCallEntry })"));
    let stop = block(ts, "StopReason");
    assert!(stop.contains("\"end_turn\"") && stop.contains("\"unknown\""));
    assert!(block(ts, "ErrorCode").contains("\"thread.queue_full\""));
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

/// Left-out optional fields are `field?: T`. The only `null`s: JSON's own, in a JSON value, and
/// `f64`, which `serde_json` writes as `null` when it is not finite.
fn assert_optional(ts: &str) {
    assert!(block(ts, "Envelope").contains("\tcommand_id?: CommandId,\n"));
    assert!(block(ts, "AvailableCommand").contains("\tinput_hint?: string,\n"));
    assert!(block(ts, "AgentCapabilities").contains("\tsteering?: SteeringCapability,\n"));
    for line in code_lines(ts).filter(|line| line.contains("null")) {
        let allowed = line.starts_with("export type JsonValue = null | ")
            || line.starts_with("export type RawPayload = null | ")
            || line == "\tamount: number | null,"
            || line.trim_start().starts_with("used_percent");
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
    ] {
        assert_eq!(block(ts, name), format!("export type {name} = string;"));
    }
    for name in ["Seq", "Cursor", "Timestamp"] {
        assert_eq!(block(ts, name), format!("export type {name} = number;"));
    }
    assert!(block(ts, "BlobRef").contains("\tsize_bytes: number,\n"));
    assert!(block(ts, "UsageUpdated").contains("\tcontext_used?: number,\n"));
    assert!(block(ts, "ConfigOption").contains("\tid: string,\n"));
    assert!(!ts.contains("bigint") && !ts.contains("BigInt"));
    assert!(!ts.contains("CompactString"));
}

#[test]
fn exports_every_wire_type() {
    let types = wire_types();
    assert_collection(&types);

    // Default settings: a `u64`, `i64` or `usize` without `specta(type = ...)` fails here.
    let ts = Typescript::default()
        .export(&types, WireFormat)
        .unwrap_or_else(|err| panic!("export failed: {err}"));

    assert_exported(&ts);
    assert_tagging(&ts);
    assert_snake_case(&ts);
    assert_optional(&ts);
    assert_scalars(&ts);
}
