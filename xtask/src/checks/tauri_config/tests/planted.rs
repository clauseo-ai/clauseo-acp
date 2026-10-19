//! The planted cases of the tauri-config check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in
//! `tests/cases.rs`.

crate::checks::planted::cases! {
    /// A valid `tauri.conf.json` and `build.rs` pass.
    clean_tree: Passes => super::cases::clean_tree,
    /// `AppManifest::new().commands(...)` is still a manifest, so the file passes.
    app_manifest_with_commands_passes: Passes => super::cases::app_manifest_with_commands_passes,
    /// `app.security.devCsp` is not checked, so a relaxed dev policy passes.
    dev_csp_not_checked: Passes => super::cases::dev_csp_not_checked,
    /// Updater endpoints with a public key pass.
    endpoints_with_pubkey_passes: Passes => super::cases::endpoints_with_pubkey_passes,
    /// The asset protocol switched off passes.
    asset_protocol_disabled_passes: Passes => super::cases::asset_protocol_disabled_passes,
    /// A `capabilities_path_pattern(` call in a line comment does not count.
    path_pattern_in_comment_passes: Passes => super::cases::path_pattern_in_comment_passes,
    /// A function named `capabilities_path_pattern` is a definition, not a call.
    fn_named_like_the_call_passes: Passes => super::cases::fn_named_like_the_call_passes,
    /// A missing `tauri.conf.json` fails at line 1.
    conf_missing: FailsWith(&[
        "crates/desktop/tauri.conf.json:1: `crates/desktop/tauri.conf.json` is missing\n    \
         fix: restore it: Tauri reads the window's security settings from it, and this check \
         guards them",
    ]) => super::cases::conf_missing,
    /// A `tauri.conf.json` that is not UTF-8 fails at line 1 as unreadable.
    conf_not_utf8: FailsWith(&[
        "crates/desktop/tauri.conf.json:1: could not read `crates/desktop/tauri.conf.json`: \
         stream did not contain valid UTF-8\n    \
         fix: make the file readable as UTF-8 text",
    ]) => super::cases::conf_not_utf8,
    /// A `tauri.conf.json` that is not JSON fails at line 1 with the parser's error.
    conf_invalid_json: FailsWith(&[
        "crates/desktop/tauri.conf.json:1: `crates/desktop/tauri.conf.json` is not valid JSON: \
         EOF while parsing a value at line 1 column 9\n    \
         fix: fix the JSON syntax: Tauri cannot read the file either",
    ]) => super::cases::conf_invalid_json,
    /// `app.withGlobalTauri` set to true fails at its line.
    with_global_tauri_true: FailsWith(&[
        "crates/desktop/tauri.conf.json:7: app.withGlobalTauri must be false\n    \
         fix: set `\"withGlobalTauri\": false` under `app`: the global object hands every script \
         in the page the whole IPC API",
    ]) => super::cases::with_global_tauri_true,
    /// `app.withGlobalTauri` left out fails at line 1.
    with_global_tauri_missing: FailsWith(&[
        "crates/desktop/tauri.conf.json:1: app.withGlobalTauri must be false\n    \
         fix: set `\"withGlobalTauri\": false` under `app`: the global object hands every script \
         in the page the whole IPC API",
    ]) => super::cases::with_global_tauri_missing,
    /// `app.security.freezePrototype` set to false fails at its line.
    freeze_prototype_false: FailsWith(&[
        "crates/desktop/tauri.conf.json:10: app.security.freezePrototype must be true\n    \
         fix: set `\"freezePrototype\": true` under app.security: a frozen prototype stops script \
         in the page from hijacking the objects the IPC bridge uses",
    ]) => super::cases::freeze_prototype_false,
    /// A CSP that is a string but not the pinned one fails at the `csp` line.
    csp_not_pinned: FailsWith(&[
        "crates/desktop/tauri.conf.json:12: app.security.csp is not the pinned policy\n    \
         fix: set app.security.csp to the string in PINNED_CSP: the policy is pinned in \
         xtask/src/checks/tauri_config.rs and changes only together with that const, in review",
    ]) => super::cases::csp_not_pinned,
    /// A CSP given as an object of directives fails: it must be the pinned string.
    csp_object: FailsWith(&[
        "crates/desktop/tauri.conf.json:12: app.security.csp is an object, not the pinned policy \
         string\n    \
         fix: set app.security.csp to the string in PINNED_CSP: the policy is pinned in \
         xtask/src/checks/tauri_config.rs and changes only together with that const, in review",
    ]) => super::cases::csp_object,
    /// A CSP that is neither a string nor an object fails as not a string.
    csp_not_string: FailsWith(&[
        "crates/desktop/tauri.conf.json:12: app.security.csp is not a string\n    \
         fix: set app.security.csp to the string in PINNED_CSP: the policy is pinned in \
         xtask/src/checks/tauri_config.rs and changes only together with that const, in review",
    ]) => super::cases::csp_not_string,
    /// A CSP left out fails at line 1.
    csp_missing: FailsWith(&[
        "crates/desktop/tauri.conf.json:1: app.security.csp is missing\n    \
         fix: set app.security.csp to the string in PINNED_CSP: the policy is pinned in \
         xtask/src/checks/tauri_config.rs and changes only together with that const, in review",
    ]) => super::cases::csp_missing,
    /// A `frontendDist` that is an https URL fails at its line.
    frontend_dist_url: FailsWith(&[
        "crates/desktop/tauri.conf.json:4: build.frontendDist is a URL\n    \
         fix: frontendDist must be the local dist folder; a URL makes the release app load a \
         remote site",
    ]) => super::cases::frontend_dist_url,
    /// A `frontendDist` that is a list of files fails at its line.
    frontend_dist_list: FailsWith(&[
        "crates/desktop/tauri.conf.json:4: build.frontendDist is a list of files, not the dist \
         folder\n    \
         fix: frontendDist must be the local dist folder; a URL makes the release app load a \
         remote site",
    ]) => super::cases::frontend_dist_list,
    /// A config window in `app.windows` fails at the `windows` line.
    app_windows_config_window: FailsWith(&[
        "crates/desktop/tauri.conf.json:8: app.windows must be an empty array\n    \
         fix: build windows in crates/desktop/src/lib.rs: a config window skips \
         use_https_scheme(true)",
    ]) => super::cases::app_windows_config_window,
    /// The asset protocol enabled fails at its `enable` line.
    asset_protocol_enabled: FailsWith(&[
        "crates/desktop/tauri.conf.json:11: app.security.assetProtocol.enable is true\n    \
         fix: set app.security.assetProtocol.enable to false: the asset protocol serves files \
         from disk to the webview",
    ]) => super::cases::asset_protocol_enabled,
    /// Updater endpoints with an empty public key fail at the `endpoints` line.
    updater_endpoints_without_pubkey: FailsWith(&[
        "crates/desktop/tauri.conf.json:21: plugins.updater has endpoints but no pubkey\n    \
         fix: set plugins.updater.pubkey to the release public key before adding endpoints",
    ]) => super::cases::updater_endpoints_without_pubkey,
    /// `bundle.createUpdaterArtifacts` set to `"v1Compatible"` fails at its line.
    updater_v1_compatible: FailsWith(&[
        "crates/desktop/tauri.conf.json:17: bundle.createUpdaterArtifacts is \"v1Compatible\"\n    \
         fix: the updater is built without its zip feature, so v1Compatible artifacts cannot be \
         installed; use true",
    ]) => super::cases::updater_v1_compatible,
    /// A `dangerous*` key nested under three objects fails under its full key path.
    dangerous_key_nested: FailsWith(&[
        "crates/desktop/tauri.conf.json:11: `app.security.assetProtocol.dangerousScope` switches \
         off a Tauri safeguard\n    \
         fix: remove it: each `dangerous*` setting switches off a Tauri safeguard",
    ]) => super::cases::dangerous_key_nested,
    /// A platform config `tauri.macos.conf.json` fails at line 1.
    platform_config_json: FailsWith(&[
        "crates/desktop/tauri.macos.conf.json:1: platform config \
         `crates/desktop/tauri.macos.conf.json` overrides tauri.conf.json\n    \
         fix: put the setting in tauri.conf.json: a platform config overrides the checked settings \
         on one OS",
    ]) => super::cases::platform_config_json,
    /// A platform config `tauri.linux.conf.json5` fails the same way.
    platform_config_json5: FailsWith(&[
        "crates/desktop/tauri.linux.conf.json5:1: platform config \
         `crates/desktop/tauri.linux.conf.json5` overrides tauri.conf.json\n    \
         fix: put the setting in tauri.conf.json: a platform config overrides the checked settings \
         on one OS",
    ]) => super::cases::platform_config_json5,
    /// A platform config `Tauri.windows.toml` fails the same way.
    platform_config_toml: FailsWith(&[
        "crates/desktop/Tauri.windows.toml:1: platform config `crates/desktop/Tauri.windows.toml` \
         overrides tauri.conf.json\n    \
         fix: put the setting in tauri.conf.json: a platform config overrides the checked settings \
         on one OS",
    ]) => super::cases::platform_config_toml,
    /// A missing `build.rs` fails as having no manifest, at line 1.
    build_rs_missing: FailsWith(&[
        "crates/desktop/build.rs:1: build.rs declares no AppManifest\n    \
         fix: pass `tauri_build::AppManifest::new()` to \
         `tauri_build::Attributes::new().app_manifest(...)` in `tauri_build::try_build`: without a \
         manifest Tauri skips the access check for app commands",
    ]) => super::cases::build_rs_missing,
    /// A `build.rs` that calls no `app_manifest` fails at line 1.
    build_rs_without_app_manifest: FailsWith(&[
        "crates/desktop/build.rs:1: build.rs declares no AppManifest\n    \
         fix: pass `tauri_build::AppManifest::new()` to \
         `tauri_build::Attributes::new().app_manifest(...)` in `tauri_build::try_build`: without a \
         manifest Tauri skips the access check for app commands",
    ]) => super::cases::build_rs_without_app_manifest,
    /// `AppManifest::new()` that is never passed to `app_manifest(` fails at line 1.
    app_manifest_new_not_passed: FailsWith(&[
        "crates/desktop/build.rs:1: build.rs declares no AppManifest\n    \
         fix: pass `tauri_build::AppManifest::new()` to \
         `tauri_build::Attributes::new().app_manifest(...)` in `tauri_build::try_build`: without a \
         manifest Tauri skips the access check for app commands",
    ]) => super::cases::app_manifest_new_not_passed,
    /// `app_manifest(` without an `AppManifest::new()` fails at line 1.
    app_manifest_without_new: FailsWith(&[
        "crates/desktop/build.rs:1: build.rs declares no AppManifest\n    \
         fix: pass `tauri_build::AppManifest::new()` to \
         `tauri_build::Attributes::new().app_manifest(...)` in `tauri_build::try_build`: without a \
         manifest Tauri skips the access check for app commands",
    ]) => super::cases::app_manifest_without_new,
    /// The manifest only inside a block comment does not count, so the file fails at line 1.
    app_manifest_only_in_block_comment: FailsWith(&[
        "crates/desktop/build.rs:1: build.rs declares no AppManifest\n    \
         fix: pass `tauri_build::AppManifest::new()` to \
         `tauri_build::Attributes::new().app_manifest(...)` in `tauri_build::try_build`: without a \
         manifest Tauri skips the access check for app commands",
    ]) => super::cases::app_manifest_only_in_block_comment,
    /// The manifest only inside a line comment does not count, so the file fails at line 1.
    app_manifest_only_in_line_comment: FailsWith(&[
        "crates/desktop/build.rs:1: build.rs declares no AppManifest\n    \
         fix: pass `tauri_build::AppManifest::new()` to \
         `tauri_build::Attributes::new().app_manifest(...)` in `tauri_build::try_build`: without a \
         manifest Tauri skips the access check for app commands",
    ]) => super::cases::app_manifest_only_in_line_comment,
    /// `AppManifest::new()` in a trailing comment does not count as a manifest.
    manifest_new_in_trailing_comment: FailsWith(&[
        "crates/desktop/build.rs:1: build.rs declares no AppManifest\n    \
         fix: pass `tauri_build::AppManifest::new()` to \
         `tauri_build::Attributes::new().app_manifest(...)` in `tauri_build::try_build`: without a \
         manifest Tauri skips the access check for app commands",
    ]) => super::cases::manifest_new_in_trailing_comment,
    /// The manifest calls only inside a string literal do not count, so the file fails.
    app_manifest_in_string_only: FailsWith(&[
        "crates/desktop/build.rs:1: build.rs declares no AppManifest\n    \
         fix: pass `tauri_build::AppManifest::new()` to \
         `tauri_build::Attributes::new().app_manifest(...)` in `tauri_build::try_build`: without a \
         manifest Tauri skips the access check for app commands",
    ]) => super::cases::app_manifest_in_string_only,
    /// A `build.rs` that is not Rust fails at the line of the syntax error.
    build_rs_not_valid_rust: FailsWith(&[
        "crates/desktop/build.rs:2: build.rs is not valid Rust: expected an expression\n    \
         fix: fix the syntax: cargo cannot build the crate either",
    ]) => super::cases::build_rs_not_valid_rust,
    /// A `build.rs` that is not UTF-8 fails at line 1 as unreadable.
    build_rs_not_utf8: FailsWith(&[
        "crates/desktop/build.rs:1: could not read `crates/desktop/build.rs`: stream did not \
         contain valid UTF-8\n    \
         fix: make the file readable as UTF-8 text",
    ]) => super::cases::build_rs_not_utf8,
    /// A `.capabilities_path_pattern(` call fails at its line.
    build_rs_calls_path_pattern: FailsWith(&[
        "crates/desktop/build.rs:5: build.rs calls `capabilities_path_pattern(`\n    \
         fix: remove it: capabilities live in the crate's capabilities folder, the one the \
         capabilities check scans",
    ]) => super::cases::build_rs_calls_path_pattern,
    /// The call after a string that holds `//` on the same line fails at its line.
    path_pattern_after_slashes_string: FailsWith(&[
        "crates/desktop/build.rs:5: build.rs calls `capabilities_path_pattern(`\n    \
         fix: remove it: capabilities live in the crate's capabilities folder, the one the \
         capabilities check scans",
    ]) => super::cases::path_pattern_after_slashes_string,
    /// The call inside a macro's arguments fails at its line.
    path_pattern_in_macro: FailsWith(&[
        "crates/desktop/build.rs:3: build.rs calls `capabilities_path_pattern(`\n    \
         fix: remove it: capabilities live in the crate's capabilities folder, the one the \
         capabilities check scans",
    ]) => super::cases::path_pattern_in_macro,
    /// The raw identifier `r#capabilities_path_pattern(` fails at its line.
    path_pattern_raw_ident: FailsWith(&[
        "crates/desktop/build.rs:3: build.rs calls `capabilities_path_pattern(`\n    \
         fix: remove it: capabilities live in the crate's capabilities folder, the one the \
         capabilities check scans",
    ]) => super::cases::path_pattern_raw_ident,
    /// A crate that is not a workspace member stops the check: nothing is inspected.
    desktop_crate_missing: FailsWith(&[
        "Cargo.toml:1: the desktop crate (package `clauseo-desktop`) is not a workspace \
         member\n    \
         fix: restore crates/desktop or update xtask/src/checks: these checks guard its \
         capabilities, plugins and Tauri config",
        "0 config files inspected in the desktop crate (tauri.conf.json and build.rs)\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]) => super::cases::desktop_crate_missing,
    /// Neither config file exists: the check looked and found none, so the zero line gets the
    /// normal fix, not the stopped one.
    both_config_files_missing: FailsWith(&[
        "crates/desktop/tauri.conf.json:1: `crates/desktop/tauri.conf.json` is missing\n    \
         fix: restore it: Tauri reads the window's security settings from it, and this check \
         guards them",
        "crates/desktop/build.rs:1: build.rs declares no AppManifest\n    \
         fix: pass `tauri_build::AppManifest::new()` to \
         `tauri_build::Attributes::new().app_manifest(...)` in `tauri_build::try_build`: without a \
         manifest Tauri skips the access check for app commands",
        "0 config files inspected in the desktop crate (tauri.conf.json and build.rs)\n    \
         fix: tauri-config looks for config files in the desktop crate (tauri.conf.json and \
         build.rs); restore them, or, if they moved, update the consts \
         xtask/src/checks/tauri_config.rs builds that place from",
    ]) => super::cases::both_config_files_missing,
}
