//! The tauri-config builders for the other config files Tauri reads: platform configs and a main config in json5 or toml.

use super::*;

pub(in crate::checks::tauri_config::tests) fn platform_config_json() -> Repo {
    platform_case(
        "platform-config-json",
        "tauri.macos.conf.json",
        "{\"app\":{\"withGlobalTauri\":true}}",
    )
}

pub(in crate::checks::tauri_config::tests) fn platform_config_json5() -> Repo {
    platform_case(
        "platform-config-json5",
        "tauri.linux.conf.json5",
        "{ app: { withGlobalTauri: true } }",
    )
}

pub(in crate::checks::tauri_config::tests) fn platform_config_toml() -> Repo {
    platform_case(
        "platform-config-toml",
        "Tauri.windows.toml",
        "[app]\nwithGlobalTauri = true\n",
    )
}

pub(in crate::checks::tauri_config::tests) fn platform_config_android() -> Repo {
    platform_case(
        "platform-config-android",
        "tauri.android.conf.json",
        "{\"app\":{\"withGlobalTauri\":true}}",
    )
}

pub(in crate::checks::tauri_config::tests) fn platform_config_ios_toml() -> Repo {
    platform_case(
        "platform-config-ios-toml",
        "Tauri.ios.toml",
        "[app]\nwithGlobalTauri = true\n",
    )
}

/// A platform config named in capitals: a macOS or Windows disk hands it to Tauri all the same.
pub(in crate::checks::tauri_config::tests) fn platform_config_other_case() -> Repo {
    platform_case(
        "platform-config-other-case",
        "TAURI.MACOS.CONF.JSON",
        "{\"app\":{\"withGlobalTauri\":true}}",
    )
}

/// `tauri.conf.json5` as the only main config: Tauri reads it when `tauri.conf.json` is missing.
pub(in crate::checks::tauri_config::tests) fn main_config_json5_only() -> Repo {
    run_files(
        "main-config-json5-only",
        &[
            ("tauri.conf.json5", b"{ app: { withGlobalTauri: true } }"),
            (BUILD_RS, BUILD.as_bytes()),
        ],
    )
}

/// `Tauri.toml` as the only main config: Tauri reads it when `tauri.conf.json` is missing.
pub(in crate::checks::tauri_config::tests) fn main_config_toml_only() -> Repo {
    run_files(
        "main-config-toml-only",
        &[
            ("Tauri.toml", b"[app]\nwithGlobalTauri = true\n"),
            (BUILD_RS, BUILD.as_bytes()),
        ],
    )
}
