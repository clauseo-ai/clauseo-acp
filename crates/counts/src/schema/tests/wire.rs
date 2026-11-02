//! Closed on the wire (research 37 §7.2): the committed JSON Schema of both payloads, a walk that
//! fails on a string with no fixed set of values or an integer with no cap, and a property test
//! over arbitrary days.

use std::borrow::Cow;
use std::collections::BTreeSet;

use proptest::prelude::*;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde_json::{Map, Value, json};

use super::sample::{Sample, Tape};
use crate::schema::{
    Arch, Batch, Cell, Channel, Count, DAY_PATTERN, Day, DayCounter, DayReport, Event, InstallId,
    Language, Platform, SchemaVersion, TallyCounter, TallyId, TallyReport, UUID_V4_PATTERN,
    VERSION_PATTERN, Version, WEEK_PATTERN, Week,
};

impl<K: JsonSchema> JsonSchema for Cell<K> {
    fn schema_name() -> Cow<'static, str> {
        format!("Cell_{}", K::schema_name()).into()
    }

    /// `K`'s object (each variant's, for an enum) with `n` beside its fields.
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        fn add_n(schema: &mut Value, n: &Value) {
            let Some(object) = schema.as_object_mut() else {
                return;
            };
            for key in ["oneOf", "anyOf"] {
                if let Some(Value::Array(variants)) = object.get_mut(key) {
                    for variant in variants.iter_mut() {
                        add_n(variant, n);
                    }
                    return;
                }
            }
            let properties = object.entry("properties").or_insert_with(|| json!({}));
            properties
                .as_object_mut()
                .unwrap()
                .insert("n".to_owned(), n.clone());
            let required = object.entry("required").or_insert_with(|| json!([]));
            required.as_array_mut().unwrap().push(json!("n"));
        }
        let n = generator.subschema_for::<Count>().to_value();
        let mut schema = K::json_schema(generator).to_value();
        add_n(&mut schema, &n);
        Schema::try_from(schema).unwrap()
    }
}

fn counts_schema() -> Value {
    schemars::schema_for!(Batch).to_value()
}

fn tally_schema() -> Value {
    schemars::schema_for!(TallyReport).to_value()
}

#[test]
fn the_committed_schema_matches() {
    insta::with_settings!({
        snapshot_path => "../snapshots",
        prepend_module_to_snapshot => false,
        sort_maps => true,
    }, {
        insta::assert_json_snapshot!("counts_schema", counts_schema());
        insta::assert_json_snapshot!("tally_schema", tally_schema());
    });
}

/// Every place in `schema` where a string has no fixed values (other than a UUID v4, a day, a
/// week or a version), an integer has no cap, an object takes other fields, or a float appears.
fn open_places(schema: &Value, at: &str, found: &mut Vec<String>) {
    match schema {
        Value::Object(object) => {
            let types: Vec<&str> = match object.get("type") {
                Some(Value::String(one)) => vec![one.as_str()],
                Some(Value::Array(many)) => many.iter().filter_map(Value::as_str).collect(),
                _ => Vec::new(),
            };
            let pattern = object.get("pattern").and_then(Value::as_str);
            let fixed = object.contains_key("enum") || object.contains_key("const");
            let uuid_v4 =
                object.get("format") == Some(&json!("uuid")) && pattern == Some(UUID_V4_PATTERN);
            let formed = [DAY_PATTERN, WEEK_PATTERN, VERSION_PATTERN]
                .iter()
                .any(|known| pattern == Some(*known));
            if types.contains(&"string") && !(fixed || uuid_v4 || formed) {
                found.push(format!("{at}: a string with no enum"));
            }
            if types.contains(&"integer") && !object.contains_key("maximum") {
                found.push(format!("{at}: an integer with no maximum"));
            }
            if types.contains(&"number") {
                found.push(format!("{at}: a float"));
            }
            if types.contains(&"object")
                && object.get("additionalProperties") != Some(&json!(false))
            {
                found.push(format!("{at}: an object that takes other fields"));
            }
            for (key, value) in object {
                open_places(value, &format!("{at}/{key}"), found);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                open_places(item, &format!("{at}/{index}"), found);
            }
        }
        _ => {}
    }
}

#[test]
fn every_string_has_an_enum_and_every_integer_a_maximum() {
    for schema in [counts_schema(), tally_schema()] {
        let mut found = Vec::new();
        open_places(&schema, "", &mut found);
        assert_eq!(found, Vec::<String>::new());
    }
}

#[test]
fn the_walk_finds_an_open_string_and_an_uncapped_integer() {
    let planted = json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "name": { "type": "string" },
            "size": { "type": "integer", "minimum": 0 },
            "id": { "type": "string", "format": "uuid" },
        },
    });
    let mut found = Vec::new();
    open_places(&planted, "", &mut found);
    assert_eq!(found.len(), 3, "{found:?}");
}

fn properties<'a>(schema: &'a Value, def: &str) -> BTreeSet<&'a str> {
    let object = if def.is_empty() {
        schema
    } else {
        &schema["$defs"][def]
    };
    object["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("{def} has properties"))
        .keys()
        .map(String::as_str)
        .collect()
}

#[test]
fn the_day_report_has_no_fleet_or_crash_field_and_the_tally_no_install() {
    let counts = counts_schema();
    let day = properties(&counts, "DayReport");
    for field in ["install_type", "managed", "crash", "tally", "week"] {
        assert!(!day.contains(field), "the day report has {field}");
    }
    let day_cells = properties(&counts, "DayCells");
    assert!(!day_cells.contains("crash") && !day_cells.contains("active_week"));
    assert_eq!(day_cells.len(), 17);

    let tally = tally_schema();
    let fields = properties(&tally, "");
    for field in ["install", "language", "day"] {
        assert!(!fields.contains(field), "the tally has {field}");
    }
    assert_eq!(
        properties(&tally, "TallyCells"),
        BTreeSet::from(["crash", "active_week"])
    );
}

/// Every string an `enum` or `const` in `schema` lists.
fn enum_names(schema: &Value, names: &mut BTreeSet<String>) {
    match schema {
        Value::Object(object) => {
            if let Some(Value::String(name)) = object.get("const") {
                names.insert(name.clone());
            }
            if let Some(Value::Array(values)) = object.get("enum") {
                names.extend(values.iter().filter_map(Value::as_str).map(str::to_owned));
            }
            object.values().for_each(|value| enum_names(value, names));
        }
        Value::Array(items) => items.iter().for_each(|item| enum_names(item, names)),
        _ => {}
    }
}

/// The strings and numbers of a payload that are neither a known name nor a closed form.
fn open_values(value: &Value, names: &BTreeSet<String>, at: &str, found: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            let closed = names.contains(text)
                || text.parse::<InstallId>().is_ok()
                || text.parse::<Day>().is_ok()
                || text.parse::<Week>().is_ok()
                || text.parse::<Version>().is_ok();
            if !closed {
                found.push(format!("{at}: {text:?}"));
            }
        }
        Value::Number(number) => {
            let capped = number.as_u64().is_some_and(|n| n <= u64::from(Count::MAX));
            let counted = !at.ends_with("/n") || number.as_u64().is_some_and(|n| n >= 1);
            if !(capped && counted) {
                found.push(format!("{at}: {number}"));
            }
        }
        Value::Object(object) => object_values(object, names, at, found),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                open_values(item, names, &format!("{at}/{index}"), found);
            }
        }
        Value::Bool(_) | Value::Null => {}
    }
}

fn object_values(
    object: &Map<String, Value>,
    names: &BTreeSet<String>,
    at: &str,
    found: &mut Vec<String>,
) {
    for (key, value) in object {
        open_values(value, names, &format!("{at}/{key}"), found);
    }
}

/// A day report of arbitrary cells, `age` days before 2026-11-02.
fn arbitrary_day(tape: &mut Tape, age: i64) -> DayReport {
    let mut counter = DayCounter::default();
    for _ in 0..tape.below(400) {
        counter.bump(Event::sample(tape));
    }
    counter.set_time_open(tape.below(30 * 3600));
    let platform = Platform::sample(tape);
    DayReport {
        schema: SchemaVersion::V1,
        install: InstallId::sample(tape),
        day: Day::from_ymd(2026, 11, 2)
            .unwrap()
            .days_before(age)
            .unwrap(),
        version: Version::sample(tape),
        channel: Channel::sample(tape),
        os: platform.os(),
        os_major: platform.os_major(),
        arch: Arch::sample(tape),
        language: Language::sample(tape),
        cells: counter.to_cells(),
    }
}

proptest! {
    #[test]
    fn arbitrary_days_carry_only_known_names_ids_and_capped_numbers(
        values in proptest::collection::vec(any::<u64>(), 0..400),
    ) {
        let mut tape = Tape::new(values);
        let days = (1..=1 + tape.below(7))
            .map(|age| arbitrary_day(&mut tape, i64::try_from(age).unwrap()))
            .collect();
        let batch = Batch { days };
        let json = serde_json::to_value(&batch).unwrap();

        let mut names = BTreeSet::new();
        enum_names(&counts_schema(), &mut names);
        let mut found = Vec::new();
        open_values(&json, &names, "", &mut found);
        prop_assert_eq!(found, Vec::<String>::new());

        let back: Batch = serde_json::from_value(json).unwrap();
        prop_assert_eq!(&back, &batch);
        prop_assert_eq!(batch.check(Day::from_ymd(2026, 11, 2).unwrap()), Ok(()));
    }

    #[test]
    fn arbitrary_tallies_carry_only_known_names_ids_and_capped_numbers(
        values in proptest::collection::vec(any::<u64>(), 0..100),
    ) {
        let mut tape = Tape::new(values);
        let mut counter = TallyCounter::default();
        for _ in 0..tape.below(50) {
            counter.crash(Sample::sample(&mut tape));
        }
        counter.mark_active();
        let platform = Platform::sample(&mut tape);
        let this_week = Day::from_ymd(2026, 11, 2).unwrap().week();
        let tally = TallyReport {
            schema: SchemaVersion::V1,
            tally: TallyId::sample(&mut tape),
            week: this_week.weeks_before(1 + i64::try_from(tape.below(4)).unwrap()).unwrap(),
            version: Version::sample(&mut tape),
            channel: Channel::sample(&mut tape),
            os: platform.os(),
            os_major: platform.os_major(),
            arch: Arch::sample(&mut tape),
            install_type: Sample::sample(&mut tape),
            managed: Sample::sample(&mut tape),
            cells: counter.to_cells(),
        };
        let json = serde_json::to_value(&tally).unwrap();
        let mut names = BTreeSet::new();
        enum_names(&tally_schema(), &mut names);
        let mut found = Vec::new();
        open_values(&json, &names, "", &mut found);
        prop_assert_eq!(found, Vec::<String>::new());
        let back: TallyReport = serde_json::from_value(json).unwrap();
        prop_assert_eq!(&back, &tally);
        prop_assert_eq!(tally.check(this_week), Ok(()));
    }
}
