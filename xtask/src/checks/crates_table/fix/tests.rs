//! Pins the text of every `crates_table::Fix` variant with `fix_texts!`: one arm per variant, and
//! `assert_texts` fails until each arm has a sample, so a new variant needs both.

use super::Fix;
use crate::fix::tests::{assert_texts, fix_texts};

fix_texts! {
    fn pinned for Fix {
        Fix::Shape => "keep the Crates table in ARCHITECTURE.md as one pipe table with the columns \
             Crate | Owns | May depend on | Never depends on | Status: xtask/src/checks/crates_table.rs \
             reads it"
            .to_owned(),
        Fix::AfterTable { end, line } => format!(
            "remove the blank lines between line {end} and line {line}, so the row joins the table, \
             or move the text at line {line} out of the `## Crates` section: a blank line ends a table"
        ),
        Fix::NoRows => "add a row for every crate that exists".to_owned(),
        Fix::StatusWord => "write `exists` or `planned` in the Status column".to_owned(),
        Fix::CrateName => {
            "name the crate as its directory under crates/: lowercase letters, digits and _".to_owned()
        },
        Fix::OneRow => "keep one row per crate".to_owned(),
        Fix::StdModule { entry } => format!(
            "write `any I/O` (or `fs` or `process`) instead of `{entry}`: each bans the standard \
             library's file, network, process, environment and clock APIs; `clock` alone bans only \
             Instant::now and SystemTime::now"
        ),
        Fix::NotAName => "write fs, process, any I/O, clock, a crate of the table or a crates.io \
             package name (as `<name> (crates.io)` to mark it as one), separated by commas"
            .to_owned(),
        Fix::IoWordCase { exact, entry } => match entry {
            Some(entry) => format!(
                "write `{exact}` to ban the standard library's APIs it names, or `{entry} (crates.io)` \
                 if you mean the crates.io package"
            ),
            None => format!("write `{exact}` to ban the standard library's APIs it names"),
        },
        Fix::TypoBan {
            ban,
            package,
            entry,
        } => format!(
            "write `{ban}` to ban our crate `{package}`, or `{entry} (crates.io)` if you mean the \
             crates.io package: the module doc of xtask/src/checks/crates_table.rs lists which names \
             read as ours"
        ),
        Fix::Underscore { dashed } => format!(
            "write `{dashed}` for the crates.io package (this check reads `-` and `_` the same), or \
             the row name of the crate you mean if it is ours"
        ),
        Fix::OwnPackage {
            package,
            row,
            entry,
        } => {
            let or_row = row
                .as_ref()
                .map_or(String::new(), |row| format!(" (or the row name `{row}`)"));
            format!(
                "write `{package}`{or_row} to ban our crate, or `{entry} (crates.io)` if you mean the \
                 crates.io package"
            )
        },
        Fix::NoRowName { entry } => format!(
            "write the name that bans the crate you mean (a name in the Crate column), or \
             `{entry} (crates.io)` if you mean the crates.io package"
        ),
        Fix::Ban {
            ban,
            package,
            entry,
        } => format!(
            "write `{ban}` to ban our crate `{package}`, or `{entry} (crates.io)` if you mean the \
             crates.io package"
        ),
        Fix::NoRow { near, row, entry } => {
            let hint = match near.as_slice() {
                [] => "a name in the Crate column".to_owned(),
                names => format!(
                    "perhaps {}",
                    names
                        .iter()
                        .map(|name| format!("`{name}`"))
                        .collect::<Vec<_>>()
                        .join(" or ")
                ),
            };
            format!(
                "write the name that bans the crate you mean ({hint}), add a row `{row}` for the \
                 crate, or write `{entry} (crates.io)` if you mean the crates.io package"
            )
        },
    }
}

#[test]
fn every_fix_prints_its_pinned_text() {
    let samples = [
        Fix::Shape,
        Fix::AfterTable { end: 9, line: 12 },
        Fix::NoRows,
        Fix::StatusWord,
        Fix::CrateName,
        Fix::OneRow,
        Fix::StdModule {
            entry: "net".to_owned(),
        },
        Fix::NotAName,
        Fix::IoWordCase {
            exact: "fs",
            entry: Some("FS".to_owned()),
        },
        Fix::TypoBan {
            ban: "mcp".to_owned(),
            package: "clauseo-mcp".to_owned(),
            entry: "Mcp".to_owned(),
        },
        Fix::Underscore {
            dashed: "serde-json".to_owned(),
        },
        Fix::OwnPackage {
            package: "clauseo-process".to_owned(),
            row: Some("process".to_owned()),
            entry: "clauseo_process".to_owned(),
        },
        Fix::NoRowName {
            entry: "clauseo-".to_owned(),
        },
        Fix::Ban {
            ban: "xtask".to_owned(),
            package: "xtask".to_owned(),
            entry: "clauseo-xtask".to_owned(),
        },
        Fix::NoRow {
            near: vec!["mcp".to_owned()],
            row: "mcpx".to_owned(),
            entry: "clauseo-mcpx".to_owned(),
        },
    ];
    assert_texts(&samples, pinned, ARMS);
}

#[test]
fn the_optional_parts_of_the_text_appear_only_when_they_apply() {
    // An I/O word in other case gets the crates.io marker only when it is a package name.
    assert_eq!(
        Fix::IoWordCase {
            exact: "any I/O",
            entry: None,
        }
        .to_string(),
        "write `any I/O` to ban the standard library's APIs it names"
    );
    // A package whose row name is not its ban spelling gets no `(or the row name ...)` part.
    assert_eq!(
        Fix::OwnPackage {
            package: "clauseo-transport-ws".to_owned(),
            row: None,
            entry: "clauseo_transport_ws".to_owned(),
        }
        .to_string(),
        "write `clauseo-transport-ws` to ban our crate, or `clauseo_transport_ws (crates.io)` if you \
         mean the crates.io package"
    );
    // With no near row, the hint is the Crate column, not a list of spellings.
    assert_eq!(
        Fix::NoRow {
            near: Vec::new(),
            row: "store".to_owned(),
            entry: "clauseo-store".to_owned(),
        }
        .to_string(),
        "write the name that bans the crate you mean (a name in the Crate column), add a row `store` \
         for the crate, or write `clauseo-store (crates.io)` if you mean the crates.io package"
    );
}
