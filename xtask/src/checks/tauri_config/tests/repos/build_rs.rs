//! The tauri-config builders for `build.rs`: the app manifest it must hand tauri-build, and the configs it must not point at.

use super::*;

pub(in crate::checks::tauri_config::tests) fn app_manifest_with_commands_passes() -> Repo {
    build_case(
        "app-manifest-with-commands",
        &BUILD.replace(
            "AppManifest::new()",
            "AppManifest::new().commands(&[\"open_matter\"])",
        ),
    )
}

pub(in crate::checks::tauri_config::tests) fn build_rs_missing() -> Repo {
    run_files("build-rs-missing", &[(TAURI_CONF, CONF.as_bytes())])
}

pub(in crate::checks::tauri_config::tests) fn build_rs_without_app_manifest() -> Repo {
    build_case(
        "build-rs-without-app-manifest",
        "fn main() {\n    tauri_build::build();\n}\n",
    )
}

pub(in crate::checks::tauri_config::tests) fn app_manifest_new_not_passed() -> Repo {
    build_case(
        "app-manifest-new-not-passed",
        "fn main() {\n    let _ = tauri_build::AppManifest::new();\n    tauri_build::build();\n}\n",
    )
}

pub(in crate::checks::tauri_config::tests) fn app_manifest_without_new() -> Repo {
    build_case(
        "app-manifest-without-new",
        "fn main() {\n    tauri_build::try_build(\n        tauri_build::Attributes::new().app_manifest(manifest()),\n    );\n}\n",
    )
}

pub(in crate::checks::tauri_config::tests) fn app_manifest_only_in_block_comment() -> Repo {
    build_case(
        "app-manifest-only-in-block-comment",
        "/*\n    tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new())\n*/\nfn main() {\n    tauri_build::build();\n}\n",
    )
}

pub(in crate::checks::tauri_config::tests) fn app_manifest_only_in_line_comment() -> Repo {
    build_case(
        "app-manifest-only-in-line-comment",
        "// tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new());\nfn main() {\n    tauri_build::build();\n}\n",
    )
}

pub(in crate::checks::tauri_config::tests) fn manifest_new_in_trailing_comment() -> Repo {
    build_case(
        "manifest-new-in-trailing-comment",
        "fn main() {\n    let a = tauri_build::Attributes::new().app_manifest(manifest()); // tauri_build::AppManifest::new()\n}\n",
    )
}

pub(in crate::checks::tauri_config::tests) fn app_manifest_in_string_only() -> Repo {
    build_case(
        "app-manifest-in-string-only",
        r#"fn main() {
    let note = "tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new())";
    tauri_build::build();
}
"#,
    )
}

pub(in crate::checks::tauri_config::tests) fn app_manifest_only_defined() -> Repo {
    build_case(
        "app-manifest-only-defined",
        "fn main() {\n    let _ = tauri_build::AppManifest::new();\n    tauri_build::build();\n}\nfn app_manifest(_manifest: tauri_build::AppManifest) {}\n",
    )
}

pub(in crate::checks::tauri_config::tests) fn build_rs_not_valid_rust() -> Repo {
    build_case("build-rs-not-valid-rust", "fn main() {\n    let x = ;\n}\n")
}

pub(in crate::checks::tauri_config::tests) fn build_rs_not_utf8() -> Repo {
    run_files(
        "build-rs-not-utf8",
        &[
            (TAURI_CONF, CONF.as_bytes()),
            (BUILD_RS, &[0xff, 0xfe, b'f']),
        ],
    )
}

/// `Attributes::config_path` points tauri-build at another config, on line 3.
pub(in crate::checks::tauri_config::tests) fn build_rs_config_path() -> Repo {
    build_case(
        "build-rs-config-path",
        &BUILD.replace(
            "tauri_build::Attributes::new()",
            "tauri_build::Attributes::new().config_path(\"other.conf.json\")",
        ),
    )
}

/// `build.rs` sets `TAURI_CONFIG`, which tauri-build merges over `tauri.conf.json`, on line 2.
pub(in crate::checks::tauri_config::tests) fn build_rs_sets_env_config() -> Repo {
    build_case(
        "build-rs-sets-env-config",
        &BUILD.replace(
            "fn main() {\n",
            "fn main() {\n    unsafe { std::env::set_var(\"TAURI_CONFIG\", \"{}\") };\n",
        ),
    )
}

/// `build.rs` hands `TAURI_CONFIG` to rustc, so `generate_context!` merges it, on line 2.
pub(in crate::checks::tauri_config::tests) fn build_rs_rustc_env_config() -> Repo {
    build_case(
        "build-rs-rustc-env-config",
        &BUILD.replace(
            "fn main() {\n",
            "fn main() {\n    println!(\"cargo:rustc-env=TAURI_CONFIG={{}}\");\n",
        ),
    )
}
