//! The planted cases of the tauri-config check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in
//! `tests/repos.rs`.

use crate::checks::planted::{fails, failure};
use crate::checks::tauri_config::Fix;

crate::checks::planted::cases! {
    /// A valid `tauri.conf.json` and `build.rs` pass.
    clean_tree: Passes, inspects 2 => super::repos::clean_tree,
    /// `AppManifest::new().commands(...)` is still a manifest, so the file passes.
    app_manifest_with_commands_passes: Passes, inspects 2
        => super::repos::app_manifest_with_commands_passes,
    /// `app.security.devCsp` is not checked, so a relaxed dev policy passes.
    dev_csp_not_checked: Passes, inspects 2 => super::repos::dev_csp_not_checked,
    /// Updater endpoints with a public key pass.
    endpoints_with_pubkey_passes: Passes, inspects 2 => super::repos::endpoints_with_pubkey_passes,
    /// `bundle.createUpdaterArtifacts` set to `true` passes: only `"v1Compatible"` fails.
    updater_artifacts_true_passes: Passes, inspects 2
        => super::repos::updater_artifacts_true_passes,
    /// The asset protocol switched off passes.
    asset_protocol_disabled_passes: Passes, inspects 2
        => super::repos::asset_protocol_disabled_passes,
    /// A missing `tauri.conf.json` fails at line 1.
    conf_missing: Fails[fails(
        "crates/desktop/tauri.conf.json:1",
        "`crates/desktop/tauri.conf.json` is missing",
        Fix::ConfMissing,
    )], inspects 1 => super::repos::conf_missing,
    /// A `tauri.conf.json` that is not UTF-8 fails at line 1 as unreadable.
    conf_not_utf8: Fails[fails(
        "crates/desktop/tauri.conf.json:1",
        "could not read `crates/desktop/tauri.conf.json`: stream did not contain valid UTF-8",
        crate::checks::Fix::NotUtf8,
    )], inspects 1 => super::repos::conf_not_utf8,
    /// A `tauri.conf.json` that is not JSON fails at line 1 with the parser's error.
    conf_invalid_json: Fails[fails(
        "crates/desktop/tauri.conf.json:1",
        "`crates/desktop/tauri.conf.json` is not valid JSON: EOF while parsing a value at line 1 \
         column 9",
        Fix::ConfInvalid,
    )], inspects 2 => super::repos::conf_invalid_json,
    /// `app.withGlobalTauri` set to true fails at its line.
    with_global_tauri_true: Fails[fails(
        "crates/desktop/tauri.conf.json:7",
        "app.withGlobalTauri must be false",
        Fix::GlobalTauri,
    )], inspects 2 => super::repos::with_global_tauri_true,
    /// `app.withGlobalTauri` left out fails at line 1.
    with_global_tauri_missing: Fails[fails(
        "crates/desktop/tauri.conf.json:1",
        "app.withGlobalTauri must be false",
        Fix::GlobalTauri,
    )], inspects 2 => super::repos::with_global_tauri_missing,
    /// `app.security.freezePrototype` set to false fails at its line.
    freeze_prototype_false: Fails[fails(
        "crates/desktop/tauri.conf.json:10",
        "app.security.freezePrototype must be true",
        Fix::FreezePrototype,
    )], inspects 2 => super::repos::freeze_prototype_false,
    /// `app.security.freezePrototype` left out fails at line 1.
    freeze_prototype_missing: Fails[fails(
        "crates/desktop/tauri.conf.json:1",
        "app.security.freezePrototype must be true",
        Fix::FreezePrototype,
    )], inspects 2 => super::repos::freeze_prototype_missing,
    /// A CSP that is a string but not the pinned one fails at the `csp` line.
    csp_not_pinned: Fails[fails(
        "crates/desktop/tauri.conf.json:12",
        "app.security.csp is not the pinned policy",
        Fix::CspPinned,
    )], inspects 2 => super::repos::csp_not_pinned,
    /// A CSP given as an object of directives fails: it must be the pinned string.
    csp_object: Fails[fails(
        "crates/desktop/tauri.conf.json:12",
        "app.security.csp is an object, not the pinned policy string",
        Fix::CspPinned,
    )], inspects 2 => super::repos::csp_object,
    /// A CSP that is neither a string nor an object fails as not a string.
    csp_not_string: Fails[fails(
        "crates/desktop/tauri.conf.json:12",
        "app.security.csp is not a string",
        Fix::CspPinned,
    )], inspects 2 => super::repos::csp_not_string,
    /// A CSP left out fails at line 1.
    csp_missing: Fails[fails(
        "crates/desktop/tauri.conf.json:1",
        "app.security.csp is missing",
        Fix::CspPinned,
    )], inspects 2 => super::repos::csp_missing,
    /// A `frontendDist` that is an https URL fails at its line.
    frontend_dist_url: Fails[fails(
        "crates/desktop/tauri.conf.json:4",
        "build.frontendDist is a URL",
        Fix::Frontend,
    )], inspects 2 => super::repos::frontend_dist_url,
    /// A `frontendDist` that is a list of files fails at its line.
    frontend_dist_list: Fails[fails(
        "crates/desktop/tauri.conf.json:4",
        "build.frontendDist is a list of files, not the dist folder",
        Fix::Frontend,
    )], inspects 2 => super::repos::frontend_dist_list,
    /// A config window in `app.windows` fails at the `windows` line.
    app_windows_config_window: Fails[fails(
        "crates/desktop/tauri.conf.json:8",
        "app.windows must be an empty array",
        Fix::Windows,
    )], inspects 2 => super::repos::app_windows_config_window,
    /// The asset protocol enabled fails at its `enable` line.
    asset_protocol_enabled: Fails[fails(
        "crates/desktop/tauri.conf.json:11",
        "app.security.assetProtocol.enable is true",
        Fix::AssetProtocol,
    )], inspects 2 => super::repos::asset_protocol_enabled,
    /// Updater endpoints with an empty public key fail at the `endpoints` line.
    updater_endpoints_without_pubkey: Fails[fails(
        "crates/desktop/tauri.conf.json:21",
        "plugins.updater has endpoints but no pubkey",
        Fix::Pubkey,
    )], inspects 2 => super::repos::updater_endpoints_without_pubkey,
    /// Updater endpoints with a whitespace-only public key fail at the `endpoints` line.
    whitespace_pubkey_with_endpoints: Fails[fails(
        "crates/desktop/tauri.conf.json:21",
        "plugins.updater has endpoints but no pubkey",
        Fix::Pubkey,
    )], inspects 2 => super::repos::whitespace_pubkey_with_endpoints,
    /// `bundle.createUpdaterArtifacts` set to `"v1Compatible"` fails at its line.
    updater_v1_compatible: Fails[fails(
        "crates/desktop/tauri.conf.json:17",
        "bundle.createUpdaterArtifacts is \"v1Compatible\"",
        Fix::V1Compatible,
    )], inspects 2 => super::repos::updater_v1_compatible,
    /// A `dangerous*` key nested under three objects fails under its full key path.
    dangerous_key_nested: Fails[fails(
        "crates/desktop/tauri.conf.json:11",
        "`app.security.assetProtocol.dangerousScope` switches off a Tauri safeguard",
        Fix::Dangerous,
    )], inspects 2 => super::repos::dangerous_key_nested,
    /// A `dangerous*` key in an object inside an array fails under its key path, at line 1.
    dangerous_key_in_an_array: Fails[fails(
        "crates/desktop/tauri.conf.json:1",
        "`app.security.capabilities.dangerousScope` switches off a Tauri safeguard",
        Fix::Dangerous,
    )], inspects 2 => super::repos::dangerous_key_in_an_array,
    /// A platform config `tauri.macos.conf.json` fails at line 1.
    platform_config_json: Fails[fails(
        "crates/desktop/tauri.macos.conf.json:1",
        "platform config `crates/desktop/tauri.macos.conf.json` overrides tauri.conf.json",
        Fix::Platform,
    )], inspects 3 => super::repos::platform_config_json,
    /// A platform config `tauri.linux.conf.json5` fails the same way.
    platform_config_json5: Fails[fails(
        "crates/desktop/tauri.linux.conf.json5:1",
        "platform config `crates/desktop/tauri.linux.conf.json5` overrides tauri.conf.json",
        Fix::Platform,
    )], inspects 3 => super::repos::platform_config_json5,
    /// A platform config `Tauri.windows.toml` fails the same way.
    platform_config_toml: Fails[fails(
        "crates/desktop/Tauri.windows.toml:1",
        "platform config `crates/desktop/Tauri.windows.toml` overrides tauri.conf.json",
        Fix::Platform,
    )], inspects 3 => super::repos::platform_config_toml,
    /// A missing `build.rs` fails as having no manifest, at line 1.
    build_rs_missing: Fails[fails(
        "crates/desktop/build.rs:1",
        "build.rs declares no AppManifest",
        Fix::AppManifest,
    )], inspects 1 => super::repos::build_rs_missing,
    /// A `build.rs` that calls no `app_manifest` fails at line 1.
    build_rs_without_app_manifest: Fails[fails(
        "crates/desktop/build.rs:1",
        "build.rs declares no AppManifest",
        Fix::AppManifest,
    )], inspects 2 => super::repos::build_rs_without_app_manifest,
    /// `AppManifest::new()` that is never passed to `app_manifest(` fails at line 1.
    app_manifest_new_not_passed: Fails[fails(
        "crates/desktop/build.rs:1",
        "build.rs declares no AppManifest",
        Fix::AppManifest,
    )], inspects 2 => super::repos::app_manifest_new_not_passed,
    /// `app_manifest(` without an `AppManifest::new()` fails at line 1.
    app_manifest_without_new: Fails[fails(
        "crates/desktop/build.rs:1",
        "build.rs declares no AppManifest",
        Fix::AppManifest,
    )], inspects 2 => super::repos::app_manifest_without_new,
    /// The manifest only inside a block comment does not count, so the file fails at line 1.
    app_manifest_only_in_block_comment: Fails[fails(
        "crates/desktop/build.rs:1",
        "build.rs declares no AppManifest",
        Fix::AppManifest,
    )], inspects 2 => super::repos::app_manifest_only_in_block_comment,
    /// The manifest only inside a line comment does not count, so the file fails at line 1.
    app_manifest_only_in_line_comment: Fails[fails(
        "crates/desktop/build.rs:1",
        "build.rs declares no AppManifest",
        Fix::AppManifest,
    )], inspects 2 => super::repos::app_manifest_only_in_line_comment,
    /// `AppManifest::new()` in a trailing comment does not count as a manifest.
    manifest_new_in_trailing_comment: Fails[fails(
        "crates/desktop/build.rs:1",
        "build.rs declares no AppManifest",
        Fix::AppManifest,
    )], inspects 2 => super::repos::manifest_new_in_trailing_comment,
    /// The manifest calls only inside a string literal do not count, so the file fails.
    app_manifest_in_string_only: Fails[fails(
        "crates/desktop/build.rs:1",
        "build.rs declares no AppManifest",
        Fix::AppManifest,
    )], inspects 2 => super::repos::app_manifest_in_string_only,
    /// A function named `app_manifest` is a definition, not a call, so the file fails at line 1.
    app_manifest_only_defined: Fails[fails(
        "crates/desktop/build.rs:1",
        "build.rs declares no AppManifest",
        Fix::AppManifest,
    )], inspects 2 => super::repos::app_manifest_only_defined,
    /// A `build.rs` that is not Rust fails at the line of the syntax error.
    build_rs_not_valid_rust: Fails[fails(
        "crates/desktop/build.rs:2",
        "build.rs is not valid Rust: expected an expression",
        Fix::BuildRsSyntax,
    )], inspects 2 => super::repos::build_rs_not_valid_rust,
    /// A `build.rs` that is not UTF-8 fails at line 1 as unreadable.
    build_rs_not_utf8: Fails[fails(
        "crates/desktop/build.rs:1",
        "could not read `crates/desktop/build.rs`: stream did not contain valid UTF-8",
        crate::checks::Fix::NotUtf8,
    )], inspects 1 => super::repos::build_rs_not_utf8,
    /// A crate that is not a workspace member stops the check: nothing is inspected.
    desktop_crate_missing: Fails[fails(
        "Cargo.toml:1",
        "the desktop crate (package `clauseo-desktop`) is not a workspace member",
        crate::checks::Fix::DesktopNotMember,
    )], inspects 0 => super::repos::desktop_crate_missing,
    /// Neither config file exists: the check looked and found none, so the zero line gets the
    /// normal fix, not the stopped one.
    both_config_files_missing: Fails[
        fails(
            "crates/desktop/tauri.conf.json:1",
            "`crates/desktop/tauri.conf.json` is missing",
            Fix::ConfMissing,
        ),
        fails(
            "crates/desktop/build.rs:1",
            "build.rs declares no AppManifest",
            Fix::AppManifest,
        ),
        failure(
            "0 config files inspected in the desktop crate (tauri.conf.json and build.rs)",
            crate::checks::Fix::NothingInspected {
                name: "tauri-config",
                many: "config files",
                place: "in the desktop crate (tauri.conf.json and build.rs)".to_owned(),
                source: "xtask/src/checks/tauri_config.rs",
            },
        ),
    ], inspects 0 => super::repos::both_config_files_missing,
}
