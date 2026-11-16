//! The tauri-config builders for `tauri.conf.json` with one key changed, or unreadable.

use super::*;

pub(in crate::checks::tauri_config::tests) fn dev_csp_not_checked() -> Repo {
    conf_case(
        "dev-csp-not-checked",
        &csp_entry(),
        &format!(
            "{},\n      \"devCsp\": \"script-src 'unsafe-eval'\"",
            csp_entry()
        ),
    )
}

/// `CONF` with `endpoints` and `pubkey` set to these JSON texts.
fn updater_conf(endpoints: &str, pubkey: &str) -> String {
    edited_conf("\"endpoints\": []", &format!("\"endpoints\": {endpoints}"))
        .replace("\"pubkey\": \"\"", &format!("\"pubkey\": {pubkey}"))
}

/// A desktop crate whose `tauri.conf.json` is [`updater_conf`].
fn updater_case(case: &str, endpoints: &str, pubkey: &str) -> Repo {
    let conf = updater_conf(endpoints, pubkey);
    desktop_repo(
        case,
        &[(CONF_AT, conf.as_bytes()), (BUILD_AT, BUILD.as_bytes())],
    )
}

/// The release endpoint as a JSON array.
fn release_endpoints() -> String {
    format!("[\"{}\"]", updater::RELEASE_ENDPOINT)
}

pub(in crate::checks::tauri_config::tests) fn endpoints_with_pubkey_passes() -> Repo {
    updater_case(
        "endpoints-with-pubkey",
        &release_endpoints(),
        &format!("\"{}\"", updater::tests::other_key()),
    )
}

pub(in crate::checks::tauri_config::tests) fn other_endpoint() -> Repo {
    updater_case(
        "other-endpoint",
        "[\"https://releases.example/latest.json\"]",
        &format!("\"{}\"", updater::tests::other_key()),
    )
}

pub(in crate::checks::tauri_config::tests) fn second_endpoint() -> Repo {
    updater_case(
        "second-endpoint",
        &format!(
            "[\"{}\", \"https://releases.example/latest.json\"]",
            updater::RELEASE_ENDPOINT
        ),
        &format!("\"{}\"", updater::tests::other_key()),
    )
}

pub(in crate::checks::tauri_config::tests) fn placeholder_pubkey() -> Repo {
    updater_case(
        "placeholder-pubkey",
        &release_endpoints(),
        &format!(
            "\"{}\"",
            updater::tests::release_pub("updater-placeholder.pub")
        ),
    )
}

pub(in crate::checks::tauri_config::tests) fn test_pubkey() -> Repo {
    updater_case(
        "test-pubkey",
        &release_endpoints(),
        &format!("\"{}\"", updater::tests::release_pub("updater-test.pub")),
    )
}

pub(in crate::checks::tauri_config::tests) fn pubkey_not_a_key() -> Repo {
    updater_case(
        "pubkey-not-a-key",
        &release_endpoints(),
        "\"dW50cnVzdGVkIGNvbW1lbnQ=\"",
    )
}

pub(in crate::checks::tauri_config::tests) fn updater_artifacts_true_passes() -> Repo {
    conf_case(
        "updater-artifacts-true",
        "\"icon\": []",
        "\"icon\": [],\n    \"createUpdaterArtifacts\": true",
    )
}

pub(in crate::checks::tauri_config::tests) fn asset_protocol_disabled_passes() -> Repo {
    conf_case(
        "asset-protocol-disabled",
        "\"freezePrototype\": true,",
        "\"freezePrototype\": true,\n      \"assetProtocol\": { \"enable\": false },",
    )
}

pub(in crate::checks::tauri_config::tests) fn conf_missing() -> Repo {
    desktop_repo("conf-missing", &[(BUILD_AT, BUILD.as_bytes())])
}

pub(in crate::checks::tauri_config::tests) fn conf_not_utf8() -> Repo {
    raw_conf_case("conf-not-utf8", &[0xff, 0xfe, b'{'])
}

pub(in crate::checks::tauri_config::tests) fn conf_invalid_json() -> Repo {
    raw_conf_case("conf-invalid-json", b"{ \"app\": ")
}

pub(in crate::checks::tauri_config::tests) fn with_global_tauri_true() -> Repo {
    conf_case(
        "with-global-tauri-true",
        "\"withGlobalTauri\": false",
        "\"withGlobalTauri\": true",
    )
}

pub(in crate::checks::tauri_config::tests) fn with_global_tauri_missing() -> Repo {
    conf_case(
        "with-global-tauri-missing",
        "\"withGlobalTauri\": false,",
        "",
    )
}

pub(in crate::checks::tauri_config::tests) fn freeze_prototype_false() -> Repo {
    conf_case(
        "freeze-prototype-false",
        "\"freezePrototype\": true",
        "\"freezePrototype\": false",
    )
}

pub(in crate::checks::tauri_config::tests) fn freeze_prototype_missing() -> Repo {
    conf_case("freeze-prototype-missing", "\"freezePrototype\": true,", "")
}

pub(in crate::checks::tauri_config::tests) fn csp_not_pinned() -> Repo {
    let trimmed = PINNED_CSP.replace("; require-trusted-types-for 'script'", "");
    conf_case(
        "csp-not-pinned",
        &csp_entry(),
        &format!("\"csp\": \"{trimmed}\""),
    )
}

pub(in crate::checks::tauri_config::tests) fn csp_object() -> Repo {
    conf_case(
        "csp-object",
        &csp_entry(),
        "\"csp\": { \"default-src\": \"'self'\", \"script-src\": [\"'self'\"] }",
    )
}

pub(in crate::checks::tauri_config::tests) fn csp_not_string() -> Repo {
    conf_case("csp-not-string", &csp_entry(), "\"csp\": true")
}

pub(in crate::checks::tauri_config::tests) fn csp_missing() -> Repo {
    conf_case(
        "csp-missing",
        &format!("[\"main\"],\n      {}", csp_entry()),
        "[\"main\"]",
    )
}

pub(in crate::checks::tauri_config::tests) fn frontend_dist_url() -> Repo {
    conf_case(
        "frontend-dist-url",
        "\"frontendDist\": \"../../ui/app/dist\"",
        "\"frontendDist\": \"https://evil.example\"",
    )
}

pub(in crate::checks::tauri_config::tests) fn frontend_dist_list() -> Repo {
    conf_case(
        "frontend-dist-list",
        "\"frontendDist\": \"../../ui/app/dist\"",
        "\"frontendDist\": [\"index.html\"]",
    )
}

pub(in crate::checks::tauri_config::tests) fn app_windows_config_window() -> Repo {
    conf_case(
        "app-windows-config-window",
        "\"windows\": []",
        "\"windows\": [{ \"label\": \"main\" }]",
    )
}

pub(in crate::checks::tauri_config::tests) fn asset_protocol_enabled() -> Repo {
    conf_case(
        "asset-protocol-enabled",
        "\"freezePrototype\": true,",
        "\"freezePrototype\": true,\n      \"assetProtocol\": { \"enable\": true },",
    )
}

pub(in crate::checks::tauri_config::tests) fn updater_endpoints_without_pubkey() -> Repo {
    updater_case(
        "updater-endpoints-without-pubkey",
        &release_endpoints(),
        "\"\"",
    )
}

pub(in crate::checks::tauri_config::tests) fn whitespace_pubkey_with_endpoints() -> Repo {
    updater_case(
        "whitespace-pubkey-with-endpoints",
        &release_endpoints(),
        "\"   \"",
    )
}

pub(in crate::checks::tauri_config::tests) fn updater_v1_compatible() -> Repo {
    conf_case(
        "updater-v1-compatible",
        "\"icon\": []",
        "\"icon\": [],\n    \"createUpdaterArtifacts\": \"v1Compatible\"",
    )
}

pub(in crate::checks::tauri_config::tests) fn dangerous_key_nested() -> Repo {
    conf_case(
        "dangerous-key-nested",
        "\"freezePrototype\": true,",
        "\"freezePrototype\": true,\n      \"assetProtocol\": { \"enable\": false, \"dangerousScope\": true },",
    )
}

pub(in crate::checks::tauri_config::tests) fn dangerous_key_in_an_array() -> Repo {
    conf_case(
        "dangerous-key-in-an-array",
        "\"capabilities\": [\"main\"]",
        "\"capabilities\": [\"main\", { \"identifier\": \"ipc\", \"dangerousScope\": true }]",
    )
}

/// `withGlobalTauri` written twice, the safe value first: the last one wins, as in Tauri.
pub(in crate::checks::tauri_config::tests) fn duplicate_key_unsafe_last() -> Repo {
    conf_case(
        "duplicate-key-unsafe-last",
        "\"withGlobalTauri\": false,",
        "\"withGlobalTauri\": false,\n    \"withGlobalTauri\": true,",
    )
}
