//! The planted cases of the plugins check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). Their builders are in `tests/repos.rs` and `repos/`.

use crate::checks::planted::fails;
use crate::checks::plugins::Fix;

crate::checks::planted::cases! {
    /// The fixed plugin set in the desktop crate, with its two Tauri crates, passes.
    fixed_plugin_set_passes: Passes, inspects 10 => super::repos::fixed_plugin_set_passes,
    /// `tauri-build` as a build dependency of the desktop crate passes.
    tauri_build_as_a_build_dependency_passes: Passes, inspects 11
        => super::repos::tauri_build_as_a_build_dependency_passes,
    /// A plugin outside the fixed set as a dev-dependency passes: only normal ones count.
    dev_dependency_plugin_passes: Passes, inspects 10 => super::repos::dev_dependency_plugin_passes,
    /// A plugin outside the fixed set in `[dependencies]` fails at its line.
    plugin_outside_fixed_set: Fails[fails(
        "crates/desktop/Cargo.toml:17",
        "`tauri-plugin-shell` is not in the fixed plugin set",
        Fix::PluginOutsideSet,
    )], inspects 11 => super::repos::plugin_outside_fixed_set,
    /// A plugin outside the fixed set, renamed in the workspace table, is named by its package.
    renamed_plugin_outside_fixed_set: Fails[fails(
        "crates/desktop/Cargo.toml:17",
        "`tauri-plugin-fs` (as `files`) is not in the fixed plugin set",
        Fix::PluginOutsideSet,
    )], inspects 11 => super::repos::renamed_plugin_outside_fixed_set,
    /// A plugin outside the fixed set in a `[target.*]` table fails.
    target_plugin_outside_fixed_set: Fails[fails(
        "crates/desktop/Cargo.toml:19",
        "`tauri-plugin-fs` is not in the fixed plugin set",
        Fix::PluginOutsideSet,
    )], inspects 11 => super::repos::target_plugin_outside_fixed_set,
    /// A fixed plugin with `optional = true` fails at its line.
    optional_fixed_plugin: Fails[fails(
        "crates/desktop/Cargo.toml:13",
        "`tauri-plugin-dialog` is optional",
        Fix::Optional,
    )], inspects 10 => super::repos::optional_fixed_plugin,
    /// A fixed plugin missing from the desktop crate fails at the `[dependencies]` header.
    fixed_plugin_missing: Fails[fails(
        "crates/desktop/Cargo.toml:6",
        "`tauri-plugin-os` is missing from the fixed plugin set",
        Fix::AddBackToDependencies { plugin: "tauri-plugin-os".to_owned() },
    )], inspects 9 => super::repos::fixed_plugin_missing,
    /// `wry` in a `[target.*]` table of the desktop crate fails as a Tauri crate.
    wry_in_desktop: Fails[fails(
        "crates/desktop/Cargo.toml:19",
        "`wry` is a Tauri crate outside the allowed set",
        Fix::Family,
    )], inspects 11 => super::repos::wry_in_desktop,
    /// `tauri-runtime-wry` with `devtools` in the desktop crate fails as a Tauri crate.
    tauri_runtime_wry_in_desktop: Fails[fails(
        "crates/desktop/Cargo.toml:17",
        "`tauri-runtime-wry` is a Tauri crate outside the allowed set",
        Fix::Family,
    )], inspects 11 => super::repos::tauri_runtime_wry_in_desktop,
    /// `devtools` on the `tauri-runtime` entry of the desktop crate fails.
    tauri_runtime_devtools_in_desktop: Fails[fails(
        "crates/desktop/Cargo.toml:8",
        "`tauri-runtime` enables the `devtools` feature",
        Fix::Runtime,
    )], inspects 10 => super::repos::tauri_runtime_devtools_in_desktop,
    /// `macos-private-api` on the workspace's `tauri-runtime` entry fails at the root line.
    tauri_runtime_feature_in_workspace: Fails[fails(
        "Cargo.toml:7",
        "`tauri-runtime` enables the `macos-private-api` feature",
        Fix::Runtime,
    )], inspects 10 => super::repos::tauri_runtime_feature_in_workspace,
    /// `devtools` in the desktop crate's `tauri` entry fails at the line of the feature.
    devtools_in_tauri_entry: Fails[fails(
        "crates/desktop/Cargo.toml:8",
        "`tauri` enables the `devtools` feature",
        Fix::Feature,
    )], inspects 10 => super::repos::devtools_in_tauri_entry,
    /// `protocol-asset` in the desktop crate's `tauri` entry fails.
    protocol_asset_in_tauri_entry: Fails[fails(
        "crates/desktop/Cargo.toml:7",
        "`tauri` enables the `protocol-asset` feature",
        Fix::Feature,
    )], inspects 10 => super::repos::protocol_asset_in_tauri_entry,
    /// `devtools` in a `tauri` entry renamed to `t` fails, named by its package at the root line.
    devtools_in_renamed_tauri_entry: Fails[fails(
        "Cargo.toml:6",
        "`tauri` enables the `devtools` feature",
        Fix::Feature,
    )], inspects 10 => super::repos::devtools_in_renamed_tauri_entry,
    /// `protocol-asset` in a `tauri` entry renamed to `t` in the workspace table fails.
    protocol_asset_in_renamed_tauri_entry: Fails[fails(
        "Cargo.toml:6",
        "`tauri` enables the `protocol-asset` feature",
        Fix::Feature,
    )], inspects 10 => super::repos::protocol_asset_in_renamed_tauri_entry,
    /// `[features]` turning on `tauri/devtools` fails at the feature's line.
    feature_table_enables_devtools_through_tauri: Fails[fails(
        "crates/desktop/Cargo.toml:19",
        "`[features]` enables the `tauri` feature `devtools` through `tauri/devtools`",
        Fix::Feature,
    )], inspects 10 => super::repos::feature_table_enables_devtools_through_tauri,
    /// `[features]` turning on `protocol-asset` through a renamed `tauri` entry fails.
    feature_table_enables_through_a_rename: Fails[fails(
        "crates/desktop/Cargo.toml:19",
        "`[features]` enables the `tauri` feature `protocol-asset` through `t/protocol-asset`",
        Fix::Feature,
    )], inspects 10 => super::repos::feature_table_enables_through_a_rename,
    /// `[features]` turning on `tauri?/devtools` through an optional `tauri` fails.
    feature_table_enables_through_an_optional_tauri: Fails[fails(
        "crates/desktop/Cargo.toml:19",
        "`[features]` enables the `tauri` feature `devtools` through `tauri?/devtools`",
        Fix::Feature,
    )], inspects 10 => super::repos::feature_table_enables_through_an_optional_tauri,
    /// Another member depending on `tauri` fails at its line.
    other_member_depends_on_tauri: Fails[fails(
        "crates/core/Cargo.toml:7",
        "`clauseo-core` depends on `tauri`",
        Fix::MemberDependsOnTauri,
    )], inspects 10 => super::repos::other_member_depends_on_tauri,
    /// Another member depending on `wry` fails.
    other_member_depends_on_wry: Fails[fails(
        "crates/core/Cargo.toml:7",
        "`clauseo-core` depends on `wry`",
        Fix::MemberDependsOnTauri,
    )], inspects 10 => super::repos::other_member_depends_on_wry,
    /// Another member's renamed `tauri-runtime` in a target table fails under its key.
    other_member_renamed_tauri_runtime_in_target: Fails[fails(
        "crates/core/Cargo.toml:7",
        "`clauseo-core` depends on `tauri-runtime` (as `rt`)",
        Fix::MemberDependsOnTauri,
    )], inspects 10 => super::repos::other_member_renamed_tauri_runtime_in_target,
    /// Another member's `tauri` by path, to a crate outside the workspace, fails twice: as a
    /// member's Tauri dependency and as a path dependency that clippy never sees.
    other_member_depends_on_tauri_through_a_path_dependency: Fails[
        fails(
            "crates/core/Cargo.toml:7",
            "`clauseo-core` depends on `tauri`",
            Fix::MemberDependsOnTauri,
        ),
        fails(
            "crates/core/Cargo.toml:7",
            "`clauseo-core` depends on `tauri` by path, but `tauri` is not a workspace member, so \
             these checks and clippy never see it",
            Fix::OutsideMembers,
        ),
    ], inspects 10 => super::repos::other_member_depends_on_tauri_through_a_path_dependency,
    /// A path dependency on a crate under `[workspace] exclude` fails at its line.
    path_dependency_on_excluded_crate: Fails[fails(
        "crates/desktop/Cargo.toml:17",
        "`clauseo-desktop` depends on `evil` by path, but `evil` is not a workspace member, so \
         these checks and clippy never see it",
        Fix::OutsideMembers,
    )], inspects 11 => super::repos::path_dependency_on_excluded_crate,
    /// A context with no members stops the check at the root manifest, before it inspects.
    desktop_crate_missing: Fails[fails(
        "Cargo.toml:1",
        "the desktop crate (package `clauseo-desktop`) is not a workspace member",
        crate::checks::Fix::DesktopNotMember,
    )], inspects 0 => super::repos::desktop_crate_missing,
    /// A plugin outside the fixed set behind an optional desktop dependency (feature `extra`)
    /// fails at that dependency's line: the desktop crate's graph is read with all its features.
    plugin_behind_an_optional_desktop_feature: Fails[fails(
        "crates/desktop/Cargo.toml:17",
        "`tauri-plugin-zz` 2.0.0 is in the desktop crate's tree through `mid`, which is not a \
         fixed plugin",
        Fix::Tree,
    )], inspects 11 => super::repos::plugin_behind_an_optional_desktop_feature,
    /// The same optional dependency without a plugin under it passes.
    optional_desktop_feature_without_a_plugin_passes: Passes, inspects 11
        => super::repos::optional_desktop_feature_without_a_plugin_passes,
}
