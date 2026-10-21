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
    /// A path build dependency on a crate under `[workspace] exclude` fails at its line: the path
    /// rule reads every table.
    build_dependency_path_to_excluded_crate: Fails[fails(
        "crates/desktop/Cargo.toml:19",
        "`clauseo-desktop` depends on `evil` by path, but `evil` is not a workspace member, so \
         these checks and clippy never see it",
        Fix::OutsideMembers,
    )], inspects 11 => super::repos::build_dependency_path_to_excluded_crate,
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
    /// A plugin outside the set as a desktop build dependency fails at its line through the graph.
    plugin_as_a_desktop_build_dependency: Fails[fails(
        "crates/desktop/Cargo.toml:19",
        "`tauri-plugin-zz` 2.0.0 is in the desktop crate's tree through `clauseo-desktop`, which \
         is not a fixed plugin",
        Fix::Tree,
    )], inspects 11 => super::repos::plugin_as_a_desktop_build_dependency,
    /// A plugin pulled in by another member's dependency fails at that member's desktop line.
    plugin_through_another_member: Fails[fails(
        "crates/desktop/Cargo.toml:17",
        "`tauri-plugin-zz` 2.0.0 is in the desktop crate's tree through `mid`, which is not a \
         fixed plugin",
        Fix::Tree,
    )], inspects 11 => super::repos::plugin_through_another_member,
    /// A crate that depends on `tauri` under a name that is no `tauri-plugin-*` fails.
    plugin_by_another_name: Fails[fails(
        "crates/desktop/Cargo.toml:17",
        "`evil` 0.1.0 depends on `tauri`, so it can register a Tauri plugin, and is in the \
         desktop crate's tree through `clauseo-desktop`, which is not a fixed plugin",
        Fix::Tree,
    )], inspects 11 => super::repos::plugin_by_another_name,
    /// Root `[patch.crates-io]` swaps the fixed `tauri-plugin-dialog` for a fork that pulls in
    /// `tauri-plugin-zz`: the graph rule trusts the fixed plugin, and the patch rule fails it.
    patched_fixed_plugin_brings_a_plugin: Fails[fails(
        "Cargo.toml:18",
        "`[patch.crates-io]` swaps in another source for the Tauri crate `tauri-plugin-dialog`, \
         whose dependencies the plugin rules trust",
        Fix::Patched,
    )], inspects 10 => super::repos::patched_fixed_plugin_brings_a_plugin,
    /// A `[patch.'<url>']` entry renamed with `package` fails at its key, naming the package.
    patch_renamed_under_a_url: Fails[fails(
        "Cargo.toml:18",
        "`[patch.'https://github.com/tauri-apps/plugins-workspace']` swaps in another source for \
         the Tauri crate `tauri-plugin-opener`, whose dependencies the plugin rules trust",
        Fix::Patched,
    )], inspects 10 => super::repos::patch_renamed_under_a_url,
    /// A `[replace]` by package ID spec, in another case, fails at its key.
    replace_by_package_spec: Fails[fails(
        "Cargo.toml:18",
        "`[replace]` swaps in another source for the Tauri crate `Tao`, whose dependencies the \
         plugin rules trust",
        Fix::Patched,
    )], inspects 10 => super::repos::replace_by_package_spec,
    /// `[patch]` in `.cargo/config.toml`, which cargo applies too, fails; its `[replace]`,
    /// which cargo does not read there, is not reported.
    patch_in_the_cargo_config: Fails[fails(
        ".cargo/config.toml:2",
        "`[patch.crates-io]` swaps in another source for the Tauri crate `tauri_runtime`, whose \
         dependencies the plugin rules trust",
        Fix::Patched,
    )], inspects 10 => super::repos::patch_in_the_cargo_config,
    /// `paths` in `.cargo/config.toml` overrides the fixed `tauri-plugin-dialog` with a fork
    /// that pulls in `tauri-plugin-zz`: the graph rule trusts the fixed plugin, so it fails here.
    paths_override_brings_a_plugin: Fails[fails(
        ".cargo/config.toml:1",
        "`paths` overrides every crate that a listed folder holds, so it can swap in another \
         source for a Tauri crate, whose dependencies the plugin rules trust",
        Fix::Patched,
    )], inspects 10 => super::repos::paths_override_brings_a_plugin,
    /// Source replacement in `.cargo/config.toml` serves crates.io from `forks`, whose
    /// `tauri-plugin-dialog` pulls in `tauri-plugin-zz`: both keys fail.
    source_replacement_brings_a_plugin: Fails[
        fails(
            ".cargo/config.toml:2",
            "`replace-with` of source `crates-io` swaps in another source for all of its crates, \
             Tauri crates included, whose dependencies the plugin rules trust",
            Fix::Patched,
        ),
        fails(
            ".cargo/config.toml:5",
            "`directory` of source `forked` makes a source from local files, which \
             `replace-with` can swap in for Tauri crates, whose dependencies the plugin rules \
             trust",
            Fix::Patched,
        ),
    ], inspects 10 => super::repos::source_replacement_brings_a_plugin,
    /// Patches of crates outside the Tauri family pass: the graph rule sees what they bring.
    patch_of_another_crate_passes: Passes, inspects 10 => super::repos::patch_of_another_crate_passes,
}
