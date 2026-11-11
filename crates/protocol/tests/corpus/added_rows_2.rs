//! The frozen files added after v1, continued: `added_rows.rs` holds the first ones and reached the
//! line limit, so new pairs are listed here, at the end of [`ADDED_AFTER_V1_2`], in the same form.

use super::Corpus;

/// What was added after v1 once [`super::added_rows::ADDED_AFTER_V1`] was full, in the same form:
/// `("added/<name>", include_bytes!(..jsonl), include_bytes!(..msgpack), (length, hash), (length,
/// hash))`. New pairs go at the end.
pub(crate) const ADDED_AFTER_V1_2: &[Corpus] = &[
    (
        "added/turn_failed-MatterFolderInClauseoData",
        include_bytes!("added/turn_failed-MatterFolderInClauseoData.jsonl"),
        include_bytes!("added/turn_failed-MatterFolderInClauseoData.msgpack"),
        (356, 0xafc1_f543_703a_fe8b),
        (257, 0x0afe_b885_7f63_59de),
    ),
    (
        "added/turn_failed-MatterFolderInsideMatter",
        include_bytes!("added/turn_failed-MatterFolderInsideMatter.jsonl"),
        include_bytes!("added/turn_failed-MatterFolderInsideMatter.msgpack"),
        (356, 0x8578_f49b_e8e6_bef4),
        (257, 0x7cac_7fd8_3d0d_5fd3),
    ),
    (
        "added/turn_failed-MatterFolderHoldsMatter",
        include_bytes!("added/turn_failed-MatterFolderHoldsMatter.jsonl"),
        include_bytes!("added/turn_failed-MatterFolderHoldsMatter.msgpack"),
        (354, 0x9867_94c3_8ae8_b4a2),
        (255, 0xb5a5_fcdc_8f39_3357),
    ),
];
