//! `schema_drift`: every mirrored type against its definition in the vendored schema, and every
//! fixture against the definition it stands for. A pin bump that renames a field, changes
//! whether one is required, or adds an enum value fails here until the types follow.
//!
//! A test crate root resolves modules beside itself, so the checker names its path.

#[cfg(test)]
#[path = "schema_drift/checker.rs"]
mod checker;

#[cfg(test)]
#[path = "schema_drift/checker_tests.rs"]
mod checker_tests;

use std::collections::BTreeMap;
use std::path::Path;

use clauseo_codex_appserver::{
    Account, AccountRateLimitsUpdatedNotification, AmazonBedrockAccount, ChatgptAccount,
    ClientInfo, CreditsSnapshot, GetAccountParams, GetAccountRateLimitsParams,
    GetAccountRateLimitsResponse, GetAccountResponse, InitializeCapabilities, InitializeParams,
    InitializeResponse, PlanType, RateLimitReachedType, RateLimitResetCredit,
    RateLimitResetCreditStatus, RateLimitResetCreditsSummary, RateLimitResetType,
    RateLimitSnapshot, RateLimitWindow, SpendControlLimitSnapshot,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

use checker::{Schema, check_enum, check_struct, compare_fields, validate, vendored_folder};

/// Checks one definition; returns one line per finding.
type Check = fn(&Schema, &Value) -> Vec<String>;

/// A definition in a vendored file (`""` is the file's root) and the check of the type that
/// mirrors it. This table is the one list of what is mirrored: `schema_drift` runs each check,
/// and `every_vendored_definition_is_mirrored_or_listed` reads the names.
struct Mirror {
    file: &'static str,
    definition: &'static str,
    check: Check,
}

const RESPONSE: &str = "v2/GetAccountRateLimitsResponse.json";
const UPDATED: &str = "v2/AccountRateLimitsUpdatedNotification.json";

const MIRRORS: &[Mirror] = &[
    m("v1/InitializeParams.json", "", plain::<InitializeParams>),
    m(
        "v1/InitializeParams.json",
        "ClientInfo",
        plain::<ClientInfo>,
    ),
    m(
        "v1/InitializeParams.json",
        "InitializeCapabilities",
        plain::<InitializeCapabilities>,
    ),
    m(
        "v1/InitializeResponse.json",
        "",
        plain::<InitializeResponse>,
    ),
    m("ClientNotification.json", "", initialized),
    m("v2/GetAccountParams.json", "", plain::<GetAccountParams>),
    m(
        "v2/GetAccountResponse.json",
        "",
        plain::<GetAccountResponse>,
    ),
    m("v2/GetAccountResponse.json", "Account", account),
    m("v2/GetAccountResponse.json", "PlanType", plan_type),
    m(
        "v2/NullableGetAccountRateLimitsParams.json",
        "GetAccountRateLimitsParams",
        plain::<GetAccountRateLimitsParams>,
    ),
    m(RESPONSE, "", plain::<GetAccountRateLimitsResponse>),
    m(RESPONSE, "RateLimitSnapshot", plain::<RateLimitSnapshot>),
    m(RESPONSE, "RateLimitWindow", plain::<RateLimitWindow>),
    m(RESPONSE, "CreditsSnapshot", plain::<CreditsSnapshot>),
    m(
        RESPONSE,
        "SpendControlLimitSnapshot",
        plain::<SpendControlLimitSnapshot>,
    ),
    m(
        RESPONSE,
        "RateLimitResetCreditsSummary",
        plain::<RateLimitResetCreditsSummary>,
    ),
    m(
        RESPONSE,
        "RateLimitResetCredit",
        plain::<RateLimitResetCredit>,
    ),
    m(RESPONSE, "PlanType", plan_type),
    m(RESPONSE, "RateLimitReachedType", reached_type),
    m(RESPONSE, "RateLimitResetCreditStatus", credit_status),
    m(RESPONSE, "RateLimitResetType", reset_type),
    m(UPDATED, "", plain::<AccountRateLimitsUpdatedNotification>),
    m(UPDATED, "RateLimitSnapshot", plain::<RateLimitSnapshot>),
    m(UPDATED, "RateLimitWindow", plain::<RateLimitWindow>),
    m(UPDATED, "CreditsSnapshot", plain::<CreditsSnapshot>),
    m(
        UPDATED,
        "SpendControlLimitSnapshot",
        plain::<SpendControlLimitSnapshot>,
    ),
    m(UPDATED, "PlanType", plan_type),
    m(UPDATED, "RateLimitReachedType", reached_type),
];

/// Definitions in the vendored files that no mirrored type stands for, and why.
const NOT_MIRRORED: &[(&str, &str)] = &[
    // A string alias; `InitializeResponse.codexHome` is a `String`.
    ("v1/InitializeResponse.json", "AbsolutePathBuf"),
    // Only `account/read`'s experimental `workspaceRouting` field uses these, and the schema's
    // stable form leaves that field out.
    ("v2/GetAccountResponse.json", "AccountRoutingOverride"),
    ("v2/GetAccountResponse.json", "WorkspaceRouting"),
];

const fn m(file: &'static str, definition: &'static str, check: Check) -> Mirror {
    Mirror {
        file,
        definition,
        check,
    }
}

fn plain<T: DeserializeOwned>(schema: &Schema, node: &Value) -> Vec<String> {
    check_struct::<T>(schema, node, &[])
}

fn plan_type(_: &Schema, node: &Value) -> Vec<String> {
    check_enum("PlanType", PlanType::VALUES, node, None)
}

fn reached_type(_: &Schema, node: &Value) -> Vec<String> {
    check_enum(
        "RateLimitReachedType",
        RateLimitReachedType::VALUES,
        node,
        None,
    )
}

fn credit_status(_: &Schema, node: &Value) -> Vec<String> {
    check_enum(
        "RateLimitResetCreditStatus",
        RateLimitResetCreditStatus::VALUES,
        node,
        None,
    )
}

fn reset_type(_: &Schema, node: &Value) -> Vec<String> {
    check_enum("RateLimitResetType", RateLimitResetType::VALUES, node, None)
}

/// `initialized` is the client's one notification, and it has no params.
fn initialized(schema: &Schema, node: &Value) -> Vec<String> {
    let mut findings = check_enum("ClientNotification", &["initialized"], node, Some("method"));
    let entry = schema.variant("", "InitializedNotification");
    findings.extend(compare_fields(
        "InitializedNotification",
        &[],
        entry,
        &["method"],
    ));
    findings
}

/// `Account`'s kinds, the fields of each, and that a sample of each decodes to its own variant.
fn account(schema: &Schema, node: &Value) -> Vec<String> {
    let mut findings = check_enum("Account", Account::VALUES, node, Some("type"));
    let api_key = schema.variant("Account", "ApiKeyAccount");
    findings.extend(compare_fields("Account::ApiKey", &[], api_key, &["type"]));
    let chatgpt = schema.variant("Account", "ChatgptAccount");
    findings.extend(check_struct::<ChatgptAccount>(schema, chatgpt, &["type"]));
    let bedrock = schema.variant("Account", "AmazonBedrockAccount");
    findings.extend(check_struct::<AmazonBedrockAccount>(
        schema,
        bedrock,
        &["type"],
    ));
    for entry in node["oneOf"].as_array().into_iter().flatten() {
        let sample = schema.sample(entry);
        match serde_json::from_value::<Account>(sample.clone()) {
            Ok(Account::Unknown(kind)) => {
                findings.push(format!("Account: value `{kind}` decodes to Unknown"));
            }
            Ok(_) => {}
            Err(error) => findings.push(format!("Account: {sample} does not decode: {error}")),
        }
    }
    findings
}

#[test]
fn schema_drift() {
    let mut schemas = BTreeMap::new();
    let mut findings = Vec::new();
    for mirror in MIRRORS {
        let schema = schemas
            .entry(mirror.file)
            .or_insert_with(|| Schema::vendored(mirror.file));
        findings.extend((mirror.check)(schema, schema.def(mirror.definition)));
    }
    assert!(
        findings.is_empty(),
        "the types drifted from the schema:\n{}",
        findings.join("\n")
    );
}

/// Every vendored file has a row in [`MIRRORS`], and every definition in it is mirrored there or
/// listed in [`NOT_MIRRORED`], so a pin bump that adds one is noticed.
#[test]
fn every_vendored_definition_is_mirrored_or_listed() {
    let mut files: Vec<&str> = MIRRORS.iter().map(|mirror| mirror.file).collect();
    files.sort_unstable();
    files.dedup();
    let mut on_disk = Vec::new();
    for folder in ["", "v1", "v2"] {
        for entry in std::fs::read_dir(vendored_folder().join(folder)).unwrap() {
            let path = entry.unwrap().path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                let name = path.file_name().unwrap().to_str().unwrap();
                on_disk.push(if folder.is_empty() {
                    name.to_owned()
                } else {
                    format!("{folder}/{name}")
                });
            }
        }
    }
    on_disk.sort_unstable();
    assert_eq!(
        on_disk, files,
        "every vendored file is read, and every file read is vendored"
    );

    let mut unlisted = Vec::new();
    for file in files {
        for name in Schema::vendored(file).definition_names() {
            let mirrored = MIRRORS
                .iter()
                .any(|mirror| mirror.file == file && mirror.definition == name);
            if !mirrored && !NOT_MIRRORED.contains(&(file, name.as_str())) {
                unlisted.push(format!("{file}: definition `{name}`"));
            }
        }
    }
    assert!(
        unlisted.is_empty(),
        "neither mirrored nor listed:\n{}",
        unlisted.join("\n")
    );
}

/// Each fixture `tests/client.rs` answers with, against the schema file whose root definition
/// `tests/fixtures/README.md`'s table names for it.
#[test]
fn fixtures_match_the_schema() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let is_json = |name: &str| {
        Path::new(name)
            .extension()
            .is_some_and(|extension| extension == "json")
    };
    let readme = std::fs::read_to_string(folder.join("README.md")).unwrap();
    let rows: Vec<(String, String)> = readme
        .lines()
        .filter_map(|line| {
            let mut cells = line.split('|').map(|cell| cell.trim().trim_matches('`'));
            let fixture = cells.nth(1)?;
            let file = cells.next()?;
            is_json(fixture).then(|| (fixture.to_owned(), file.to_owned()))
        })
        .collect();
    let mut on_disk: Vec<String> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| is_json(name))
        .collect();
    on_disk.sort();
    let mut named: Vec<String> = rows.iter().map(|(fixture, _)| fixture.clone()).collect();
    named.sort();
    assert_eq!(
        on_disk, named,
        "every fixture has exactly one row in the README"
    );

    let mut findings = Vec::new();
    for (fixture, file) in &rows {
        let text = std::fs::read_to_string(folder.join(fixture)).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        let schema = Schema::vendored(file);
        findings.extend(validate(&schema, schema.def(""), &value, fixture));
    }
    assert!(
        findings.is_empty(),
        "fixtures off the schema:\n{}",
        findings.join("\n")
    );
}
