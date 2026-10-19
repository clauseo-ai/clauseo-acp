//! The planted cases of the plugins check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in the
//! check's tests module.

crate::checks::planted::cases! {
    /// The fixed plugin set in the desktop crate, with its two Tauri crates, passes.
    fixed_plugin_set_passes: Passes => super::fixed_plugin_set_passes,
    /// `tauri-build` as a build dependency of the desktop crate passes.
    tauri_build_as_a_build_dependency_passes: Passes
        => super::tauri_build_as_a_build_dependency_passes,
    /// A plugin outside the fixed set as a dev-dependency passes: only normal ones count.
    dev_dependency_plugin_passes: Passes => super::dev_dependency_plugin_passes,
    /// A plugin outside the fixed set in `[dependencies]` fails at its line.
    plugin_outside_fixed_set: FailsWith(&[
        "crates/desktop/Cargo.toml:17: `tauri-plugin-shell` is not in the fixed plugin set\n    \
         fix: remove it: the plugin set is fixed (each change forces a near-cold rebuild and \
         widens what the webview can reach); changing it needs a decision recorded in a task brief",
    ]) => super::plugin_outside_fixed_set,
    /// A plugin outside the fixed set, renamed in the workspace table, is named by its package.
    renamed_plugin_outside_fixed_set: FailsWith(&[
        "crates/desktop/Cargo.toml:17: `tauri-plugin-fs` (as `files`) is not in the fixed plugin \
         set\n    \
         fix: remove it: the plugin set is fixed (each change forces a near-cold rebuild and \
         widens what the webview can reach); changing it needs a decision recorded in a task brief",
    ]) => super::renamed_plugin_outside_fixed_set,
    /// A plugin outside the fixed set in a `[target.*]` table fails.
    target_plugin_outside_fixed_set: FailsWith(&[
        "crates/desktop/Cargo.toml:19: `tauri-plugin-fs` is not in the fixed plugin set\n    \
         fix: remove it: the plugin set is fixed (each change forces a near-cold rebuild and \
         widens what the webview can reach); changing it needs a decision recorded in a task brief",
    ]) => super::target_plugin_outside_fixed_set,
    /// A fixed plugin with `optional = true` fails at its line.
    optional_fixed_plugin: FailsWith(&[
        "crates/desktop/Cargo.toml:13: `tauri-plugin-dialog` is optional\n    \
         fix: drop `optional = true` and any feature that enables it: the plugin set is fixed and \
         sits behind no cargo feature, so every build ships the same plugins",
    ]) => super::optional_fixed_plugin,
    /// A fixed plugin missing from the desktop crate fails at the `[dependencies]` header.
    fixed_plugin_missing: FailsWith(&[
        "crates/desktop/Cargo.toml:6: `tauri-plugin-os` is missing from the fixed plugin set\n    \
         fix: add it back under [dependencies]: `tauri-plugin-os.workspace = true`",
    ]) => super::fixed_plugin_missing,
    /// `wry` in a `[target.*]` table of the desktop crate fails as a Tauri crate.
    wry_in_desktop: FailsWith(&[
        "crates/desktop/Cargo.toml:19: `wry` is a Tauri crate outside the allowed set\n    \
         fix: depend on `tauri` and the fixed plugins only: other Tauri crates can switch on \
         features such as devtools that the tauri entry's checks never see",
    ]) => super::wry_in_desktop,
    /// `tauri-runtime-wry` with `devtools` in the desktop crate fails as a Tauri crate.
    tauri_runtime_wry_in_desktop: FailsWith(&[
        "crates/desktop/Cargo.toml:17: `tauri-runtime-wry` is a Tauri crate outside the allowed \
         set\n    \
         fix: depend on `tauri` and the fixed plugins only: other Tauri crates can switch on \
         features such as devtools that the tauri entry's checks never see",
    ]) => super::tauri_runtime_wry_in_desktop,
    /// `devtools` on the `tauri-runtime` entry of the desktop crate fails.
    tauri_runtime_devtools_in_desktop: FailsWith(&[
        "crates/desktop/Cargo.toml:8: `tauri-runtime` enables the `devtools` feature\n    \
         fix: remove it: tauri-runtime is there only to name a type, and its `devtools` and \
         `macos-private-api` features would reach the binary through it",
    ]) => super::tauri_runtime_devtools_in_desktop,
    /// `macos-private-api` on the workspace's `tauri-runtime` entry fails at the root line.
    tauri_runtime_feature_in_workspace: FailsWith(&[
        "Cargo.toml:7: `tauri-runtime` enables the `macos-private-api` feature\n    \
         fix: remove it: tauri-runtime is there only to name a type, and its `devtools` and \
         `macos-private-api` features would reach the binary through it",
    ]) => super::tauri_runtime_feature_in_workspace,
    /// `devtools` in the desktop crate's `tauri` entry fails at the line of the feature.
    devtools_in_tauri_entry: FailsWith(&[
        "crates/desktop/Cargo.toml:8: `tauri` enables the `devtools` feature\n    \
         fix: remove it: devtools let script in the page open the inspector, and `protocol-asset` \
         serves files from disk to the webview",
    ]) => super::devtools_in_tauri_entry,
    /// `protocol-asset` in the desktop crate's `tauri` entry fails.
    protocol_asset_in_tauri_entry: FailsWith(&[
        "crates/desktop/Cargo.toml:7: `tauri` enables the `protocol-asset` feature\n    \
         fix: remove it: devtools let script in the page open the inspector, and `protocol-asset` \
         serves files from disk to the webview",
    ]) => super::protocol_asset_in_tauri_entry,
    /// `devtools` in a `tauri` entry renamed to `t` fails, named by its package at the root line.
    devtools_in_renamed_tauri_entry: FailsWith(&[
        "Cargo.toml:6: `tauri` enables the `devtools` feature\n    \
         fix: remove it: devtools let script in the page open the inspector, and `protocol-asset` \
         serves files from disk to the webview",
    ]) => super::devtools_in_renamed_tauri_entry,
    /// `protocol-asset` in a `tauri` entry renamed to `t` in the workspace table fails.
    protocol_asset_in_renamed_tauri_entry: FailsWith(&[
        "Cargo.toml:6: `tauri` enables the `protocol-asset` feature\n    \
         fix: remove it: devtools let script in the page open the inspector, and `protocol-asset` \
         serves files from disk to the webview",
    ]) => super::protocol_asset_in_renamed_tauri_entry,
    /// `[features]` turning on `tauri/devtools` fails at the feature's line.
    feature_table_enables_devtools_through_tauri: FailsWith(&[
        "crates/desktop/Cargo.toml:19: `[features]` enables the `tauri` feature `devtools` through \
         `tauri/devtools`\n    \
         fix: remove it: devtools let script in the page open the inspector, and `protocol-asset` \
         serves files from disk to the webview",
    ]) => super::feature_table_enables_devtools_through_tauri,
    /// `[features]` turning on `protocol-asset` through a renamed `tauri` entry fails.
    feature_table_enables_through_a_rename: FailsWith(&[
        "crates/desktop/Cargo.toml:19: `[features]` enables the `tauri` feature `protocol-asset` \
         through `t/protocol-asset`\n    \
         fix: remove it: devtools let script in the page open the inspector, and `protocol-asset` \
         serves files from disk to the webview",
    ]) => super::feature_table_enables_through_a_rename,
    /// `[features]` turning on `tauri?/devtools` through an optional `tauri` fails.
    feature_table_enables_through_an_optional_tauri: FailsWith(&[
        "crates/desktop/Cargo.toml:19: `[features]` enables the `tauri` feature `devtools` through \
         `tauri?/devtools`\n    \
         fix: remove it: devtools let script in the page open the inspector, and `protocol-asset` \
         serves files from disk to the webview",
    ]) => super::feature_table_enables_through_an_optional_tauri,
    /// Another member depending on `tauri` fails at its line.
    other_member_depends_on_tauri: FailsWith(&[
        "crates/core/Cargo.toml:7: `clauseo-core` depends on `tauri`\n    \
         fix: only crates/desktop depends on Tauri; move the code there or behind the host \
         interface",
    ]) => super::other_member_depends_on_tauri,
    /// Another member depending on `wry` fails.
    other_member_depends_on_wry: FailsWith(&[
        "crates/core/Cargo.toml:7: `clauseo-core` depends on `wry`\n    \
         fix: only crates/desktop depends on Tauri; move the code there or behind the host \
         interface",
    ]) => super::other_member_depends_on_wry,
    /// Another member's renamed `tauri-runtime` in a target table fails under its key.
    other_member_renamed_tauri_runtime_in_target: FailsWith(&[
        "crates/core/Cargo.toml:7: `clauseo-core` depends on `tauri-runtime` (as `rt`)\n    \
         fix: only crates/desktop depends on Tauri; move the code there or behind the host \
         interface",
    ]) => super::other_member_renamed_tauri_runtime_in_target,
    /// Another member's `tauri` by path, to a crate outside the workspace, fails twice: as a
    /// member's Tauri dependency and as a path dependency that clippy never sees.
    other_member_depends_on_tauri_through_a_path_dependency: FailsWith(&[
        "crates/core/Cargo.toml:7: `clauseo-core` depends on `tauri`\n    \
         fix: only crates/desktop depends on Tauri; move the code there or behind the host \
         interface",
        "crates/core/Cargo.toml:7: `clauseo-core` depends on `tauri` by path, but `tauri` is not a \
         workspace member, so these checks and clippy never see it\n    \
         fix: add it to the workspace members, or remove the dependency",
    ]) => super::other_member_depends_on_tauri_through_a_path_dependency,
    /// A path dependency on a crate under `[workspace] exclude` fails at its line.
    path_dependency_on_excluded_crate: FailsWith(&[
        "crates/desktop/Cargo.toml:17: `clauseo-desktop` depends on `evil` by path, but `evil` is \
         not a workspace member, so these checks and clippy never see it\n    \
         fix: add it to the workspace members, or remove the dependency",
    ]) => super::path_dependency_on_excluded_crate,
    /// A `cargo metadata` that lists no package named like the desktop crate stops the check.
    metadata_lists_no_desktop_package: FailsWith(&[
        "crates/desktop/Cargo.toml:1: `cargo metadata` lists no package `clauseo-desktop`\n    \
         fix: fix the manifest until `cargo metadata --no-deps` succeeds: this check reads the \
         dependencies from it",
        "0 direct dependencies inspected of the desktop crate (package `clauseo-desktop`) in \
         `cargo metadata`\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]) => super::metadata_lists_no_desktop_package,
    /// A context with no members stops the check at the root manifest, before it inspects.
    desktop_crate_missing: FailsWith(&[
        "Cargo.toml:1: the desktop crate (package `clauseo-desktop`) is not a workspace member\n    \
         fix: restore crates/desktop or update xtask/src/checks: these checks guard its \
         capabilities, plugins and Tauri config",
        "0 direct dependencies inspected of the desktop crate (package `clauseo-desktop`) in \
         `cargo metadata`\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]) => super::desktop_crate_missing,
    /// A plugin outside the fixed set behind an optional desktop dependency (feature `extra`)
    /// fails at that dependency's line: the desktop crate's graph is read with all its features.
    plugin_behind_an_optional_desktop_feature: FailsWith(&[
        "crates/desktop/Cargo.toml:17: `tauri-plugin-zz` 2.0.0 is in the desktop crate's tree \
         through `mid`, which is not a fixed plugin\n    \
         fix: only the fixed plugins and what they pull in may be in the desktop crate's tree",
    ]) => super::resolved::plugin_behind_an_optional_desktop_feature,
    /// The same optional dependency without a plugin under it passes.
    optional_desktop_feature_without_a_plugin_passes: Passes
        => super::resolved::optional_desktop_feature_without_a_plugin_passes,
}
