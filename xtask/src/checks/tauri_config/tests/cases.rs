//! The builders of the tauri-config planted cases. Each writes a desktop crate into a scratch repo
//! and runs the check's own `run` on it: a valid `tauri.conf.json` (`CONF`) and `build.rs`
//! (`BUILD`), with one thing changed.

use super::*;
use crate::checks::tests::desktop_context;
use crate::scratch::Scratch;
use std::fs;

/// Runs the check on a scratch repo whose desktop crate holds `files`, each a name and its bytes.
fn run_files(case: &str, files: &[(&str, &[u8])]) -> Inspection {
    let dir = Scratch::new(&format!("planted-tauri-config-{case}"));
    fs::create_dir_all(dir.join("crates/desktop")).expect("create the desktop dir");
    for (name, bytes) in files {
        fs::write(dir.join("crates/desktop").join(name), bytes).expect("write a desktop file");
    }
    run(&desktop_context(&dir))
}

/// `CONF` with `from` replaced by `to`, once.
fn edited_conf(from: &str, to: &str) -> String {
    assert!(CONF.contains(from), "the fixture contains {from}");
    CONF.replacen(from, to, 1)
}

/// A desktop crate whose `tauri.conf.json` is `CONF` with `from` replaced by `to`, and whose
/// `build.rs` is valid.
fn conf_case(case: &str, from: &str, to: &str) -> Inspection {
    let conf = edited_conf(from, to);
    run_files(
        case,
        &[(TAURI_CONF, conf.as_bytes()), (BUILD_RS, BUILD.as_bytes())],
    )
}

/// A desktop crate whose `build.rs` is `build` and whose `tauri.conf.json` is valid.
fn build_case(case: &str, build: &str) -> Inspection {
    run_files(
        case,
        &[(TAURI_CONF, CONF.as_bytes()), (BUILD_RS, build.as_bytes())],
    )
}

/// A desktop crate whose `build.rs` is valid and whose `tauri.conf.json` is `conf`.
fn raw_conf_case(case: &str, conf: &[u8]) -> Inspection {
    run_files(case, &[(TAURI_CONF, conf), (BUILD_RS, BUILD.as_bytes())])
}

/// The valid desktop crate with a platform config `name` next to its valid files.
fn platform_case(case: &str, name: &str, text: &str) -> Inspection {
    run_files(
        case,
        &[
            (TAURI_CONF, CONF.as_bytes()),
            (BUILD_RS, BUILD.as_bytes()),
            (name, text.as_bytes()),
        ],
    )
}

pub(super) fn clean_tree() -> Inspection {
    run_files(
        "clean-tree",
        &[(TAURI_CONF, CONF.as_bytes()), (BUILD_RS, BUILD.as_bytes())],
    )
}

pub(super) fn app_manifest_with_commands_passes() -> Inspection {
    build_case(
        "app-manifest-with-commands",
        &BUILD.replace(
            "AppManifest::new()",
            "AppManifest::new().commands(&[\"open_matter\"])",
        ),
    )
}

pub(super) fn dev_csp_not_checked() -> Inspection {
    conf_case(
        "dev-csp-not-checked",
        &csp_entry(),
        &format!(
            "{},\n      \"devCsp\": \"script-src 'unsafe-eval'\"",
            csp_entry()
        ),
    )
}

pub(super) fn endpoints_with_pubkey_passes() -> Inspection {
    let conf = edited_conf(
        "\"endpoints\": []",
        "\"endpoints\": [\"https://releases.example/latest.json\"]",
    )
    .replace(
        "\"pubkey\": \"\"",
        "\"pubkey\": \"dW50cnVzdGVkIGNvbW1lbnQ=\"",
    );
    run_files(
        "endpoints-with-pubkey",
        &[(TAURI_CONF, conf.as_bytes()), (BUILD_RS, BUILD.as_bytes())],
    )
}

pub(super) fn updater_artifacts_true_passes() -> Inspection {
    conf_case(
        "updater-artifacts-true",
        "\"icon\": []",
        "\"icon\": [],\n    \"createUpdaterArtifacts\": true",
    )
}

pub(super) fn asset_protocol_disabled_passes() -> Inspection {
    conf_case(
        "asset-protocol-disabled",
        "\"freezePrototype\": true,",
        "\"freezePrototype\": true,\n      \"assetProtocol\": { \"enable\": false },",
    )
}

pub(super) fn conf_missing() -> Inspection {
    run_files("conf-missing", &[(BUILD_RS, BUILD.as_bytes())])
}

pub(super) fn conf_not_utf8() -> Inspection {
    raw_conf_case("conf-not-utf8", &[0xff, 0xfe, b'{'])
}

pub(super) fn conf_invalid_json() -> Inspection {
    raw_conf_case("conf-invalid-json", b"{ \"app\": ")
}

pub(super) fn with_global_tauri_true() -> Inspection {
    conf_case(
        "with-global-tauri-true",
        "\"withGlobalTauri\": false",
        "\"withGlobalTauri\": true",
    )
}

pub(super) fn with_global_tauri_missing() -> Inspection {
    conf_case(
        "with-global-tauri-missing",
        "\"withGlobalTauri\": false,",
        "",
    )
}

pub(super) fn freeze_prototype_false() -> Inspection {
    conf_case(
        "freeze-prototype-false",
        "\"freezePrototype\": true",
        "\"freezePrototype\": false",
    )
}

pub(super) fn freeze_prototype_missing() -> Inspection {
    conf_case("freeze-prototype-missing", "\"freezePrototype\": true,", "")
}

pub(super) fn csp_not_pinned() -> Inspection {
    let trimmed = PINNED_CSP.replace("; require-trusted-types-for 'script'", "");
    conf_case(
        "csp-not-pinned",
        &csp_entry(),
        &format!("\"csp\": \"{trimmed}\""),
    )
}

pub(super) fn csp_object() -> Inspection {
    conf_case(
        "csp-object",
        &csp_entry(),
        "\"csp\": { \"default-src\": \"'self'\", \"script-src\": [\"'self'\"] }",
    )
}

pub(super) fn csp_not_string() -> Inspection {
    conf_case("csp-not-string", &csp_entry(), "\"csp\": true")
}

pub(super) fn csp_missing() -> Inspection {
    conf_case(
        "csp-missing",
        &format!("[\"main\"],\n      {}", csp_entry()),
        "[\"main\"]",
    )
}

pub(super) fn frontend_dist_url() -> Inspection {
    conf_case(
        "frontend-dist-url",
        "\"frontendDist\": \"../../ui/app/dist\"",
        "\"frontendDist\": \"https://evil.example\"",
    )
}

pub(super) fn frontend_dist_list() -> Inspection {
    conf_case(
        "frontend-dist-list",
        "\"frontendDist\": \"../../ui/app/dist\"",
        "\"frontendDist\": [\"index.html\"]",
    )
}

pub(super) fn app_windows_config_window() -> Inspection {
    conf_case(
        "app-windows-config-window",
        "\"windows\": []",
        "\"windows\": [{ \"label\": \"main\" }]",
    )
}

pub(super) fn asset_protocol_enabled() -> Inspection {
    conf_case(
        "asset-protocol-enabled",
        "\"freezePrototype\": true,",
        "\"freezePrototype\": true,\n      \"assetProtocol\": { \"enable\": true },",
    )
}

pub(super) fn updater_endpoints_without_pubkey() -> Inspection {
    conf_case(
        "updater-endpoints-without-pubkey",
        "\"endpoints\": []",
        "\"endpoints\": [\"https://releases.example/latest.json\"]",
    )
}

pub(super) fn whitespace_pubkey_with_endpoints() -> Inspection {
    let conf = edited_conf(
        "\"endpoints\": []",
        "\"endpoints\": [\"https://releases.example/latest.json\"]",
    )
    .replace("\"pubkey\": \"\"", "\"pubkey\": \"   \"");
    run_files(
        "whitespace-pubkey-with-endpoints",
        &[(TAURI_CONF, conf.as_bytes()), (BUILD_RS, BUILD.as_bytes())],
    )
}

pub(super) fn updater_v1_compatible() -> Inspection {
    conf_case(
        "updater-v1-compatible",
        "\"icon\": []",
        "\"icon\": [],\n    \"createUpdaterArtifacts\": \"v1Compatible\"",
    )
}

pub(super) fn dangerous_key_nested() -> Inspection {
    conf_case(
        "dangerous-key-nested",
        "\"freezePrototype\": true,",
        "\"freezePrototype\": true,\n      \"assetProtocol\": { \"enable\": false, \"dangerousScope\": true },",
    )
}

pub(super) fn dangerous_key_in_an_array() -> Inspection {
    conf_case(
        "dangerous-key-in-an-array",
        "\"capabilities\": [\"main\"]",
        "\"capabilities\": [\"main\", { \"identifier\": \"ipc\", \"dangerousScope\": true }]",
    )
}

pub(super) fn platform_config_json() -> Inspection {
    platform_case(
        "platform-config-json",
        "tauri.macos.conf.json",
        "{\"app\":{\"withGlobalTauri\":true}}",
    )
}

pub(super) fn platform_config_json5() -> Inspection {
    platform_case(
        "platform-config-json5",
        "tauri.linux.conf.json5",
        "{ app: { withGlobalTauri: true } }",
    )
}

pub(super) fn platform_config_toml() -> Inspection {
    platform_case(
        "platform-config-toml",
        "Tauri.windows.toml",
        "[app]\nwithGlobalTauri = true\n",
    )
}

pub(super) fn build_rs_missing() -> Inspection {
    run_files("build-rs-missing", &[(TAURI_CONF, CONF.as_bytes())])
}

pub(super) fn build_rs_without_app_manifest() -> Inspection {
    build_case(
        "build-rs-without-app-manifest",
        "fn main() {\n    tauri_build::build();\n}\n",
    )
}

pub(super) fn app_manifest_new_not_passed() -> Inspection {
    build_case(
        "app-manifest-new-not-passed",
        "fn main() {\n    let _ = tauri_build::AppManifest::new();\n    tauri_build::build();\n}\n",
    )
}

pub(super) fn app_manifest_without_new() -> Inspection {
    build_case(
        "app-manifest-without-new",
        "fn main() {\n    tauri_build::try_build(\n        tauri_build::Attributes::new().app_manifest(manifest()),\n    );\n}\n",
    )
}

pub(super) fn app_manifest_only_in_block_comment() -> Inspection {
    build_case(
        "app-manifest-only-in-block-comment",
        "/*\n    tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new())\n*/\nfn main() {\n    tauri_build::build();\n}\n",
    )
}

pub(super) fn app_manifest_only_in_line_comment() -> Inspection {
    build_case(
        "app-manifest-only-in-line-comment",
        "// tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new());\nfn main() {\n    tauri_build::build();\n}\n",
    )
}

pub(super) fn manifest_new_in_trailing_comment() -> Inspection {
    build_case(
        "manifest-new-in-trailing-comment",
        "fn main() {\n    let a = tauri_build::Attributes::new().app_manifest(manifest()); // tauri_build::AppManifest::new()\n}\n",
    )
}

pub(super) fn app_manifest_in_string_only() -> Inspection {
    build_case(
        "app-manifest-in-string-only",
        r#"fn main() {
    let note = "tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new())";
    tauri_build::build();
}
"#,
    )
}

pub(super) fn app_manifest_only_defined() -> Inspection {
    build_case(
        "app-manifest-only-defined",
        "fn main() {\n    let _ = tauri_build::AppManifest::new();\n    tauri_build::build();\n}\nfn app_manifest(_manifest: tauri_build::AppManifest) {}\n",
    )
}

pub(super) fn build_rs_not_valid_rust() -> Inspection {
    build_case("build-rs-not-valid-rust", "fn main() {\n    let x = ;\n}\n")
}

pub(super) fn build_rs_not_utf8() -> Inspection {
    run_files(
        "build-rs-not-utf8",
        &[
            (TAURI_CONF, CONF.as_bytes()),
            (BUILD_RS, &[0xff, 0xfe, b'f']),
        ],
    )
}

pub(super) fn desktop_crate_missing() -> Inspection {
    let dir = Scratch::new("planted-tauri-config-desktop-crate-missing");
    run(&Context {
        root: dir.to_path_buf(),
        members: Ok(Vec::new()),
        load_resolved_graph: false,
    })
}

/// The desktop crate with neither tauri.conf.json nor build.rs: the check looks and finds none.
pub(super) fn both_config_files_missing() -> Inspection {
    run_files("both-config-files-missing", &[])
}
