//! The builders of the tauri-config planted cases. Each writes a desktop crate into a scratch repo
//! and runs the check's own `run` on it: a valid `tauri.conf.json` (`CONF`) and `build.rs`
//! (`BUILD`), with one thing changed.

use super::*;
use crate::checks::planted::Repo;
use crate::scratch::Scratch;

/// A scratch repo whose desktop crate holds `files`, each a name and its bytes.
fn run_files(case: &str, files: &[(&str, &[u8])]) -> Repo {
    let dir = Scratch::new(&format!("planted-tauri-config-{case}"));
    Scratch::create_dir_all(dir.join("crates/desktop")).expect("create the desktop dir");
    for (name, bytes) in files {
        Scratch::write(dir.join("crates/desktop").join(name), bytes).expect("write a desktop file");
    }
    Repo::desktop(dir)
}

/// `CONF` with `from` replaced by `to`, once.
fn edited_conf(from: &str, to: &str) -> String {
    assert!(CONF.contains(from), "the fixture contains {from}");
    CONF.replacen(from, to, 1)
}

/// A desktop crate whose `tauri.conf.json` is `CONF` with `from` replaced by `to`, and whose
/// `build.rs` is valid.
fn conf_case(case: &str, from: &str, to: &str) -> Repo {
    let conf = edited_conf(from, to);
    run_files(
        case,
        &[(TAURI_CONF, conf.as_bytes()), (BUILD_RS, BUILD.as_bytes())],
    )
}

/// A desktop crate whose `build.rs` is `build` and whose `tauri.conf.json` is valid.
fn build_case(case: &str, build: &str) -> Repo {
    run_files(
        case,
        &[(TAURI_CONF, CONF.as_bytes()), (BUILD_RS, build.as_bytes())],
    )
}

/// A desktop crate whose `build.rs` is valid and whose `tauri.conf.json` is `conf`.
fn raw_conf_case(case: &str, conf: &[u8]) -> Repo {
    run_files(case, &[(TAURI_CONF, conf), (BUILD_RS, BUILD.as_bytes())])
}

/// The valid desktop crate with a platform config `name` next to its valid files.
fn platform_case(case: &str, name: &str, text: &str) -> Repo {
    run_files(
        case,
        &[
            (TAURI_CONF, CONF.as_bytes()),
            (BUILD_RS, BUILD.as_bytes()),
            (name, text.as_bytes()),
        ],
    )
}

pub(super) fn clean_tree() -> Repo {
    run_files(
        "clean-tree",
        &[(TAURI_CONF, CONF.as_bytes()), (BUILD_RS, BUILD.as_bytes())],
    )
}

pub(super) fn app_manifest_with_commands_passes() -> Repo {
    build_case(
        "app-manifest-with-commands",
        &BUILD.replace(
            "AppManifest::new()",
            "AppManifest::new().commands(&[\"open_matter\"])",
        ),
    )
}

pub(super) fn dev_csp_not_checked() -> Repo {
    conf_case(
        "dev-csp-not-checked",
        &csp_entry(),
        &format!(
            "{},\n      \"devCsp\": \"script-src 'unsafe-eval'\"",
            csp_entry()
        ),
    )
}

pub(super) fn endpoints_with_pubkey_passes() -> Repo {
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

pub(super) fn updater_artifacts_true_passes() -> Repo {
    conf_case(
        "updater-artifacts-true",
        "\"icon\": []",
        "\"icon\": [],\n    \"createUpdaterArtifacts\": true",
    )
}

pub(super) fn asset_protocol_disabled_passes() -> Repo {
    conf_case(
        "asset-protocol-disabled",
        "\"freezePrototype\": true,",
        "\"freezePrototype\": true,\n      \"assetProtocol\": { \"enable\": false },",
    )
}

pub(super) fn conf_missing() -> Repo {
    run_files("conf-missing", &[(BUILD_RS, BUILD.as_bytes())])
}

pub(super) fn conf_not_utf8() -> Repo {
    raw_conf_case("conf-not-utf8", &[0xff, 0xfe, b'{'])
}

pub(super) fn conf_invalid_json() -> Repo {
    raw_conf_case("conf-invalid-json", b"{ \"app\": ")
}

pub(super) fn with_global_tauri_true() -> Repo {
    conf_case(
        "with-global-tauri-true",
        "\"withGlobalTauri\": false",
        "\"withGlobalTauri\": true",
    )
}

pub(super) fn with_global_tauri_missing() -> Repo {
    conf_case(
        "with-global-tauri-missing",
        "\"withGlobalTauri\": false,",
        "",
    )
}

pub(super) fn freeze_prototype_false() -> Repo {
    conf_case(
        "freeze-prototype-false",
        "\"freezePrototype\": true",
        "\"freezePrototype\": false",
    )
}

pub(super) fn freeze_prototype_missing() -> Repo {
    conf_case("freeze-prototype-missing", "\"freezePrototype\": true,", "")
}

pub(super) fn csp_not_pinned() -> Repo {
    let trimmed = PINNED_CSP.replace("; require-trusted-types-for 'script'", "");
    conf_case(
        "csp-not-pinned",
        &csp_entry(),
        &format!("\"csp\": \"{trimmed}\""),
    )
}

pub(super) fn csp_object() -> Repo {
    conf_case(
        "csp-object",
        &csp_entry(),
        "\"csp\": { \"default-src\": \"'self'\", \"script-src\": [\"'self'\"] }",
    )
}

pub(super) fn csp_not_string() -> Repo {
    conf_case("csp-not-string", &csp_entry(), "\"csp\": true")
}

pub(super) fn csp_missing() -> Repo {
    conf_case(
        "csp-missing",
        &format!("[\"main\"],\n      {}", csp_entry()),
        "[\"main\"]",
    )
}

pub(super) fn frontend_dist_url() -> Repo {
    conf_case(
        "frontend-dist-url",
        "\"frontendDist\": \"../../ui/app/dist\"",
        "\"frontendDist\": \"https://evil.example\"",
    )
}

pub(super) fn frontend_dist_list() -> Repo {
    conf_case(
        "frontend-dist-list",
        "\"frontendDist\": \"../../ui/app/dist\"",
        "\"frontendDist\": [\"index.html\"]",
    )
}

pub(super) fn app_windows_config_window() -> Repo {
    conf_case(
        "app-windows-config-window",
        "\"windows\": []",
        "\"windows\": [{ \"label\": \"main\" }]",
    )
}

pub(super) fn asset_protocol_enabled() -> Repo {
    conf_case(
        "asset-protocol-enabled",
        "\"freezePrototype\": true,",
        "\"freezePrototype\": true,\n      \"assetProtocol\": { \"enable\": true },",
    )
}

pub(super) fn updater_endpoints_without_pubkey() -> Repo {
    conf_case(
        "updater-endpoints-without-pubkey",
        "\"endpoints\": []",
        "\"endpoints\": [\"https://releases.example/latest.json\"]",
    )
}

pub(super) fn whitespace_pubkey_with_endpoints() -> Repo {
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

pub(super) fn updater_v1_compatible() -> Repo {
    conf_case(
        "updater-v1-compatible",
        "\"icon\": []",
        "\"icon\": [],\n    \"createUpdaterArtifacts\": \"v1Compatible\"",
    )
}

pub(super) fn dangerous_key_nested() -> Repo {
    conf_case(
        "dangerous-key-nested",
        "\"freezePrototype\": true,",
        "\"freezePrototype\": true,\n      \"assetProtocol\": { \"enable\": false, \"dangerousScope\": true },",
    )
}

pub(super) fn dangerous_key_in_an_array() -> Repo {
    conf_case(
        "dangerous-key-in-an-array",
        "\"capabilities\": [\"main\"]",
        "\"capabilities\": [\"main\", { \"identifier\": \"ipc\", \"dangerousScope\": true }]",
    )
}

pub(super) fn platform_config_json() -> Repo {
    platform_case(
        "platform-config-json",
        "tauri.macos.conf.json",
        "{\"app\":{\"withGlobalTauri\":true}}",
    )
}

pub(super) fn platform_config_json5() -> Repo {
    platform_case(
        "platform-config-json5",
        "tauri.linux.conf.json5",
        "{ app: { withGlobalTauri: true } }",
    )
}

pub(super) fn platform_config_toml() -> Repo {
    platform_case(
        "platform-config-toml",
        "Tauri.windows.toml",
        "[app]\nwithGlobalTauri = true\n",
    )
}

pub(super) fn build_rs_missing() -> Repo {
    run_files("build-rs-missing", &[(TAURI_CONF, CONF.as_bytes())])
}

pub(super) fn build_rs_without_app_manifest() -> Repo {
    build_case(
        "build-rs-without-app-manifest",
        "fn main() {\n    tauri_build::build();\n}\n",
    )
}

pub(super) fn app_manifest_new_not_passed() -> Repo {
    build_case(
        "app-manifest-new-not-passed",
        "fn main() {\n    let _ = tauri_build::AppManifest::new();\n    tauri_build::build();\n}\n",
    )
}

pub(super) fn app_manifest_without_new() -> Repo {
    build_case(
        "app-manifest-without-new",
        "fn main() {\n    tauri_build::try_build(\n        tauri_build::Attributes::new().app_manifest(manifest()),\n    );\n}\n",
    )
}

pub(super) fn app_manifest_only_in_block_comment() -> Repo {
    build_case(
        "app-manifest-only-in-block-comment",
        "/*\n    tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new())\n*/\nfn main() {\n    tauri_build::build();\n}\n",
    )
}

pub(super) fn app_manifest_only_in_line_comment() -> Repo {
    build_case(
        "app-manifest-only-in-line-comment",
        "// tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new());\nfn main() {\n    tauri_build::build();\n}\n",
    )
}

pub(super) fn manifest_new_in_trailing_comment() -> Repo {
    build_case(
        "manifest-new-in-trailing-comment",
        "fn main() {\n    let a = tauri_build::Attributes::new().app_manifest(manifest()); // tauri_build::AppManifest::new()\n}\n",
    )
}

pub(super) fn app_manifest_in_string_only() -> Repo {
    build_case(
        "app-manifest-in-string-only",
        r#"fn main() {
    let note = "tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new())";
    tauri_build::build();
}
"#,
    )
}

pub(super) fn app_manifest_only_defined() -> Repo {
    build_case(
        "app-manifest-only-defined",
        "fn main() {\n    let _ = tauri_build::AppManifest::new();\n    tauri_build::build();\n}\nfn app_manifest(_manifest: tauri_build::AppManifest) {}\n",
    )
}

pub(super) fn build_rs_not_valid_rust() -> Repo {
    build_case("build-rs-not-valid-rust", "fn main() {\n    let x = ;\n}\n")
}

pub(super) fn build_rs_not_utf8() -> Repo {
    run_files(
        "build-rs-not-utf8",
        &[
            (TAURI_CONF, CONF.as_bytes()),
            (BUILD_RS, &[0xff, 0xfe, b'f']),
        ],
    )
}

pub(super) fn desktop_crate_missing() -> Repo {
    let dir = Scratch::new("planted-tauri-config-desktop-crate-missing");
    Repo::new(dir)
}

/// The desktop crate with neither tauri.conf.json nor build.rs: the check looks and finds none.
pub(super) fn both_config_files_missing() -> Repo {
    run_files("both-config-files-missing", &[])
}
pub(super) fn platform_config_android() -> Repo {
    platform_case(
        "platform-config-android",
        "tauri.android.conf.json",
        "{\"app\":{\"withGlobalTauri\":true}}",
    )
}

pub(super) fn platform_config_ios_toml() -> Repo {
    platform_case(
        "platform-config-ios-toml",
        "Tauri.ios.toml",
        "[app]\nwithGlobalTauri = true\n",
    )
}

/// A platform config named in capitals: a macOS or Windows disk hands it to Tauri all the same.
pub(super) fn platform_config_other_case() -> Repo {
    platform_case(
        "platform-config-other-case",
        "TAURI.MACOS.CONF.JSON",
        "{\"app\":{\"withGlobalTauri\":true}}",
    )
}

/// `tauri.conf.json5` as the only main config: Tauri reads it when `tauri.conf.json` is missing.
pub(super) fn main_config_json5_only() -> Repo {
    run_files(
        "main-config-json5-only",
        &[
            ("tauri.conf.json5", b"{ app: { withGlobalTauri: true } }"),
            (BUILD_RS, BUILD.as_bytes()),
        ],
    )
}

/// `Tauri.toml` as the only main config: Tauri reads it when `tauri.conf.json` is missing.
pub(super) fn main_config_toml_only() -> Repo {
    run_files(
        "main-config-toml-only",
        &[
            ("Tauri.toml", b"[app]\nwithGlobalTauri = true\n"),
            (BUILD_RS, BUILD.as_bytes()),
        ],
    )
}

/// `withGlobalTauri` written twice, the safe value first: the last one wins, as in Tauri.
pub(super) fn duplicate_key_unsafe_last() -> Repo {
    conf_case(
        "duplicate-key-unsafe-last",
        "\"withGlobalTauri\": false,",
        "\"withGlobalTauri\": false,\n    \"withGlobalTauri\": true,",
    )
}

/// `Attributes::config_path` points tauri-build at another config, on line 3.
pub(super) fn build_rs_config_path() -> Repo {
    build_case(
        "build-rs-config-path",
        &BUILD.replace(
            "tauri_build::Attributes::new()",
            "tauri_build::Attributes::new().config_path(\"other.conf.json\")",
        ),
    )
}

/// `build.rs` sets `TAURI_CONFIG`, which tauri-build merges over `tauri.conf.json`, on line 2.
pub(super) fn build_rs_sets_env_config() -> Repo {
    build_case(
        "build-rs-sets-env-config",
        &BUILD.replace(
            "fn main() {\n",
            "fn main() {\n    unsafe { std::env::set_var(\"TAURI_CONFIG\", \"{}\") };\n",
        ),
    )
}

/// `build.rs` hands `TAURI_CONFIG` to rustc, so `generate_context!` merges it, on line 2.
pub(super) fn build_rs_rustc_env_config() -> Repo {
    build_case(
        "build-rs-rustc-env-config",
        &BUILD.replace(
            "fn main() {\n",
            "fn main() {\n    println!(\"cargo:rustc-env=TAURI_CONFIG={{}}\");\n",
        ),
    )
}

/// A scratch repo with the valid `tauri.conf.json` and `build.rs` in its desktop crate and each
/// `(path, text)` of `files`, paths from the repo root.
pub(super) fn tree_case(case: &str, files: &[(&str, &str)]) -> Repo {
    let dir = Scratch::new(&format!("planted-tauri-config-{case}"));
    let desktop = [
        ("crates/desktop/tauri.conf.json", CONF),
        ("crates/desktop/build.rs", BUILD),
    ];
    for (path, text) in desktop.iter().chain(files) {
        let full = dir.join(path);
        Scratch::create_dir_all(full.parent().expect("a file has a parent")).expect("create dirs");
        Scratch::write(full, text).expect("write a file");
    }
    Repo::desktop(dir)
}

/// `generate_context!("other.json")` on line 2 of `src/lib.rs`.
pub(super) fn generate_context_with_a_config_path() -> Repo {
    tree_case(
        "context-config-path",
        &[(
            "crates/desktop/src/lib.rs",
            "pub fn run() {\n    let _context = tauri::generate_context!(\"other.json\");\n}\n",
        )],
    )
}

/// `src/ctx.rs` holds only an expression, as `include!` takes it: braced `generate_context!`
/// with `capabilities` on line 2. It is no whole Rust file, yet its tokens are read.
pub(super) fn generate_context_in_an_included_fragment() -> Repo {
    tree_case(
        "context-fragment",
        &[(
            "crates/desktop/src/ctx.rs",
            "// pulled in with include!\ntauri::generate_context! { capabilities = [\"x.json\"] }\n",
        )],
    )
}

/// `generate_context` imported as `ctx` on line 1 of a nested module file.
pub(super) fn generate_context_renamed() -> Repo {
    tree_case(
        "context-renamed",
        &[(
            "crates/desktop/src/app/mod.rs",
            "use tauri::generate_context as ctx;\n\npub fn run() {\n    let _c = ctx!(\"o.json\");\n}\n",
        )],
    )
}

/// `generate_context!()` with no argument, beside a doc comment and a string that show it with
/// one: the two Rust files are read and pass.
pub(super) fn generate_context_without_arguments_passes() -> Repo {
    tree_case(
        "context-clean",
        &[
            (
                "crates/desktop/src/lib.rs",
                "/// Not `generate_context!(\"other.json\")`.\npub fn run() {\n    \
                 let _c = tauri::generate_context!();\n    \
                 let _s = \"generate_context!(\\\"x\\\")\";\n}\n",
            ),
            (
                "crates/desktop/src/main.rs",
                "fn main() {\n    app::run();\n}\n",
            ),
        ],
    )
}

/// A Rust file of the desktop crate whose string never ends, so it does not lex.
pub(super) fn source_that_does_not_lex() -> Repo {
    tree_case(
        "source-syntax",
        &[(
            "crates/desktop/src/lib.rs",
            "pub fn run() {\n    \"open\n}\n",
        )],
    )
}

/// `.cargo/config.toml` sets `tauri_config` (lower case) in `[env]` on line 4.
pub(super) fn tauri_config_in_cargo_env() -> Repo {
    tree_case(
        "cargo-env",
        &[(
            ".cargo/config.toml",
            "[alias]\nx = \"run\"\n\n[env]\ntauri_config = { value = \"{}\", force = true }\n",
        )],
    )
}

/// `.cargo/config.toml` with another `[env]` entry and a dotted `env.TAURI_CONFIG_X`: the
/// file is read and passes.
pub(super) fn other_cargo_env_passes() -> Repo {
    tree_case(
        "cargo-env-clean",
        &[(
            ".cargo/config.toml",
            "env.TAURI_CONFIG_X = \"1\"\nenv.OTHER = \"{}\"\n",
        )],
    )
}
