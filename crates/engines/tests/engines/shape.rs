//! The shape of `clauseo-engines/1`: fields, format, seq, generated, apps and sizes.

use clauseo_engines::{Agent, Error};

use crate::fixtures::{
    BAD_GENERATED, BAD_SIZE, DUP_SIZE, EMPTY_APP, GOOD, MISSING_SIZE, NO_APPS, SEQ_HUGE, SEQ_ZERO,
    UNKNOWN_FIELD, WRONG_FORMAT, ZERO_SIZE, check, refusal,
};

#[test]
fn an_unknown_top_level_field_is_refused() {
    assert_eq!(refusal(&UNKNOWN_FIELD), Some(Error::UnknownField));
}

#[test]
fn another_format_is_refused() {
    assert_eq!(refusal(&WRONG_FORMAT), Some(Error::Format));
}

#[test]
fn a_generated_time_not_in_rfc_3339_shape_is_refused() {
    assert_eq!(refusal(&BAD_GENERATED), Some(Error::Generated));
}

#[test]
fn empty_apps_are_refused() {
    assert_eq!(refusal(&NO_APPS), Some(Error::NoApps));
}

#[test]
fn seq_zero_is_refused() {
    assert_eq!(refusal(&SEQ_ZERO), Some(Error::Seq));
}

#[test]
fn a_seq_above_2_to_the_32_is_refused() {
    assert_eq!(clauseo_engines::MAX_SEQ, 1 << 32);
    assert_eq!(refusal(&SEQ_HUGE), Some(Error::Seq));
}

#[test]
fn an_unknown_os_in_sizes_is_refused() {
    // serde refuses the row while reading it, so it is a shape error.
    assert_eq!(refusal(&BAD_SIZE), Some(Error::Json));
}

#[test]
fn a_repeated_sizes_row_is_refused() {
    assert_eq!(refusal(&DUP_SIZE), Some(Error::DuplicateSize));
}

/// `SIZES` in `crates/agents/src/snapshot/embedded.rs`, read from its text: (agent, os, arch,
/// download, unpacked) per row, `Os::Win32` as `win32` and so on.
fn embedded_sizes() -> Vec<(String, String, String, u64, u64)> {
    let text = include_str!("../../../agents/src/snapshot/embedded.rs");
    let table = &text[text.find("const SIZES").unwrap()..];
    let table = &table[table.find("= [").unwrap() + 3..table.find("];").unwrap()];
    let flat: String = table.chars().filter(|c| !c.is_whitespace()).collect();
    flat.split("),")
        .map(|row| {
            row.trim_start_matches(',')
                .trim_start_matches('(')
                .trim_end_matches([')', ','])
        })
        .filter(|row| !row.is_empty())
        .map(|row| {
            let cells: Vec<&str> = row.split(',').collect();
            let number = |cell: &str| cell.replace('_', "").parse::<u64>().unwrap();
            (
                cells[0].trim_matches('"').to_owned(),
                cells[1].trim_start_matches("Os::").to_lowercase(),
                cells[2].trim_start_matches("Arch::").to_lowercase(),
                number(cells[3]),
                number(cells[4]),
            )
        })
        .collect()
}

#[test]
fn the_sizes_rows_mirror_the_embedded_snapshot() {
    let list = check(&GOOD).unwrap();
    let ours: Vec<(String, String, String, u64, u64)> = list
        .sizes()
        .iter()
        .map(|row| {
            let name = |value: serde_json::Value| value.as_str().unwrap().to_owned();
            (
                name(serde_json::to_value(row.agent).unwrap()),
                name(serde_json::to_value(row.os).unwrap()),
                name(serde_json::to_value(row.arch).unwrap()),
                row.download,
                row.unpacked,
            )
        })
        .collect();
    let embedded = embedded_sizes();
    assert_eq!(embedded.len(), 12);
    assert_eq!(ours, embedded);
    for agent in [Agent::ClaudeAcp, Agent::CodexAcp] {
        let rows = list.sizes().iter().filter(|row| row.agent == agent).count();
        assert_eq!(rows, 6);
    }
}

#[test]
fn an_empty_app_entry_is_refused() {
    assert_eq!(refusal(&EMPTY_APP), Some(Error::NoApps));
}

#[test]
fn a_zero_size_is_refused() {
    assert_eq!(refusal(&ZERO_SIZE), Some(Error::Size));
}

#[test]
fn sizes_missing_a_platform_are_refused_and_all_12_load() {
    assert_eq!(refusal(&MISSING_SIZE), Some(Error::SizesIncomplete));
    assert_eq!(check(&GOOD).unwrap().sizes().len(), 12);
}
