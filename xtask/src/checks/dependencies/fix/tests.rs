//! Pins the text of every `dependencies::Fix` variant. The `match` has no `_` arm, so a new
//! variant does not compile until it has a pinned text here.

use super::Fix;
use std::collections::HashSet;
use std::mem::discriminant;

fn pinned(fix: &Fix) -> String {
    match fix {
        Fix::SetPlanned => "change the Status to `planned`: a row is `exists` only while its crate is \
             a workspace member"
            .to_owned(),
        Fix::SetExists => "change the Status to `exists`: the crate exists".to_owned(),
        Fix::AddRow { name } => format!(
            "add this row to the Crates table in ARCHITECTURE.md: | `{name}` | <what it owns> | \
             <crates it may depend on> | <crates it never depends on> | exists |"
        ),
        Fix::RenamePackage => "rename the package to `clauseo-` followed by its row name (the table \
             maps row `<name>` to package `clauseo-<name>`), then add that row to the Crates table \
             in ARCHITECTURE.md with its five columns: Crate | Owns | May depend on | Never depends \
             on | Status"
            .to_owned(),
        Fix::MoveFromLines {
            keys,
            package,
            lines,
            manifest,
            features,
            row,
            krate,
        } => format!(
            "move the code that uses {keys} into a crate whose row allows {package}, then remove \
             {keys} from lines {lines} of {manifest}{features}: the row at ARCHITECTURE.md:{row} \
             says {krate} never depends on {package}",
            features = if *features {
                " and every `[features]` item that enables it"
            } else {
                ""
            },
        ),
        Fix::MoveFromManifest {
            keys,
            package,
            manifest,
            features,
            row,
            krate,
        } => format!(
            "move the code that uses {keys} into a crate whose row allows {package}, then remove \
             {keys}{features} from {manifest}: the row at ARCHITECTURE.md:{row} says {krate} never \
             depends on {package}",
            features = if *features {
                " and every `[features]` item that enables it"
            } else {
                ""
            },
        ),
    }
}

fn move_from_lines(features: bool) -> Fix {
    Fix::MoveFromLines {
        keys: "`s1` and `s2`".to_owned(),
        package: "tauri".to_owned(),
        lines: "7 and 10".to_owned(),
        manifest: "crates/protocol/Cargo.toml".to_owned(),
        features,
        row: 7,
        krate: "clauseo-protocol".to_owned(),
    }
}

fn move_from_manifest(features: bool) -> Fix {
    Fix::MoveFromManifest {
        keys: "`tauri`".to_owned(),
        package: "tauri".to_owned(),
        manifest: "crates/protocol/Cargo.toml".to_owned(),
        features,
        row: 7,
        krate: "clauseo-protocol".to_owned(),
    }
}

#[test]
fn every_fix_prints_its_pinned_text() {
    let samples = [
        Fix::SetPlanned,
        Fix::SetExists,
        Fix::AddRow {
            name: "store".to_owned(),
        },
        Fix::RenamePackage,
        move_from_lines(false),
        move_from_manifest(false),
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

#[test]
fn a_move_names_the_lines_and_the_features_only_when_they_apply() {
    assert_eq!(
        move_from_lines(true).to_string(),
        "move the code that uses `s1` and `s2` into a crate whose row allows tauri, then remove \
         `s1` and `s2` from lines 7 and 10 of crates/protocol/Cargo.toml and every `[features]` \
         item that enables it: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on \
         tauri"
    );
    assert_eq!(
        move_from_manifest(true).to_string(),
        "move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` \
         and every `[features]` item that enables it from crates/protocol/Cargo.toml: the row at \
         ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri"
    );
    assert_eq!(
        move_from_manifest(false).to_string(),
        "move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` \
         from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never \
         depends on tauri"
    );
}
