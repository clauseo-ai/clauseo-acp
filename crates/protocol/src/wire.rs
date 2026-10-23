//! The TypeScript wire rules, under the `specta` feature: [`WireFormat`], the format an app
//! exports [`crate::wire_types`] with.

use std::borrow::Cow;

use specta::datatype::{DataType, Field, Fields};
use specta::{Format, FormatError, Types};

use crate::DomainEvent;

const SKIP_IF: &str = "serde:field:skip_serializing_if";
const DEFAULT: &str = "serde:field:default";
const OTHER: &str = "serde:variant:other";

/// The format that exports this crate's types as their JSON form: `specta_serde::Format`, which
/// applies the serde attributes, after two rewrites. Its unified mode rejects two attributes this
/// crate uses, so `WireFormat` first rewrites them to what the encoder writes:
///
/// - `skip_serializing_if` (always with `default`): the field is left out instead of being
///   `null`, so the rewrite drops the `null` and it exports as `field?: T` (`default` makes it
///   optional), not `field?: T | null`. A field with `skip_serializing_if` and no `default` is
///   an export error, since it is required on decode.
/// - `#[serde(other)] Unknown`: the variant stays, written back as the string `"unknown"`.
///
/// An app exports with it, starting from [`crate::wire_types`], and appends
/// [`known_kinds_typescript`] to the output:
/// `Typescript::default().export(&wire_types(), WireFormat)? + &known_kinds_typescript()`.
#[derive(Debug, Clone, Copy, Default)]
pub struct WireFormat;

/// The known event kinds as one TypeScript line, `export const KNOWN_DOMAIN_EVENT_KINDS = [...]
/// as const;` followed by a newline, listing [`DomainEvent::KINDS`] in order. The webview checks
/// an event's kind against it, so it does not keep a second list of kinds by hand.
#[must_use]
pub fn known_kinds_typescript() -> String {
    let kinds: Vec<String> = DomainEvent::KINDS
        .iter()
        .map(|kind| format!("\"{kind}\""))
        .collect();
    format!(
        "export const KNOWN_DOMAIN_EVENT_KINDS = [{}] as const;\n",
        kinds.join(", ")
    )
}

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
        // `specta_serde` exports a `default` field as `field?:`; without `default` the field is
        // required on decode, and `field?:` would be wrong.
        if !field.attributes.contains_key(DEFAULT) {
            return Err("`skip_serializing_if` without `default`".into());
        }
        if let Some(DataType::Nullable(inner)) = field.ty.take() {
            field.ty = Some(*inner);
        }
    }
    field.ty.as_mut().map_or(Ok(()), rewrite)
}
