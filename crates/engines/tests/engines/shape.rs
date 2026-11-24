//! The shape of `clauseo-engines/1`: fields, format, seq, generated, apps and sizes.

use clauseo_engines::{Agent, Arch, Error, Os};

use crate::fixtures::{
    BAD_GENERATED, BAD_SIZE, DUP_SIZE, GOOD, NO_APPS, SEQ_ZERO, UNKNOWN_FIELD, WRONG_FORMAT, check,
    refusal,
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
fn an_unknown_os_in_sizes_is_refused() {
    // serde refuses the row while reading it, so it is a shape error.
    assert_eq!(refusal(&BAD_SIZE), Some(Error::Json));
}

#[test]
fn a_repeated_sizes_row_is_refused() {
    assert_eq!(refusal(&DUP_SIZE), Some(Error::DuplicateSize));
}

#[test]
fn the_sizes_rows_mirror_the_embedded_snapshot() {
    let list = check(&GOOD).unwrap();
    let first = list.sizes()[0];
    assert_eq!(
        (
            first.agent,
            first.os,
            first.arch,
            first.download,
            first.unpacked
        ),
        (
            Agent::ClaudeAcp,
            Os::Darwin,
            Arch::Arm64,
            112_272_099,
            284_447_720
        )
    );
    for agent in [Agent::ClaudeAcp, Agent::CodexAcp] {
        let rows = list.sizes().iter().filter(|row| row.agent == agent).count();
        assert_eq!(rows, 6);
    }
}
