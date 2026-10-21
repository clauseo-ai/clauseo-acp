//! The fix lines of the dependencies check, one variant per failure kind.
//! Each text is written once, in `Display`; `fix/tests.rs` pins every variant's text.

use std::fmt;

/// A fix line of the dependencies check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// A row says `exists`, but no workspace crate is its package.
    SetPlanned,
    /// A row says `planned`, but its crate is a workspace member.
    SetExists,
    /// A workspace crate has no row: the row to add, for the crate named `name`.
    AddRow { name: String },
    /// A workspace crate's package is not `clauseo-` followed by a row name.
    RenamePackage,
    /// A crate's dependency the row forbids, declared at one or more lines of its manifest.
    /// `keys` is the declarations' keys, quoted, as one list, such as `` `s1` and `s2` ``, and
    /// `lines` their lines, such as `7 and 10`.
    MoveFromLines {
        keys: String,
        package: String,
        lines: String,
        manifest: String,
        features: bool,
        row: u32,
        krate: String,
    },
    /// A crate's dependency the row forbids, declared at one line of its manifest, or at none
    /// that xtask could find.
    MoveFromManifest {
        keys: String,
        package: String,
        manifest: String,
        features: bool,
        row: u32,
        krate: String,
    },
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fix::SetPlanned => f.write_str(
                "change the Status to `planned`: a row is `exists` only while its crate is a \
                 workspace member",
            ),
            Fix::SetExists => f.write_str("change the Status to `exists`: the crate exists"),
            Fix::AddRow { name } => write!(
                f,
                "add this row to the Crates table in ARCHITECTURE.md: | `{name}` | <what it owns> | \
                 <crates it may depend on> | <crates it never depends on> | exists |"
            ),
            Fix::RenamePackage => f.write_str(
                "rename the package to `clauseo-` followed by its row name (the table maps row \
                 `<name>` to package `clauseo-<name>`), then add that row to the Crates table in \
                 ARCHITECTURE.md with its five columns: Crate | Owns | May depend on | Never \
                 depends on | Status",
            ),
            Fix::MoveFromLines {
                keys,
                package,
                lines,
                manifest,
                features,
                row,
                krate,
            } => write!(
                f,
                "move the code that uses {keys} into a crate whose row allows {package}, then \
                 remove {keys} from lines {lines} of {manifest}{features}: the row at \
                 ARCHITECTURE.md:{row} says {krate} never depends on {package}",
                features = features_text(*features),
            ),
            Fix::MoveFromManifest {
                keys,
                package,
                manifest,
                features,
                row,
                krate,
            } => write!(
                f,
                "move the code that uses {keys} into a crate whose row allows {package}, then \
                 remove {keys}{features} from {manifest}: the row at ARCHITECTURE.md:{row} says \
                 {krate} never depends on {package}",
                features = features_text(*features),
            ),
        }
    }
}

/// The part of a move's fix that covers the `[features]` items enabling a declaration, when
/// the declaration is optional.
fn features_text(features: bool) -> &'static str {
    if features {
        " and every `[features]` item that enables it"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests;
