//! The shape of `clauseo-engines/1`: fields, format, seq, generated, apps and sizes.

use std::collections::BTreeSet;

use clauseo_engines::Error;

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

/// The (agent, os, arch) of each row of `SIZES` in `crates/agents/src/snapshot/embedded.rs`,
/// read from its text, `Os::Win32` as `win32` and so on. Only the combinations: the bump job
/// re-measures the bytes on every engine bump.
fn embedded_platforms() -> Vec<(String, String, String)> {
    let text = include_str!("../../../agents/src/snapshot/embedded.rs");
    let table = &text[text.find("const SIZES").unwrap()..];
    let table = &table[table.find("= [").unwrap() + 3..table.find("];").unwrap()];
    let flat: String = table.chars().filter(|c| !c.is_whitespace()).collect();
    flat.split("),")
        .map(|row| {
            row.trim_start_matches([',', '('])
                .trim_end_matches([')', ','])
        })
        .filter(|row| !row.is_empty())
        .map(|row| {
            let cells: Vec<&str> = row.split(',').collect();
            (
                cells[0].trim_matches('"').to_owned(),
                cells[1].trim_start_matches("Os::").to_lowercase(),
                cells[2].trim_start_matches("Arch::").to_lowercase(),
            )
        })
        .collect()
}

/// A value's serde name (`claude-acp`, `win32`, `arm64`).
fn name<T: serde::Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn the_sizes_rows_cover_the_embedded_snapshot_s_platforms() {
    let list = check(&GOOD).unwrap();
    let ours: BTreeSet<(String, String, String)> = list
        .sizes()
        .iter()
        .map(|row| (name(row.agent), name(row.os), name(row.arch)))
        .collect();
    let embedded = embedded_platforms();
    assert_eq!(embedded.len(), 12);
    assert_eq!(ours, embedded.into_iter().collect::<BTreeSet<_>>());
    // The bytes are the fixture's own, row for row.
    let raw: serde_json::Value = serde_json::from_slice(GOOD.json).unwrap();
    let raw = raw["sizes"].as_array().unwrap();
    assert_eq!(raw.len(), list.sizes().len());
    for (row, raw) in list.sizes().iter().zip(raw) {
        assert_eq!(raw["download"].as_u64(), Some(row.download));
        assert_eq!(raw["unpacked"].as_u64(), Some(row.unpacked));
        assert!(row.download > 0 && row.unpacked > 0);
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
