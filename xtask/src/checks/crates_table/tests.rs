//! Tests for the Crates table reader: table text in a scratch dir, and the real `ARCHITECTURE.md`.

use super::*;
use crate::scratch::Scratch;
use std::fs;

const HEADER: &str =
    "| Crate | Owns | May depend on | Never depends on | Status |\n|---|---|---|---|---|\n";

/// `ARCHITECTURE.md` text with a title, a `## Crates` section holding `rows` under the header
/// (the header on line 5), and a later section.
fn doc(rows: &str) -> String {
    format!("# Architecture\n\n## Crates\n\n{HEADER}{rows}\n## Seams\n\n| a | b |\n")
}

fn row(line: u32, name: &str, may: &[&str], never: Vec<Never>, status: Status) -> CrateRow {
    CrateRow {
        line,
        name: name.to_owned(),
        owns: "things".to_owned(),
        may_depend_on: may.iter().map(|entry| (*entry).to_owned()).collect(),
        never,
        status,
    }
}

#[test]
fn rows_keep_their_line_and_lose_their_backticks() {
    let text = doc("| `xtask` | things | none of ours | tauri | exists |\n\
         | `agents` | things | `protocol`, process | tauri | planned |\n\
         | `host` | things | | tauri, tokio, agents, process, any I/O, clock | planned |\n");
    let table = parse(&text).expect("the table parses");
    assert_eq!(
        table.rows,
        vec![
            row(
                7,
                "xtask",
                &["none of ours"],
                vec![Never::External("tauri".into())],
                Status::Exists
            ),
            row(
                8,
                "agents",
                &["protocol", "process"],
                vec![Never::External("tauri".into())],
                Status::Planned
            ),
            row(
                9,
                "host",
                &[],
                vec![
                    Never::External("tauri".into()),
                    Never::External("tokio".into()),
                    Never::Workspace("agents".into()),
                    Never::Io(IoWord::Process),
                    Never::Io(IoWord::AnyIo),
                    Never::Io(IoWord::Clock),
                ],
                Status::Planned
            ),
        ]
    );
}

#[test]
fn a_row_of_the_table_bans_our_package_and_any_other_name_a_crates_io_one() {
    assert_eq!(
        Never::Workspace("agents".into()).package(),
        Some("clauseo-agents".to_owned())
    );
    assert_eq!(
        Never::External("tokio".into()).package(),
        Some("tokio".to_owned())
    );
    assert_eq!(Never::Io(IoWord::Fs).package(), None);
}

#[test]
fn core_io_is_fs_process_or_any_io_and_not_the_clock_alone() {
    let with = |never: Vec<Never>| row(1, "x", &[], never, Status::Planned).holds_core_io();
    assert!(with(vec![Never::Io(IoWord::Fs)]));
    assert!(with(vec![Never::Io(IoWord::Process)]));
    assert!(with(vec![Never::Io(IoWord::AnyIo)]));
    assert!(!with(vec![Never::Io(IoWord::Clock)]));
    assert!(!with(vec![Never::Workspace("process".into())]));
}

#[test]
fn a_package_maps_to_its_row() {
    let text = doc("| `xtask` | things | | | exists |\n\
         | `desktop` | things | | | exists |\n\
         | `fake_agent` | things | | | planned |\n");
    let table = parse(&text).expect("the table parses");
    let name = |package: &str| table.row_for_package(package).map(|row| row.name.as_str());
    assert_eq!(name("xtask"), Some("xtask"));
    assert_eq!(name("clauseo-desktop"), Some("desktop"));
    assert_eq!(name("clauseo-fake_agent"), Some("fake_agent"));
    assert_eq!(name("clauseo-fake-agent"), Some("fake_agent"));
    assert_eq!(name("desktop"), None);
    assert_eq!(name("clauseo-"), None);
}

#[test]
fn an_unknown_status_fails_at_its_line_naming_the_two_values() {
    let text = doc("| `xtask` | things | | tauri | done |\n");
    assert_eq!(
        parse(&text),
        Err(vec![violation(
            FILE,
            7,
            "crate `xtask` has status `done`",
            "write `exists` or `planned` in the Status column",
        )])
    );
}

#[test]
fn a_header_with_other_columns_fails_at_the_header_line() {
    let text = "## Crates\n\n| Crate | Owns | Status |\n|---|---|---|\n| `x` | y | exists |\n";
    assert_eq!(
        parse(text),
        Err(vec![violation(
            FILE,
            3,
            "the Crates table's columns are `Crate | Owns | Status`, not `Crate | Owns | May depend on | Never depends on | Status`",
            SHAPE_FIX,
        )])
    );
}

#[test]
fn a_missing_section_fails_at_line_one_and_a_section_without_a_table_at_its_heading() {
    assert_eq!(
        parse("# Architecture\n\n## Seams\n"),
        Err(vec![violation(
            FILE,
            1,
            "ARCHITECTURE.md has no `## Crates` section",
            SHAPE_FIX
        )])
    );
    assert_eq!(
        parse("# A\n\n## Crates\n\nprose\n\n## Seams\n\n| a |\n"),
        Err(vec![violation(
            FILE,
            3,
            "the `## Crates` section has no table",
            SHAPE_FIX
        )])
    );
}

#[test]
fn a_short_row_a_repeated_row_and_a_bad_never_entry_fail_at_their_lines() {
    let text = doc("| `xtask` | things | tauri | exists |\n\
         | `desktop` | things | | | exists |\n\
         | `desktop` | things | | no network | exists |\n");
    assert_eq!(
        parse(&text),
        Err(vec![
            violation(
                FILE,
                7,
                "this Crates table row has 4 cells, not 5",
                SHAPE_FIX
            ),
            violation(
                FILE,
                9,
                "crate `desktop` has a second row (the first is at line 8)",
                "keep one row per crate"
            ),
            violation(
                FILE,
                9,
                "`no network` in the Never depends on column of `desktop` is neither an I/O word nor a crate name",
                "write fs, process, any I/O, clock, a crate of the table or a crates.io package name, separated by commas"
            ),
        ])
    );
}

#[test]
fn a_missing_separator_and_an_empty_table_fail() {
    let text = "## Crates\n\n| Crate | Owns | May depend on | Never depends on | Status |\n| `x` | y | | | exists |\n";
    assert_eq!(
        parse(text),
        Err(vec![violation(
            FILE,
            4,
            "the Crates table's header is not followed by its `|---|` separator line",
            SHAPE_FIX
        )])
    );
    assert_eq!(
        parse(&doc("")),
        Err(vec![violation(
            FILE,
            5,
            "the Crates table has no rows",
            "add a row for every crate that exists"
        )])
    );
}

#[test]
fn an_escaped_pipe_stays_in_its_cell_and_alignment_colons_are_a_separator() {
    let text = "## Crates\n\n| Crate | Owns | May depend on | Never depends on | Status |\n|:--|---|---|---|--:|\n| `x` | a \\| b | | | exists |\n";
    let table = parse(text).expect("the table parses");
    assert_eq!(table.rows[0].owns, "a | b");
}

#[test]
fn load_reads_the_file_and_a_missing_file_has_no_location() {
    let dir = Scratch::new("crates-table-load");
    assert_eq!(load(&dir), Err(LoadError::Missing));
    assert_eq!(LoadError::Missing.into_violations(), Vec::new());
    fs::write(dir.join(FILE), doc("| `xtask` | things | | | exists |\n")).expect("write the doc");
    assert_eq!(load(&dir).map(|table| table.rows.len()), Ok(1));
}

#[test]
fn the_real_architecture_md_parses() {
    let table = load(&crate::workspace::workspace_root()).expect("ARCHITECTURE.md parses");
    let status = |name: &str| table.row(name).map(|row| row.status);
    assert_eq!(status("xtask"), Some(Status::Exists));
    assert_eq!(status("desktop"), Some(Status::Exists));
    assert_eq!(status("protocol"), Some(Status::Planned));
    assert_eq!(
        table.row("protocol").map(|row| row.never.clone()),
        Some(vec![
            Never::External("tauri".into()),
            Never::External("tokio".into()),
            Never::Io(IoWord::Fs),
            Never::Io(IoWord::Process),
        ])
    );
    assert!(table.row("protocol").is_some_and(CrateRow::holds_core_io));
}
