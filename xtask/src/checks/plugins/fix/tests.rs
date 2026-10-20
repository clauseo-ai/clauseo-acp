//! Pins the text of every `plugins::Fix` variant. The `match` has no `_` arm, so a new variant
//! does not compile until it has a pinned text here.

use super::Fix;
use std::collections::HashSet;
use std::mem::discriminant;

fn pinned(fix: &Fix) -> String {
    match fix {
        Fix::Feature => "remove it: devtools let script in the page open the inspector, and \
             `protocol-asset` serves files from disk to the webview"
            .to_owned(),
        Fix::Runtime => {
            "remove it: tauri-runtime is there only to name a type, and its `devtools` \
             and `macos-private-api` features would reach the binary through it"
                .to_owned()
        }
        Fix::PluginOutsideSet => "remove it: the plugin set is fixed (each change forces a \
             near-cold rebuild and widens what the webview can reach); changing it needs a \
             decision recorded in a task brief"
            .to_owned(),
        Fix::Optional => "drop `optional = true` and any feature that enables it: the plugin set \
             is fixed and sits behind no cargo feature, so every build ships the same plugins"
            .to_owned(),
        Fix::Family => {
            "depend on `tauri` and the fixed plugins only: other Tauri crates can switch \
             on features such as devtools that the tauri entry's checks never see"
                .to_owned()
        }
        Fix::MemberDependsOnTauri => {
            "only crates/desktop depends on Tauri; move the code there or behind the host interface"
                .to_owned()
        }
        Fix::OutsideMembers => {
            "add it to the workspace members, or remove the dependency".to_owned()
        }
        Fix::AddBackToDependencies { plugin } => {
            format!("add it back under [dependencies]: `{plugin}.workspace = true`")
        }
        Fix::Patched => "remove the entry: Tauri crates come from crates.io at the versions \
             Cargo.lock pins; using a fork needs a decision recorded in a task brief"
            .to_owned(),
        Fix::Tree => "only the fixed plugins and what they pull in may be in the desktop crate's \
             tree"
            .to_owned(),
        Fix::RemoveDeclared { keys, lines, path } => format!(
            "remove {keys} from lines {lines} of {path}: only the fixed plugins and what they pull \
             in may be in the desktop crate's tree"
        ),
    }
}

#[test]
fn every_fix_prints_its_pinned_text() {
    let samples = [
        Fix::Feature,
        Fix::Runtime,
        Fix::PluginOutsideSet,
        Fix::Optional,
        Fix::Family,
        Fix::MemberDependsOnTauri,
        Fix::OutsideMembers,
        Fix::AddBackToDependencies {
            plugin: "tauri-plugin-os".to_owned(),
        },
        Fix::Tree,
        Fix::Patched,
        Fix::RemoveDeclared {
            keys: "`s`".to_owned(),
            lines: "7 and 10".to_owned(),
            path: "crates/desktop/Cargo.toml".to_owned(),
        },
    ];
    let kinds: HashSet<_> = samples.iter().map(discriminant).collect();
    assert_eq!(
        kinds.len(),
        samples.len(),
        "two samples are the same variant"
    );
    for fix in &samples {
        assert_eq!(fix.to_string(), pinned(fix));
    }
}
