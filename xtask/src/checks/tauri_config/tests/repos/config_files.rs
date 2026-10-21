//! The tauri-config builders for the other config files Tauri reads: the platform configs.

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
