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
    (
        "added/notice-OutsideMatterBlocked",
        include_bytes!("added/notice-OutsideMatterBlocked.jsonl"),
        include_bytes!("added/notice-OutsideMatterBlocked.msgpack"),
        (344, 0xf088_4dac_1e30_eab3),
        (265, 0x5a5c_03cc_1880_7737),
    ),
    (
        "added/turn_failed-McpNameEmpty",
        include_bytes!("added/turn_failed-McpNameEmpty.jsonl"),
        include_bytes!("added/turn_failed-McpNameEmpty.msgpack"),
        (335, 0xf26c_86a4_846c_ddb1),
        (236, 0x355f_a3b1_2e79_ebeb),
    ),
    (
        "added/turn_failed-McpNameTaken",
        include_bytes!("added/turn_failed-McpNameTaken.jsonl"),
        include_bytes!("added/turn_failed-McpNameTaken.msgpack"),
        (341, 0x6f94_988c_95fd_7750),
        (242, 0xc07f_196d_c92a_fb22),
    ),
    (
        "added/turn_failed-McpNameReserved",
        include_bytes!("added/turn_failed-McpNameReserved.jsonl"),
        include_bytes!("added/turn_failed-McpNameReserved.msgpack"),
        (355, 0x7927_f25a_c74b_26cf),
        (257, 0xc038_bd02_e9b2_55a4),
    ),
    (
        "added/turn_failed-McpNameInvalid",
        include_bytes!("added/turn_failed-McpNameInvalid.jsonl"),
        include_bytes!("added/turn_failed-McpNameInvalid.msgpack"),
        (352, 0x7c23_8abb_713a_54e8),
        (254, 0xd467_fb25_5194_9dae),
    ),
    (
        "added/turn_failed-McpUrlInvalid",
        include_bytes!("added/turn_failed-McpUrlInvalid.jsonl"),
        include_bytes!("added/turn_failed-McpUrlInvalid.msgpack"),
        (334, 0x8d2c_3784_e5d5_6fdb),
        (235, 0xda13_ad0c_82c5_bf98),
    ),
    (
        "added/turn_failed-McpHeaderNameInvalid",
        include_bytes!("added/turn_failed-McpHeaderNameInvalid.jsonl"),
        include_bytes!("added/turn_failed-McpHeaderNameInvalid.msgpack"),
        (350, 0x5e40_1449_3551_0493),
        (251, 0x52b3_1a89_ce7c_b308),
    ),
    (
        "added/turn_failed-McpEnvNameInvalid",
        include_bytes!("added/turn_failed-McpEnvNameInvalid.jsonl"),
        include_bytes!("added/turn_failed-McpEnvNameInvalid.msgpack"),
        (355, 0x8f58_f486_9100_4396),
        (257, 0xadcc_64b7_0c6d_b8bd),
    ),
    (
        "added/turn_failed-McpSecretStoreFailed",
        include_bytes!("added/turn_failed-McpSecretStoreFailed.jsonl"),
        include_bytes!("added/turn_failed-McpSecretStoreFailed.msgpack"),
        (352, 0x8636_5a1f_f4f5_939b),
        (253, 0x0a0d_ea9a_fc30_4252),
    ),
    (
        "added/turn_failed-McpServerUnknown",
        include_bytes!("added/turn_failed-McpServerUnknown.jsonl"),
        include_bytes!("added/turn_failed-McpServerUnknown.msgpack"),
        (339, 0x884d_6526_59cc_85c9),
        (240, 0xeaaf_b399_9240_6147),
    ),
    (
        "added/turn_failed-McpCommandInvalid",
        include_bytes!("added/turn_failed-McpCommandInvalid.jsonl"),
        include_bytes!("added/turn_failed-McpCommandInvalid.msgpack"),
        (348, 0x54dc_8a60_e732_4456),
        (249, 0x20ca_170a_1fa0_fe7d),
    ),
    (
        "added/turn_failed-McpCommandNotFound",
        include_bytes!("added/turn_failed-McpCommandNotFound.jsonl"),
        include_bytes!("added/turn_failed-McpCommandNotFound.msgpack"),
        (355, 0xa6de_7124_a2b6_5cc4),
        (257, 0x2d3f_8cb9_08a6_543d),
    ),
];
