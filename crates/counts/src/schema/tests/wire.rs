//! Closed on the wire (research 37 §7.2): the committed JSON Schema of both payloads, a walk that
//! fails on a string with no fixed set of values or an integer with no cap, and a property test
//! over arbitrary days.

use std::borrow::Cow;
use std::collections::BTreeSet;

use proptest::prelude::*;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde_json::{Value, json};

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

/// `node` with its `$ref` followed into `root`'s `$defs`.
fn resolve<'a>(root: &'a Value, node: &'a Value) -> &'a Value {
    match node.get("$ref").and_then(Value::as_str) {
        Some(reference) => {
            let name = reference.trim_start_matches("#/$defs/");
            resolve(root, &root["$defs"][name])
        }
        None => node,
    }
}

/// Where `value` breaks the schema node `node` it travels under: a name its own field does not
/// list, a number past its own field's `minimum` or `maximum`, a string of no fixed form, a
/// field the node does not have or a required one missing. Each value is checked against its
/// own node, so a change to one field's cap (`tries`, a text size, `schema`, `os_major`) is
/// caught, not only the 9,999 of a count.
fn breaks(root: &Value, node: &Value, value: &Value, at: &str, found: &mut Vec<String>) {
    let node = resolve(root, node);
    for key in ["oneOf", "anyOf"] {
        if let Some(Value::Array(variants)) = node.get(key) {
            let fits = variants.iter().any(|variant| {
                let mut inner = Vec::new();
                breaks(root, variant, value, at, &mut inner);
                inner.is_empty()
            });
            if !fits {
                found.push(format!("{at}: {value} fits no variant"));
            }
            return;
        }
    }
    if let Some(constant) = node.get("const") {
        if constant != value {
            found.push(format!("{at}: {value} is not {constant}"));
        }
        return;
    }
    if let Some(Value::Array(names)) = node.get("enum") {
        if !names.contains(value) {
            found.push(format!("{at}: {value} is not one of its names"));
        }
        return;
    }
    let types: Vec<&str> = match node.get("type") {
        Some(Value::String(one)) => vec![one.as_str()],
        Some(Value::Array(many)) => many.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    match value {
        Value::Null if types.contains(&"null") => {}
        Value::Bool(_) if types.contains(&"boolean") => {}
        Value::Number(number) if types.contains(&"integer") => {
            let n = number.as_u64();
            let min = node.get("minimum").and_then(Value::as_u64).unwrap_or(0);
            match (n, node.get("maximum").and_then(Value::as_u64)) {
                (Some(n), Some(max)) if (min..=max).contains(&n) => {}
                (_, None) => found.push(format!("{at}: {number} under no maximum")),
                _ => found.push(format!(
                    "{at}: {number} outside {min}..={:?}",
                    node["maximum"]
                )),
            }
        }
        Value::String(text) if types.contains(&"string") => {
            let closed = node.get("pattern").is_some()
                && (text.parse::<InstallId>().is_ok()
                    || text.parse::<Day>().is_ok()
                    || text.parse::<Week>().is_ok()
                    || text.parse::<Version>().is_ok());
            if !closed {
                found.push(format!("{at}: {text:?} has no fixed form"));
            }
        }
        Value::Array(items) if types.contains(&"array") => {
            for (index, item) in items.iter().enumerate() {
                breaks(root, &node["items"], item, &format!("{at}/{index}"), found);
            }
        }
        Value::Object(object) if types.contains(&"object") => {
            let properties = node["properties"].as_object();
            for (key, field) in object {
                match properties.and_then(|properties| properties.get(key)) {
                    Some(field_node) => {
                        breaks(root, field_node, field, &format!("{at}/{key}"), found);
                    }
                    None => found.push(format!("{at}: no field {key}")),
                }
            }
            for required in node["required"].as_array().into_iter().flatten() {
                if let Some(key) = required.as_str()
                    && !object.contains_key(key)
                {
                    found.push(format!("{at}: {key} missing"));
                }
            }
        }
        _ => found.push(format!("{at}: {value} is not {types:?}")),
    }
}

/// `breaks` from the schema's root.
fn breaks_schema(schema: &Value, value: &Value) -> Vec<String> {
    let mut found = Vec::new();
    breaks(schema, schema, value, "", &mut found);
    found
}

/// Sets every field named `key` in `value` to `to`; returns how many it set.
fn set_all(value: &mut Value, key: &str, to: &Value) -> usize {
    match value {
        Value::Object(object) => {
            let mut set = 0;
            for (name, field) in object.iter_mut() {
                if name == key {
                    *field = to.clone();
                    set += 1;
                } else {
                    set += set_all(field, key, to);
                }
            }
            set
        }
        Value::Array(items) => items.iter_mut().map(|item| set_all(item, key, to)).sum(),
        _ => 0,
    }
}

#[test]
fn each_number_is_checked_against_its_own_fields_cap() {
    let mut tape = Tape::new((0..400).collect());
    let mut day = arbitrary_day(&mut tape, 1);
    day.cells.agent_install.clear();
    let json = serde_json::to_value(Batch { days: vec![day] }).unwrap();
    let schema = counts_schema();
    assert_eq!(breaks_schema(&schema, &json), Vec::<String>::new());
    // Each one under 9,999, so only its own cap can catch it.
    for (key, past) in [("schema", 2), ("os_major", 27), ("n", 10_000), ("n", 0)] {
        let mut broken = json.clone();
        assert!(set_all(&mut broken, key, &json!(past)) > 0, "{key}");
        assert_ne!(
            breaks_schema(&schema, &broken),
            Vec::<String>::new(),
            "{key} = {past}"
        );
    }
    let mut install = json.clone();
    let cell = json!({ "agent": "claude", "result": "done", "tries": 4, "took": "lt_1s", "n": 1 });
    install["days"][0]["cells"]["agent_install"] = json!([cell]);
    let found = breaks_schema(&schema, &install);
    assert_ne!(found, Vec::<String>::new());
    let mut ok = install.clone();
    ok["days"][0]["cells"]["agent_install"][0]["tries"] = json!(3);
    assert_eq!(breaks_schema(&schema, &ok), Vec::<String>::new());
    for (key, text) in [("language", "Mehra divorce"), ("code", "store.lockd")] {
        let mut free = json.clone();
        if key == "code" {
            free["days"][0]["cells"]["error_shown"] = json!([{ "code": "internal", "n": 1 }]);
            assert_eq!(breaks_schema(&schema, &free), Vec::<String>::new());
        }
        assert!(set_all(&mut free, key, &json!(text)) > 0, "{key}");
        assert_ne!(breaks_schema(&schema, &free), Vec::<String>::new(), "{key}");
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
    fn arbitrary_days_fit_their_schema_field_by_field(
        values in proptest::collection::vec(any::<u64>(), 0..400),
    ) {
        let mut tape = Tape::new(values);
        let days = (1..=1 + tape.below(7))
            .map(|age| arbitrary_day(&mut tape, i64::try_from(age).unwrap()))
            .collect();
        let batch = Batch { days };
        let json = serde_json::to_value(&batch).unwrap();

        prop_assert_eq!(breaks_schema(&counts_schema(), &json), Vec::<String>::new());

        let back: Batch = serde_json::from_value(json).unwrap();
        prop_assert_eq!(&back, &batch);
        prop_assert_eq!(batch.check(Day::from_ymd(2026, 11, 2).unwrap()), Ok(()));
    }

    #[test]
    fn arbitrary_tallies_fit_their_schema_field_by_field(
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
        prop_assert_eq!(breaks_schema(&tally_schema(), &json), Vec::<String>::new());
        let back: TallyReport = serde_json::from_value(json).unwrap();
        prop_assert_eq!(&back, &tally);
        prop_assert_eq!(tally.check(this_week), Ok(()));
    }
}
