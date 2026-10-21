//! The fix lines of the plugins check, one variant per failure kind.
//! Each text is written once, in `Display`; `fix/tests.rs` pins every variant's text.

use std::fmt;

/// A fix line of the plugins check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// A `tauri` entry or a `[features]` item enables `devtools` or `protocol-asset`.
    Feature,
    /// A `tauri-runtime` entry enables a feature.
    Runtime,
    /// A `tauri-plugin-*` package outside the fixed plugin set.
    PluginOutsideSet,
    /// A fixed plugin is declared `optional = true`.
    Optional,
    /// A Tauri crate outside the allowed set (`tauri`, `tauri-runtime` and the fixed plugins).
    Family,
    /// A member other than the desktop crate depends on a Tauri family member.
    MemberDependsOnTauri,
    /// A member depends by path on a package that is not a workspace member.
    OutsideMembers,
    /// A fixed plugin is missing from the desktop crate; `plugin` is its package name.
    AddBackToDependencies { plugin: String },
    /// The desktop crate's graph holds a plugin outside the set, and no declaring line is known.
    Tree,
    /// A `[patch]` or `[replace]` entry swaps a Tauri crate for another source, or the cargo
    /// config has a `paths` override or a source replacement.
    Patched,
    /// The desktop crate's graph holds a plugin outside the set, and these declarations bring it
    /// in: `keys` are the declared names (quoted, such as `` `s` ``), `lines` the manifest lines
    /// as a list (such as `7 and 10`), and `path` the desktop manifest.
    RemoveDeclared {
        keys: String,
        lines: String,
        path: String,
    },
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fix::Feature => f.write_str(
                "remove it: devtools let script in the page open the inspector, and \
                 `protocol-asset` serves files from disk to the webview",
            ),
            Fix::Runtime => f.write_str(
                "remove it: tauri-runtime is there only to name a type, and its `devtools` and \
                 `macos-private-api` features would reach the binary through it",
            ),
            Fix::PluginOutsideSet => f.write_str(
                "remove it: the plugin set is fixed (each change forces a near-cold rebuild and \
                 widens what the webview can reach); changing it needs a decision recorded in a \
                 task brief",
            ),
            Fix::Optional => f.write_str(
                "drop `optional = true` and any feature that enables it: the plugin set is fixed \
                 and sits behind no cargo feature, so every build ships the same plugins",
            ),
            Fix::Family => f.write_str(
                "depend on `tauri` and the fixed plugins only: other Tauri crates can switch on \
                 features such as devtools that the tauri entry's checks never see",
            ),
            Fix::MemberDependsOnTauri => f.write_str(
                "only crates/desktop depends on Tauri; move the code there or behind the host \
                 interface",
            ),
            Fix::OutsideMembers => {
                f.write_str("add it to the workspace members, or remove the dependency")
            }
            Fix::AddBackToDependencies { plugin } => {
                write!(
                    f,
                    "add it back under [dependencies]: `{plugin}.workspace = true`"
                )
            }
            Fix::Patched => f.write_str(
                "remove the entry: Tauri crates come from crates.io at the versions Cargo.lock \
                 pins; using a fork needs a decision recorded in a task brief",
            ),
            Fix::Tree => f.write_str(
                "only the fixed plugins and what they pull in may be in the desktop crate's tree",
            ),
            Fix::RemoveDeclared { keys, lines, path } => write!(
                f,
                "remove {keys} from lines {lines} of {path}: only the fixed plugins and what they \
                 pull in may be in the desktop crate's tree"
            ),
        }
    }
}

#[cfg(test)]
mod tests;
