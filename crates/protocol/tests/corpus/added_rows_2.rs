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
    (
        "added/turn_failed-PromptTooLong",
        include_bytes!("added/turn_failed-PromptTooLong.jsonl"),
        include_bytes!("added/turn_failed-PromptTooLong.msgpack"),
        (397, 0xb8b2_3a69_7983_4b22),
        (299, 0xc180_6c5c_0edc_2d6b),
    ),
    (
        "added/turn_ended-TooLong",
        include_bytes!("added/turn_ended-TooLong.jsonl"),
        include_bytes!("added/turn_ended-TooLong.msgpack"),
        (352, 0x3b96_01ab_d792_59bb),
        (234, 0x9e7a_7ca8_dbba_7255),
    ),
];
