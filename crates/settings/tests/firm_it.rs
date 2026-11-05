//! The samples in `docs/firm-it/` against the policy readers (S25): `policy.sample.json` through
//! the `policy.json` reader and the configuration profile's payload through the managed
//! preferences parser, each with no warning on the OS it is written for. Both samples, and the
//! policy key table in `docs/firm-it.md`, name every settings key the schema has (but those in
//! `COMING`), so a new key fails here until the notes for firm IT list it.
//!
//! No reader opens a profile: macOS reads managed preferences through `CFPreferences`. So the
//! payload is read with `plist`, and each key goes to `managed_prefs::parse` as a forced value.

use clauseo_settings::policy::file;
use clauseo_settings::policy::managed_prefs::{self, PlistValue};
use clauseo_settings::{
    Error, IDENTIFIER, PolicyRead, PolicyValues, Warning, WarningKind, json_schema,
};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// A file under the repo's `docs` folder.
fn doc(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs")
        .join(name)
}

/// Keys the schema has that the notes still list under "### Coming", and the samples leave out:
/// a key read and locked, but that nothing acts on yet. `update.mode` stays there until the
/// updater works (R23); the pull request that wires it moves the row into the table, adds it to
/// both samples and takes it out of here.
const COMING: [&str; 1] = ["update.mode"];

/// Every settings key, from the JSON Schema: `<section>.<field>`, and an agent's as
/// `agents.<id>`, less the [`COMING`] ones. `None` when the schema's shape is not the one read
/// here.
fn schema_keys() -> Option<BTreeSet<String>> {
    let mut keys = all_schema_keys()?;
    for key in COMING {
        // A key in `COMING` the schema does not have is a stale entry.
        if !keys.remove(key) {
            return None;
        }
    }
    Some(keys)
}

/// Every settings key, from the JSON Schema, [`COMING`] ones included.
fn all_schema_keys() -> Option<BTreeSet<String>> {
    let schema = json_schema();
    let mut keys = BTreeSet::new();
    for (section, value) in schema["properties"].as_object()? {
        match value.get("$ref").and_then(Value::as_str) {
            Some(reference) => {
                let fields = schema
                    .pointer(reference.trim_start_matches('#'))?
                    .get("properties")?
                    .as_object()?;
                keys.extend(fields.keys().map(|field| format!("{section}.{field}")));
            }
            None => {
                keys.insert(format!("{section}.<id>"));
            }
        }
    }
    Some(keys)
}

/// The keys a source locks, an agent's as `agents.<id>` and a plan's as `plans.<id>`.
fn locked(values: &PolicyValues) -> BTreeSet<String> {
    values
        .keys()
        .map(|key| {
            if key.starts_with("agents.") {
                "agents.<id>".to_owned()
            } else if key.starts_with("plans.") {
                "plans.<id>".to_owned()
            } else {
                key
            }
        })
        .collect()
}

/// What each warning names: a key, or an agent's key.
fn named(warnings: &[Warning]) -> BTreeSet<String> {
    warnings
        .iter()
        .map(|warning| match &warning.kind {
            WarningKind::UnknownKey { key } | WarningKind::InvalidValue { key, .. } => key.clone(),
            WarningKind::RelativeAgentCommand { id, .. } => format!("agents.{id}"),
            other => format!("{other:?}"),
        })
        .collect()
}

/// The warnings a sample's absolute paths give on an OS they are not absolute on: the default
/// matters folder, the agent and the MCP server, each written for the other OS.
fn path_warnings() -> BTreeSet<String> {
    [
        "matters.default_location",
        "agents.claude",
        "mcp.servers.firm-docs",
        "install.bundle",
    ]
    .map(str::to_owned)
    .into()
}

/// `docs/firm-it/policy.sample.json` through the `policy.json` reader.
fn read_sample(bytes: &[u8]) -> Result<PolicyValues, Error> {
    file::parse(&doc("firm-it/policy.sample.json"), bytes)
}

#[test]
fn the_policy_sample_loads_with_no_warning_and_sets_every_key() {
    let bytes = std::fs::read(doc("firm-it/policy.sample.json")).unwrap();
    let values = read_sample(&bytes).unwrap();
    // Its paths are Windows paths, for `C:\ProgramData\Clauseo\policy.json`.
    let expected = if cfg!(windows) {
        BTreeSet::new()
    } else {
        path_warnings()
    };
    assert_eq!(
        named(values.warnings()),
        expected,
        "{:?}",
        values.warnings()
    );
    assert_eq!(locked(&values), schema_keys().unwrap());
}

#[test]
fn a_misspelt_key_in_the_policy_sample_warns() {
    let text = std::fs::read_to_string(doc("firm-it/policy.sample.json")).unwrap();
    let values = read_sample(text.replace("\"theme\"", "\"colour\"").as_bytes()).unwrap();
    assert!(named(values.warnings()).contains("ui.colour"));
    assert!(!locked(&values).contains("ui.theme"));
}

/// A property list value as the macOS shim gives it.
fn plist_value(value: plist::Value) -> PlistValue {
    match value {
        plist::Value::Boolean(flag) => PlistValue::Bool(flag),
        plist::Value::Integer(number) => number
            .as_signed()
            .map_or(PlistValue::Other("number"), PlistValue::Integer),
        plist::Value::Real(number) => PlistValue::Real(number),
        plist::Value::String(text) => PlistValue::String(text),
        plist::Value::Array(items) => {
            PlistValue::Array(items.into_iter().map(plist_value).collect())
        }
        plist::Value::Dictionary(entries) => PlistValue::Dict(
            entries
                .into_iter()
                .map(|(key, value)| (key, plist_value(value)))
                .collect(),
        ),
        plist::Value::Date(_) => PlistValue::Other("date"),
        plist::Value::Data(_) => PlistValue::Other("data"),
        _ => PlistValue::Other("value"),
    }
}

#[test]
fn the_profile_sample_loads_with_no_warning_and_sets_every_key() {
    let profile = plist::Value::from_file(doc("firm-it/ai.clauseo.desktop.mobileconfig")).unwrap();
    let payloads = profile.as_dictionary().unwrap()["PayloadContent"]
        .as_array()
        .unwrap();
    let payload = payloads
        .iter()
        .filter_map(plist::Value::as_dictionary)
        .find(|payload| payload["PayloadType"].as_string() == Some(IDENTIFIER))
        .unwrap();
    // The payload's own `Payload*` keys describe it; the rest are the domain's preferences.
    let forced: Vec<(String, PlistValue, bool)> = payload
        .iter()
        .filter(|(key, _)| !key.starts_with("Payload"))
        .map(|(key, value)| (key.clone(), plist_value(value.clone()), true))
        .collect();
    let PolicyRead::Read(values) = managed_prefs::parse(forced) else {
        panic!("the profile forces no value");
    };
    // Its paths are macOS paths.
    let expected = if cfg!(windows) {
        path_warnings()
    } else {
        BTreeSet::new()
    };
    assert_eq!(
        named(values.warnings()),
        expected,
        "{:?}",
        values.warnings()
    );
    assert_eq!(locked(&values), schema_keys().unwrap());
}

#[test]
fn the_notes_list_every_policy_key() {
    let notes = std::fs::read_to_string(doc("firm-it.md")).unwrap();
    // The table under "## 6.", up to the next heading ("### Coming" lists keys not on main):
    // each row's first code span is a key.
    let section = notes
        .split("\n## 6.")
        .nth(1)
        .unwrap_or_else(|| panic!("firm-it.md has no section 6"));
    let table: BTreeSet<String> = section
        .lines()
        .skip(1)
        .take_while(|line| !line.starts_with('#'))
        .filter_map(|line| line.strip_prefix("| `"))
        .filter_map(|row| row.split('`').next())
        .map(str::to_owned)
        .collect();
    assert_eq!(table, schema_keys().unwrap());
    // Each key still to come is listed under "### Coming", so the notes do not drop it.
    let coming = section
        .split("\n### Coming")
        .nth(1)
        .unwrap_or_else(|| panic!("firm-it.md section 6 has no Coming list"));
    let coming = coming.split("\n## ").next().unwrap_or(coming);
    for key in COMING {
        assert!(coming.contains(&format!("| `{key}`")), "{key}");
    }
}
