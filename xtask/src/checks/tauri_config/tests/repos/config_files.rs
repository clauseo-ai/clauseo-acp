//! The tauri-config builders for the other config files Tauri reads: platform configs, and a
//! main config in json5 or toml.

use super::*;

pub(in crate::checks::tauri_config::tests) fn platform_config_json() -> Repo {
    platform_case(
        "platform-config-json",
        "crates/desktop/tauri.macos.conf.json",
        "{\"app\":{\"withGlobalTauri\":true}}",
    )
}

pub(in crate::checks::tauri_config::tests) fn platform_config_json5() -> Repo {
    platform_case(
        "platform-config-json5",
        "crates/desktop/tauri.linux.conf.json5",
        "{ app: { withGlobalTauri: true } }",
    )
}

pub(in crate::checks::tauri_config::tests) fn platform_config_toml() -> Repo {
    platform_case(
        "platform-config-toml",
        "crates/desktop/Tauri.windows.toml",
        "[app]\nwithGlobalTauri = true\n",
    )
}

pub(in crate::checks::tauri_config::tests) fn platform_config_android() -> Repo {
    platform_case(
        "platform-config-android",
        "crates/desktop/tauri.android.conf.json",
        "{\"app\":{\"withGlobalTauri\":true}}",
    )
}

pub(in crate::checks::tauri_config::tests) fn platform_config_ios_toml() -> Repo {
    platform_case(
        "platform-config-ios-toml",
        "crates/desktop/Tauri.ios.toml",
        "[app]\nwithGlobalTauri = true\n",
    )
}

/// A platform config named in capitals: a macOS or Windows disk hands it to Tauri all the same.
pub(in crate::checks::tauri_config::tests) fn platform_config_other_case() -> Repo {
    platform_case(
        "platform-config-other-case",
        "crates/desktop/TAURI.MACOS.CONF.JSON",
        "{\"app\":{\"withGlobalTauri\":true}}",
    )
}

/// `tauri.conf.json5` as the only main config: Tauri reads it when `tauri.conf.json` is missing.
pub(in crate::checks::tauri_config::tests) fn main_config_json5_only() -> Repo {
    desktop_repo(
        "main-config-json5-only",
        &[
            (
                "crates/desktop/tauri.conf.json5",
                b"{ app: { withGlobalTauri: true } }",
            ),
            (BUILD_AT, BUILD.as_bytes()),
        ],
    )
}

/// `Tauri.toml` as the only main config: Tauri reads it when `tauri.conf.json` is missing.
pub(in crate::checks::tauri_config::tests) fn main_config_toml_only() -> Repo {
    desktop_repo(
        "main-config-toml-only",
        &[
            (
                "crates/desktop/Tauri.toml",
                b"[app]\nwithGlobalTauri = true\n",
            ),
            (BUILD_AT, BUILD.as_bytes()),
        ],
    )
}
