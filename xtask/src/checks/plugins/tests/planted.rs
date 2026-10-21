//! The planted cases of the plugins check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in the
//! check's tests module.

crate::checks::planted::cases! {
    /// The fixed plugin set in the desktop crate, with its two Tauri crates, passes.
    fixed_plugin_set_passes: Passes, inspects 10 => super::fixed_plugin_set_passes,
    /// `tauri-build` as a build dependency of the desktop crate passes.
    tauri_build_as_a_build_dependency_passes: Passes, inspects 11
        => super::tauri_build_as_a_build_dependency_passes,
    /// A plugin outside the fixed set as a dev-dependency passes: only normal ones count.
    dev_dependency_plugin_passes: Passes, inspects 10 => super::dev_dependency_plugin_passes,
    /// A plugin outside the fixed set in `[dependencies]` fails at its line.
    plugin_outside_fixed_set: FailsWith(&[
        "crates/desktop/Cargo.toml:17: `tauri-plugin-shell` is not in the fixed plugin set\n    \
         fix: remove it: the plugin set is fixed (each change forces a near-cold rebuild and \
         widens what the webview can reach); changing it needs a decision recorded in a task brief",
    ]), inspects 11 => super::plugin_outside_fixed_set,
    /// A plugin outside the fixed set, renamed in the workspace table, is named by its package.
    renamed_plugin_outside_fixed_set: FailsWith(&[
        "crates/desktop/Cargo.toml:17: `tauri-plugin-fs` (as `files`) is not in the fixed plugin \
         set\n    \
         fix: remove it: the plugin set is fixed (each change forces a near-cold rebuild and \
         widens what the webview can reach); changing it needs a decision recorded in a task brief",
    ]), inspects 11 => super::renamed_plugin_outside_fixed_set,
    /// A plugin outside the fixed set in a `[target.*]` table fails.
    target_plugin_outside_fixed_set: FailsWith(&[
        "crates/desktop/Cargo.toml:19: `tauri-plugin-fs` is not in the fixed plugin set\n    \
         fix: remove it: the plugin set is fixed (each change forces a near-cold rebuild and \
         widens what the webview can reach); changing it needs a decision recorded in a task brief",
    ]), inspects 11 => super::target_plugin_outside_fixed_set,
    /// A fixed plugin with `optional = true` fails at its line.
    optional_fixed_plugin: FailsWith(&[
        "crates/desktop/Cargo.toml:13: `tauri-plugin-dialog` is optional\n    \
         fix: drop `optional = true` and any feature that enables it: the plugin set is fixed and \
         sits behind no cargo feature, so every build ships the same plugins",
    ]), inspects 10 => super::optional_fixed_plugin,
    /// A fixed plugin missing from the desktop crate fails at the `[dependencies]` header.
    fixed_plugin_missing: FailsWith(&[
        "crates/desktop/Cargo.toml:6: `tauri-plugin-os` is missing from the fixed plugin set\n    \
         fix: add it back under [dependencies]: `tauri-plugin-os.workspace = true`",
    ]), inspects 9 => super::fixed_plugin_missing,
    /// `wry` in a `[target.*]` table of the desktop crate fails as a Tauri crate.
    wry_in_desktop: FailsWith(&[
        "crates/desktop/Cargo.toml:19: `wry` is a Tauri crate outside the allowed set\n    \
         fix: depend on `tauri` and the fixed plugins only: other Tauri crates can switch on \
         features such as devtools that the tauri entry's checks never see",
    ]), inspects 11 => super::wry_in_desktop,
    /// `tauri-runtime-wry` with `devtools` in the desktop crate fails as a Tauri crate.
    tauri_runtime_wry_in_desktop: FailsWith(&[
        "crates/desktop/Cargo.toml:17: `tauri-runtime-wry` is a Tauri crate outside the allowed \
         set\n    \
         fix: depend on `tauri` and the fixed plugins only: other Tauri crates can switch on \
         features such as devtools that the tauri entry's checks never see",
    ]), inspects 11 => super::tauri_runtime_wry_in_desktop,
    /// `devtools` on the `tauri-runtime` entry of the desktop crate fails.
    tauri_runtime_devtools_in_desktop: FailsWith(&[
        "crates/desktop/Cargo.toml:8: `tauri-runtime` enables the `devtools` feature\n    \
         fix: remove it: tauri-runtime is there only to name a type, and its `devtools` and \
         `macos-private-api` features would reach the binary through it",
    ]), inspects 10 => super::tauri_runtime_devtools_in_desktop,
    /// `macos-private-api` on the workspace's `tauri-runtime` entry fails at the root line.
    tauri_runtime_feature_in_workspace: FailsWith(&[
        "Cargo.toml:7: `tauri-runtime` enables the `macos-private-api` feature\n    \
         fix: remove it: tauri-runtime is there only to name a type, and its `devtools` and \
         `macos-private-api` features would reach the binary through it",
    ]), inspects 10 => super::tauri_runtime_feature_in_workspace,
    /// `devtools` in the desktop crate's `tauri` entry fails at the line of the feature.
    devtools_in_tauri_entry: FailsWith(&[
        "crates/desktop/Cargo.toml:8: `tauri` enables the `devtools` feature\n    \
         fix: remove it: devtools let script in the page open the inspector, and `protocol-asset` \
         serves files from disk to the webview",
    ]), inspects 10 => super::devtools_in_tauri_entry,
    /// `protocol-asset` in the desktop crate's `tauri` entry fails.
    protocol_asset_in_tauri_entry: FailsWith(&[
        "crates/desktop/Cargo.toml:7: `tauri` enables the `protocol-asset` feature\n    \
         fix: remove it: devtools let script in the page open the inspector, and `protocol-asset` \
         serves files from disk to the webview",
    ]), inspects 10 => super::protocol_asset_in_tauri_entry,
    /// `devtools` in a `tauri` entry renamed to `t` fails, named by its package at the root line.
    devtools_in_renamed_tauri_entry: FailsWith(&[
        "Cargo.toml:6: `tauri` enables the `devtools` feature\n    \
         fix: remove it: devtools let script in the page open the inspector, and `protocol-asset` \
         serves files from disk to the webview",
    ]), inspects 10 => super::devtools_in_renamed_tauri_entry,
    /// `protocol-asset` in a `tauri` entry renamed to `t` in the workspace table fails.
    protocol_asset_in_renamed_tauri_entry: FailsWith(&[
        "Cargo.toml:6: `tauri` enables the `protocol-asset` feature\n    \
         fix: remove it: devtools let script in the page open the inspector, and `protocol-asset` \
         serves files from disk to the webview",
    ]), inspects 10 => super::protocol_asset_in_renamed_tauri_entry,
    /// `[features]` turning on `tauri/devtools` fails at the feature's line.
    feature_table_enables_devtools_through_tauri: FailsWith(&[
        "crates/desktop/Cargo.toml:19: `[features]` enables the `tauri` feature `devtools` through \
         `tauri/devtools`\n    \
         fix: remove it: devtools let script in the page open the inspector, and `protocol-asset` \
         serves files from disk to the webview",
    ]), inspects 10 => super::feature_table_enables_devtools_through_tauri,
    /// `[features]` turning on `protocol-asset` through a renamed `tauri` entry fails.
    feature_table_enables_through_a_rename: FailsWith(&[
        "crates/desktop/Cargo.toml:19: `[features]` enables the `tauri` feature `protocol-asset` \
         through `t/protocol-asset`\n    \
         fix: remove it: devtools let script in the page open the inspector, and `protocol-asset` \
         serves files from disk to the webview",
    ]), inspects 10 => super::feature_table_enables_through_a_rename,
    /// `[features]` turning on `tauri?/devtools` through an optional `tauri` fails.
    feature_table_enables_through_an_optional_tauri: FailsWith(&[
        "crates/desktop/Cargo.toml:19: `[features]` enables the `tauri` feature `devtools` through \
         `tauri?/devtools`\n    \
         fix: remove it: devtools let script in the page open the inspector, and `protocol-asset` \
         serves files from disk to the webview",
    ]), inspects 10 => super::feature_table_enables_through_an_optional_tauri,
    /// Another member depending on `tauri` fails at its line.
    other_member_depends_on_tauri: FailsWith(&[
        "crates/core/Cargo.toml:7: `clauseo-core` depends on `tauri`\n    \
         fix: only crates/desktop depends on Tauri; move the code there or behind the host \
         interface",
    ]), inspects 10 => super::other_member_depends_on_tauri,
    /// Another member depending on `wry` fails.
    other_member_depends_on_wry: FailsWith(&[
        "crates/core/Cargo.toml:7: `clauseo-core` depends on `wry`\n    \
         fix: only crates/desktop depends on Tauri; move the code there or behind the host \
         interface",
    ]), inspects 10 => super::other_member_depends_on_wry,
    /// Another member's renamed `tauri-runtime` in a target table fails under its key.
    other_member_renamed_tauri_runtime_in_target: FailsWith(&[
        "crates/core/Cargo.toml:7: `clauseo-core` depends on `tauri-runtime` (as `rt`)\n    \
         fix: only crates/desktop depends on Tauri; move the code there or behind the host \
         interface",
    ]), inspects 10 => super::other_member_renamed_tauri_runtime_in_target,
    /// Another member's `tauri` by path, to a crate outside the workspace, fails twice: as a
    /// member's Tauri dependency and as a path dependency that clippy never sees.
    other_member_depends_on_tauri_through_a_path_dependency: FailsWith(&[
        "crates/core/Cargo.toml:7: `clauseo-core` depends on `tauri`\n    \
         fix: only crates/desktop depends on Tauri; move the code there or behind the host \
         interface",
        "crates/core/Cargo.toml:7: `clauseo-core` depends on `tauri` by path, but `tauri` is not a \
         workspace member, so these checks and clippy never see it\n    \
         fix: add it to the workspace members, or remove the dependency",
    ]), inspects 10 => super::other_member_depends_on_tauri_through_a_path_dependency,
    /// A path dependency on a crate under `[workspace] exclude` fails at its line.
    path_dependency_on_excluded_crate: FailsWith(&[
        "crates/desktop/Cargo.toml:17: `clauseo-desktop` depends on `evil` by path, but `evil` is \
         not a workspace member, so these checks and clippy never see it\n    \
         fix: add it to the workspace members, or remove the dependency",
    ]), inspects 11 => super::path_dependency_on_excluded_crate,
    /// A path build dependency on a crate under `[workspace] exclude` fails at its line: the path
    /// rule reads every table.
    build_dependency_path_to_excluded_crate: FailsWith(&[
        "crates/desktop/Cargo.toml:19: `clauseo-desktop` depends on `evil` by path, but `evil` is \
         not a workspace member, so these checks and clippy never see it\n    \
         fix: add it to the workspace members, or remove the dependency",
    ]), inspects 11 => super::build_dependency_path_to_excluded_crate,
    /// A context with no members stops the check at the root manifest, before it inspects.
    desktop_crate_missing: FailsWith(&[
        "Cargo.toml:1: the desktop crate (package `clauseo-desktop`) is not a workspace member\n    \
         fix: restore crates/desktop or update xtask/src/checks: these checks guard its \
         capabilities, plugins and Tauri config",
    ]), inspects 0 => super::desktop_crate_missing,
    /// A plugin outside the fixed set behind an optional desktop dependency (feature `extra`)
    /// fails at that dependency's line: the desktop crate's graph is read with all its features.
    plugin_behind_an_optional_desktop_feature: FailsWith(&[
        "crates/desktop/Cargo.toml:17: `tauri-plugin-zz` 2.0.0 is in the desktop crate's tree \
         through `mid`, which is not a fixed plugin\n    \
         fix: only the fixed plugins and what they pull in may be in the desktop crate's tree",
    ]), inspects 11 => super::resolved::plugin_behind_an_optional_desktop_feature,
    /// The same optional dependency without a plugin under it passes.
    optional_desktop_feature_without_a_plugin_passes: Passes, inspects 11
        => super::resolved::optional_desktop_feature_without_a_plugin_passes,
    /// A plugin outside the set as a desktop build dependency fails at its line through the graph.
    plugin_as_a_desktop_build_dependency: FailsWith(&[
        "crates/desktop/Cargo.toml:19: `tauri-plugin-zz` 2.0.0 is in the desktop crate's tree \
         through `clauseo-desktop`, which is not a fixed plugin\n    \
         fix: only the fixed plugins and what they pull in may be in the desktop crate's tree",
    ]), inspects 11 => super::resolved::plugin_as_a_desktop_build_dependency,
    /// A plugin pulled in by another member's dependency fails at that member's desktop line.
    plugin_through_another_member: FailsWith(&[
        "crates/desktop/Cargo.toml:17: `tauri-plugin-zz` 2.0.0 is in the desktop crate's tree \
         through `mid`, which is not a fixed plugin\n    \
         fix: only the fixed plugins and what they pull in may be in the desktop crate's tree",
    ]), inspects 11 => super::resolved::plugin_through_another_member,
    /// A crate that depends on `tauri` under a name that is no `tauri-plugin-*` fails.
    plugin_by_another_name: FailsWith(&[
        "crates/desktop/Cargo.toml:17: `evil` 0.1.0 depends on `tauri`, so it can register a \
         Tauri plugin, and is in the desktop crate's tree through `clauseo-desktop`, which is \
         not a fixed plugin\n    \
         fix: only the fixed plugins and what they pull in may be in the desktop crate's tree",
    ]), inspects 11 => super::resolved::plugin_by_another_name,
    /// Root `[patch.crates-io]` swaps the fixed `tauri-plugin-dialog` for a fork that pulls in
    /// `tauri-plugin-zz`: the graph rule trusts the fixed plugin, and the patch rule fails it.
    patched_fixed_plugin_brings_a_plugin: FailsWith(&[
        "Cargo.toml:18: `[patch.crates-io]` swaps in another source for the Tauri crate \
         `tauri-plugin-dialog`, whose dependencies the plugin rules trust\n    \
         fix: remove the entry: Tauri crates come from crates.io at the versions Cargo.lock pins; \
         using a fork needs a decision recorded in a task brief",
    ]), inspects 10 => super::resolved::patched_fixed_plugin_brings_a_plugin,
    /// A `[patch.'<url>']` entry renamed with `package` fails at its key, naming the package.
    patch_renamed_under_a_url: FailsWith(&[
        "Cargo.toml:18: `[patch.'https://github.com/tauri-apps/plugins-workspace']` swaps in \
         another source for the Tauri crate `tauri-plugin-opener`, whose dependencies the plugin \
         rules trust\n    \
         fix: remove the entry: Tauri crates come from crates.io at the versions Cargo.lock pins; \
         using a fork needs a decision recorded in a task brief",
    ]), inspects 10 => super::patch_renamed_under_a_url,
    /// A `[replace]` by package ID spec, in another case, fails at its key.
    replace_by_package_spec: FailsWith(&[
        "Cargo.toml:18: `[replace]` swaps in another source for the Tauri crate `Tao`, whose \
         dependencies the plugin rules trust\n    \
         fix: remove the entry: Tauri crates come from crates.io at the versions Cargo.lock pins; \
         using a fork needs a decision recorded in a task brief",
    ]), inspects 10 => super::replace_by_package_spec,
    /// `[patch]` in `.cargo/config.toml`, which cargo applies too, fails; its `[replace]`,
    /// which cargo does not read there, is not reported.
    patch_in_the_cargo_config: FailsWith(&[
        ".cargo/config.toml:2: `[patch.crates-io]` swaps in another source for the Tauri crate \
         `tauri_runtime`, whose dependencies the plugin rules trust\n    \
         fix: remove the entry: Tauri crates come from crates.io at the versions Cargo.lock pins; \
         using a fork needs a decision recorded in a task brief",
    ]), inspects 10 => super::patch_in_the_cargo_config,
    /// `paths` in `.cargo/config.toml` overrides the fixed `tauri-plugin-dialog` with a fork
    /// that pulls in `tauri-plugin-zz`: the graph rule trusts the fixed plugin, so it fails here.
    paths_override_brings_a_plugin: FailsWith(&[
        ".cargo/config.toml:1: `paths` overrides every crate that a listed folder holds, so it can \
         swap in another source for a Tauri crate, whose dependencies the plugin rules trust\n    \
         fix: remove the entry: Tauri crates come from crates.io at the versions Cargo.lock pins; \
         using a fork needs a decision recorded in a task brief",
    ]), inspects 10 => super::resolved::paths_override_brings_a_plugin,
    /// Source replacement in `.cargo/config.toml` serves crates.io from `forks`, whose
    /// `tauri-plugin-dialog` pulls in `tauri-plugin-zz`: both keys fail.
    source_replacement_brings_a_plugin: FailsWith(&[
        ".cargo/config.toml:2: `replace-with` of source `crates-io` swaps in another source for \
         all of its crates, Tauri crates included, whose dependencies the plugin rules trust\n    \
         fix: remove the entry: Tauri crates come from crates.io at the versions Cargo.lock pins; \
         using a fork needs a decision recorded in a task brief",
        ".cargo/config.toml:5: `directory` of source `forked` makes a source from local files, \
         which `replace-with` can swap in for Tauri crates, whose dependencies the plugin rules \
         trust\n    \
         fix: remove the entry: Tauri crates come from crates.io at the versions Cargo.lock pins; \
         using a fork needs a decision recorded in a task brief",
    ]), inspects 10 => super::resolved::source_replacement_brings_a_plugin,
    /// Patches of crates outside the Tauri family pass: the graph rule sees what they bring.
    patch_of_another_crate_passes: Passes, inspects 10 => super::patch_of_another_crate_passes,
}
