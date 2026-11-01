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

use clauseo_codex_appserver::{
    Account, AccountRateLimitsUpdatedNotification, AmazonBedrockAccount, ChatgptAccount,
    ClientInfo, CreditsSnapshot, GetAccountParams, GetAccountRateLimitsParams,
    GetAccountRateLimitsResponse, GetAccountResponse, InitializeCapabilities, InitializeParams,
    InitializeResponse, PlanType, RateLimitReachedType, RateLimitResetCredit,
    RateLimitResetCreditStatus, RateLimitResetCreditsSummary, RateLimitResetType,
    RateLimitSnapshot, RateLimitWindow, SpendControlLimitSnapshot,
};
use serde_json::Value;

use checker::{Schema, check_enum, check_struct, compare_fields, validate};

/// Definitions in the vendored files that no mirrored type stands for, and why.
const NOT_MIRRORED: &[(&str, &str)] = &[
    // A string alias; `InitializeResponse.codexHome` is a `String`.
    ("AbsolutePathBuf", "v1/InitializeResponse.json"),
    // Only `account/read`'s experimental `workspaceRouting` field uses these, and the schema's
    // stable form leaves that field out.
    ("AccountRoutingOverride", "v2/GetAccountResponse.json"),
    ("WorkspaceRouting", "v2/GetAccountResponse.json"),
];

/// The snapshot types, defined alike in both rate limit files.
fn check_snapshot_types(schema: &Schema, findings: &mut Vec<String>) {
    findings.extend(check_struct::<RateLimitSnapshot>(
        schema,
        schema.def("RateLimitSnapshot"),
        &[],
    ));
    findings.extend(check_struct::<RateLimitWindow>(
        schema,
        schema.def("RateLimitWindow"),
        &[],
    ));
    findings.extend(check_struct::<CreditsSnapshot>(
        schema,
        schema.def("CreditsSnapshot"),
        &[],
    ));
    findings.extend(check_struct::<SpendControlLimitSnapshot>(
        schema,
        schema.def("SpendControlLimitSnapshot"),
        &[],
    ));
    findings.extend(check_enum(
        "PlanType",
        PlanType::VALUES,
        schema.def("PlanType"),
        None,
    ));
    findings.extend(check_enum(
        "RateLimitReachedType",
        RateLimitReachedType::VALUES,
        schema.def("RateLimitReachedType"),
        None,
    ));
}

#[test]
fn schema_drift() {
    let mut findings = Vec::new();

    let schema = Schema::vendored("v1/InitializeParams.json");
    findings.extend(check_struct::<InitializeParams>(
        &schema,
        schema.def(""),
        &[],
    ));
    findings.extend(check_struct::<ClientInfo>(
        &schema,
        schema.def("ClientInfo"),
        &[],
    ));
    findings.extend(check_struct::<InitializeCapabilities>(
        &schema,
        schema.def("InitializeCapabilities"),
        &[],
    ));

    let schema = Schema::vendored("v1/InitializeResponse.json");
    findings.extend(check_struct::<InitializeResponse>(
        &schema,
        schema.def(""),
        &[],
    ));

    // `initialized` is the client's one notification, and it has no params.
    let schema = Schema::vendored("ClientNotification.json");
    let initialized = schema.variant("", "InitializedNotification");
    findings.extend(check_enum(
        "ClientNotification",
        &["initialized"],
        schema.def(""),
        Some("method"),
    ));
    findings.extend(compare_fields(
        "InitializedNotification",
        &[],
        initialized,
        &["method"],
    ));

    let schema = Schema::vendored("v2/GetAccountParams.json");
    findings.extend(check_struct::<GetAccountParams>(
        &schema,
        schema.def(""),
        &[],
    ));

    let schema = Schema::vendored("v2/GetAccountResponse.json");
    findings.extend(check_struct::<GetAccountResponse>(
        &schema,
        schema.def(""),
        &[],
    ));
    findings.extend(check_enum(
        "Account",
        Account::VALUES,
        schema.def("Account"),
        Some("type"),
    ));
    let api_key = schema.variant("Account", "ApiKeyAccount");
    findings.extend(compare_fields("Account::ApiKey", &[], api_key, &["type"]));
    let chatgpt = schema.variant("Account", "ChatgptAccount");
    findings.extend(check_struct::<ChatgptAccount>(&schema, chatgpt, &["type"]));
    let bedrock = schema.variant("Account", "AmazonBedrockAccount");
    findings.extend(check_struct::<AmazonBedrockAccount>(
        &schema,
        bedrock,
        &["type"],
    ));
    findings.extend(check_enum(
        "PlanType",
        PlanType::VALUES,
        schema.def("PlanType"),
        None,
    ));

    let schema = Schema::vendored("v2/NullableGetAccountRateLimitsParams.json");
    let params = schema.def("GetAccountRateLimitsParams");
    findings.extend(check_struct::<GetAccountRateLimitsParams>(
        &schema,
        params,
        &[],
    ));

    let schema = Schema::vendored("v2/GetAccountRateLimitsResponse.json");
    findings.extend(check_struct::<GetAccountRateLimitsResponse>(
        &schema,
        schema.def(""),
        &[],
    ));
    check_snapshot_types(&schema, &mut findings);
    findings.extend(check_struct::<RateLimitResetCreditsSummary>(
        &schema,
        schema.def("RateLimitResetCreditsSummary"),
        &[],
    ));
    findings.extend(check_struct::<RateLimitResetCredit>(
        &schema,
        schema.def("RateLimitResetCredit"),
        &[],
    ));
    findings.extend(check_enum(
        "RateLimitResetCreditStatus",
        RateLimitResetCreditStatus::VALUES,
        schema.def("RateLimitResetCreditStatus"),
        None,
    ));
    findings.extend(check_enum(
        "RateLimitResetType",
        RateLimitResetType::VALUES,
        schema.def("RateLimitResetType"),
        None,
    ));

    let schema = Schema::vendored("v2/AccountRateLimitsUpdatedNotification.json");
    findings.extend(check_struct::<AccountRateLimitsUpdatedNotification>(
        &schema,
        schema.def(""),
        &[],
    ));
    check_snapshot_types(&schema, &mut findings);

    assert!(
        findings.is_empty(),
        "the types drifted from the schema:\n{}",
        findings.join("\n")
    );
}

/// Every definition in a vendored file is checked above or listed in [`NOT_MIRRORED`], so a
/// pin bump that adds one is noticed.
#[test]
fn every_vendored_definition_is_mirrored_or_listed() {
    let mirrored: &[(&str, &[&str])] = &[
        (
            "v1/InitializeParams.json",
            &["ClientInfo", "InitializeCapabilities"],
        ),
        ("v1/InitializeResponse.json", &[]),
        ("v2/GetAccountResponse.json", &["Account", "PlanType"]),
        (
            "v2/NullableGetAccountRateLimitsParams.json",
            &["GetAccountRateLimitsParams"],
        ),
        (
            "v2/GetAccountRateLimitsResponse.json",
            &[
                "CreditsSnapshot",
                "PlanType",
                "RateLimitReachedType",
                "RateLimitResetCredit",
                "RateLimitResetCreditStatus",
                "RateLimitResetCreditsSummary",
                "RateLimitResetType",
                "RateLimitSnapshot",
                "RateLimitWindow",
                "SpendControlLimitSnapshot",
            ],
        ),
        (
            "v2/AccountRateLimitsUpdatedNotification.json",
            &[
                "CreditsSnapshot",
                "PlanType",
                "RateLimitReachedType",
                "RateLimitSnapshot",
                "RateLimitWindow",
                "SpendControlLimitSnapshot",
            ],
        ),
        ("v2/GetAccountParams.json", &[]),
        ("ClientNotification.json", &[]),
    ];
    let mut unlisted = Vec::new();
    for (file, names) in mirrored {
        for name in Schema::vendored(file).definition_names() {
            let skipped = NOT_MIRRORED.contains(&(name.as_str(), *file));
            if !names.contains(&name.as_str()) && !skipped {
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
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let readme = std::fs::read_to_string(folder.join("README.md")).unwrap();
    let rows: Vec<(String, String)> = readme
        .lines()
        .filter_map(|line| {
            let mut cells = line.split('|').map(|cell| cell.trim().trim_matches('`'));
            let fixture = cells.nth(1)?;
            let file = cells.next()?;
            std::path::Path::new(fixture)
                .extension()
                .is_some_and(|extension| extension == "json")
                .then(|| (fixture.to_owned(), file.to_owned()))
        })
        .collect();
    let mut on_disk: Vec<String> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| {
            std::path::Path::new(name)
                .extension()
                .is_some_and(|e| e == "json")
        })
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
