//! Tests for the Crates table reader: table text in a scratch dir, and the real `ARCHITECTURE.md`.

use super::*;
use crate::scratch::Scratch;

const HEADER: &str =
    "| Crate | Owns | May depend on | Never depends on | Status |\n|---|---|---|---|---|\n";

/// `ARCHITECTURE.md` text with a title, a `## Crates` section holding `rows` under the header
/// (the header on line 5), and a later section.
fn doc(rows: &str) -> String {
    format!("# Architecture\n\n## Crates\n\n{HEADER}{rows}\n## Seams\n\n| a | b |\n")
}

fn row(line: u32, name: &str, never: Vec<Never>, status: Status) -> CrateRow {
    CrateRow {
        line,
        name: name.to_owned(),
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
                vec![Never::External("tauri".into())],
                Status::Exists
            ),
            row(
                8,
                "agents",
                vec![Never::External("tauri".into())],
                Status::Planned
            ),
            row(
                9,
                "host",
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
fn every_io_word_holds_the_crate_to_core_io_and_a_crate_name_does_not() {
    let with = |never: Vec<Never>| row(1, "x", never, Status::Planned).holds_core_io();
    assert!(with(vec![Never::Io(IoWord::Fs)]));
    assert!(with(vec![Never::Io(IoWord::Process)]));
    assert!(with(vec![Never::Io(IoWord::AnyIo)]));
    assert!(with(vec![Never::Io(IoWord::Clock)]));
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
            Fix::StatusWord,
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
            Fix::Shape,
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
            Fix::Shape
        )])
    );
    assert_eq!(
        parse("# A\n\n## Crates\n\nprose\n\n## Seams\n\n| a |\n"),
        Err(vec![violation(
            FILE,
            3,
            "the `## Crates` section has no table",
            Fix::Shape
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
                Fix::Shape
            ),
            violation(
                FILE,
                9,
                "crate `desktop` has a second row (the first is at line 8)",
                Fix::OneRow,
            ),
            violation(
                FILE,
                9,
                "`no network` in the Never depends on column of `desktop` is neither an I/O word nor a crate name",
                Fix::NotAName,
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
            Fix::Shape
        )])
    );
    assert_eq!(
        parse(&doc("")),
        Err(vec![violation(
            FILE,
            5,
            "the Crates table has no rows",
            Fix::NoRows,
        )])
    );
}

#[test]
fn an_escaped_pipe_stays_in_its_cell_and_alignment_colons_are_a_separator() {
    let text = "## Crates\n\n| Crate | Owns | May depend on | Never depends on | Status |\n|:--|---|---|---|--:|\n| `x` | a \\| b | | | exists |\n";
    assert_eq!(parse(text).map(|table| table.rows.len()), Ok(1));
}

#[test]
fn load_reads_the_file_and_a_missing_file_has_no_location() {
    let dir = Scratch::new("crates-table-load");
    assert_eq!(load(&dir), Err(LoadError::Missing));
    assert_eq!(LoadError::Missing.into_violations(), Vec::new());
    Scratch::write(dir.join(FILE), doc("| `xtask` | things | | | exists |\n"))
        .expect("write the doc");
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

/// `std`, and each std module that core-io bans whole or in part (`core_io.rs`), fails.
#[test]
fn a_std_module_in_the_never_column_fails_at_its_row() {
    let text = doc("| `reducer` | things | | net, tauri, env, os, std | planned |\n");
    assert_eq!(
        parse(&text),
        Err(vec![
            violation(
                FILE,
                7,
                "`net` in the Never depends on column of `reducer` names a standard library module, not a crate, so nothing would enforce it",
                Fix::StdModule {
                    entry: "net".to_owned(),
                },
            ),
            violation(
                FILE,
                7,
                "`env` in the Never depends on column of `reducer` names a standard library module, not a crate, so nothing would enforce it",
                Fix::StdModule {
                    entry: "env".to_owned(),
                },
            ),
            violation(
                FILE,
                7,
                "`os` in the Never depends on column of `reducer` names a standard library module, not a crate, so nothing would enforce it",
                Fix::StdModule {
                    entry: "os".to_owned(),
                },
            ),
            violation(
                FILE,
                7,
                "`std` in the Never depends on column of `reducer` names a standard library module, not a crate, so nothing would enforce it",
                Fix::StdModule {
                    entry: "std".to_owned(),
                },
            ),
        ])
    );
}

/// `time` is a crates.io crate; core-io bans only two functions inside `std::time`.
#[test]
fn time_in_the_never_column_is_the_crates_io_crate() {
    let text = doc("| `reducer` | things | | time, fs | planned |\n");
    let table = parse(&text).expect("the table parses");
    assert_eq!(
        table.rows[0].never,
        vec![Never::External("time".into()), Never::Io(IoWord::Fs)]
    );
}

#[test]
fn a_row_without_a_leading_pipe_is_a_row_and_a_row_after_a_blank_line_fails() {
    let no_pipe =
        doc("| `xtask` | things | | tauri | exists |\n`desktop` | things | | tokio | exists |\n");
    let table = parse(&no_pipe).expect("the table parses");
    assert_eq!(
        table.rows,
        vec![
            row(
                7,
                "xtask",
                vec![Never::External("tauri".into())],
                Status::Exists
            ),
            row(
                8,
                "desktop",
                vec![Never::External("tokio".into())],
                Status::Exists
            ),
        ]
    );
    let blank =
        doc("| `xtask` | things | | tauri | exists |\n\n| `desktop` | things | | | exists |\n");
    assert_eq!(
        parse(&blank),
        Err(vec![violation(
            FILE,
            9,
            "the Crates table ends at line 7, but line 9 after it starts with `|`, so a row there is not part of the table",
            Fix::AfterTable { end: 7, line: 9 },
        )])
    );
    let second =
        doc("| `xtask` | things | | tauri | exists |\n\n| a | b |\n|---|---|\n| c | d |\n");
    assert_eq!(
        parse(&second).map_err(|found| found.iter().map(|v| v.line).collect::<Vec<_>>()),
        Err(vec![9])
    );
    let prose = doc(
        "| `xtask` | things | | tauri | exists |\n\nUse `a | b` in shell; rows marked planned wait.\n",
    );
    assert_eq!(parse(&prose).map(|table| table.rows.len()), Ok(1));
}

#[test]
fn a_short_or_long_row_is_counted_as_written_not_as_gfm_pads_or_cuts_it() {
    let text =
        doc("| `xtask` | things | | tauri | exists | extra |\n| `desktop` | things | | exists |\n");
    assert_eq!(
        parse(&text),
        Err(vec![
            violation(
                FILE,
                7,
                "this Crates table row has 6 cells, not 5",
                Fix::Shape
            ),
            violation(
                FILE,
                8,
                "this Crates table row has 4 cells, not 5",
                Fix::Shape
            ),
        ])
    );
}

#[test]
fn clauseo_and_a_row_name_bans_that_row_s_crate() {
    let text = doc(
        "| `host` | things | | clauseo-process, clauseo-transport-ws | planned |\n\
         | `process` | things | | | planned |\n\
         | `transport_ws` | things | | | planned |\n",
    );
    let table = parse(&text).expect("the table parses");
    assert_eq!(
        table.row("host").map(|row| row.never.clone()),
        Some(vec![
            Never::Workspace("process".into()),
            Never::Workspace("transport_ws".into()),
        ])
    );
}

#[test]
fn a_row_name_written_with_a_dash_is_rejected_as_ours() {
    let text = doc("| `host` | things | | transport-ws | planned |\n\
         | `transport_ws` | things | | | planned |\n");
    assert_eq!(
        parse(&text),
        Err(vec![violation(
            FILE,
            7,
            "`transport-ws` in the Never depends on column of `host` is the row `transport_ws` written with `-` for `_`, and as written it would ban a crates.io package `transport-ws` instead of our `clauseo-transport_ws`",
            Fix::TypoBan {
                ban: "transport_ws".to_owned(),
                package: "clauseo-transport_ws".to_owned(),
                entry: "transport-ws".to_owned(),
            },
        )])
    );
}

#[test]
fn one_edit_means_one_insert_delete_or_replace() {
    use super::names::within_one_edit;
    assert!(within_one_edit("agents", "agents"));
    assert!(within_one_edit("agent", "agents"));
    assert!(within_one_edit("agentss", "agents"));
    assert!(within_one_edit("agbnts", "agents"));
    assert!(within_one_edit("gents", "agents"));
    assert!(within_one_edit("agnets", "agents"));
    assert!(within_one_edit("gaents", "agents"));
    assert!(within_one_edit("agenst", "agents"));
    assert!(!within_one_edit("agnet", "agents"));
    assert!(!within_one_edit("gaenst", "agents"));
    assert!(!within_one_edit("tauri", "agents"));
    assert!(!within_one_edit("tokio", "acp"));
}

#[test]
fn a_ban_names_one_package_so_tauri_is_only_tauri() {
    let text = doc("| `xtask` | things | | tauri | exists |\n");
    let table = parse(&text).expect("the table parses");
    let row = table.row("xtask").expect("the row is there");
    let banned: Vec<String> = row.never.iter().filter_map(Never::package).collect();
    assert_eq!(banned, ["tauri"]);
}

#[test]
fn a_crates_io_marker_names_the_package_whatever_the_look_alike_rules_say() {
    let text = doc(
        "| `host` | things | | rmcp (crates.io), agents (crates.io), serde_json (crates.io) | planned |\n\
         | `mcp` | things | | | planned |\n\
         | `agents` | things | | | planned |\n",
    );
    let table = parse(&text).expect("the table parses");
    assert_eq!(
        table.row("host").map(|row| row.never.clone()),
        Some(vec![
            Never::External("rmcp".into()),
            Never::External("agents".into()),
            Never::External("serde_json".into()),
        ])
    );
    let bad = doc("| `host` | things | | not a name (crates.io) | planned |\n");
    assert_eq!(
        parse(&bad).map_err(|found| found.iter().map(|v| v.message.clone()).collect::<Vec<_>>()),
        Err(vec![
            "`not a name (crates.io)` in the Never depends on column of `host` is neither an I/O word nor a crate name".to_owned()
        ])
    );
}

#[test]
fn a_look_alike_of_a_row_named_like_an_io_word_is_told_to_write_its_package() {
    let text = doc(
        "| `host` | things | | clauseo-procesz, Clauseo-Process | planned |\n\
         | `process` | things | | | planned |\n",
    );
    let at = |what: &str, fix: Fix| violation(FILE, 7, what, fix);
    assert_eq!(
        parse(&text),
        Err(vec![
            at(
                "`clauseo-procesz` in the Never depends on column of `host` starts with `clauseo-`, but `procesz` is no row of the Crates table, so it names none of our crates",
                Fix::NoRow {
                    near: vec!["clauseo-process".to_owned()],
                    row: "procesz".to_owned(),
                    entry: "clauseo-procesz".to_owned(),
                },
            ),
            at(
                "`Clauseo-Process` in the Never depends on column of `host` is the package `clauseo-process` written with other letter case",
                Fix::OwnPackage {
                    package: "clauseo-process".to_owned(),
                    row: None,
                    entry: "Clauseo-Process".to_owned(),
                },
            ),
        ])
    );
}

#[test]
fn a_row_package_written_with_an_underscore_after_clauseo_is_told_to_write_the_package() {
    let text = doc(
        "| `host` | things | | clauseo_process, Clauseo_Process | planned |\n\
         | `process` | things | | | planned |\n",
    );
    let fix = |entry: &str| Fix::OwnPackage {
        package: "clauseo-process".to_owned(),
        row: None,
        entry: entry.to_owned(),
    };
    assert_eq!(
        parse(&text),
        Err(vec![
            violation(
                FILE,
                7,
                "`clauseo_process` in the Never depends on column of `host` is the package `clauseo-process` written with `_` after `clauseo`",
                fix("clauseo_process"),
            ),
            violation(
                FILE,
                7,
                "`Clauseo_Process` in the Never depends on column of `host` is the package `clauseo-process` written with other letter case and `_` after `clauseo`",
                fix("Clauseo_Process"),
            ),
        ])
    );
}
