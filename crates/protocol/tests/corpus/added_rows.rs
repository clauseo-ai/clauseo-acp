//! The frozen files added after v1 (see `corpus/v1/README.md`), with their pins. Kept apart from
//! `corpus.rs` so that file stays under the line limit.

use super::Corpus;

/// What was added after v1, each with its own frozen entries: a new kind in
/// `corpus/added/<kind>.jsonl` and `.msgpack`, a new variant of an open enum (a unit value too)
/// in `corpus/added/<kind>-<variant>.jsonl` and `.msgpack`, `<kind>` being the event that holds it.
/// Each is written `("added/<name>", include_bytes!(..jsonl), include_bytes!(..msgpack),
/// (length, hash), (length, hash))`.
pub(crate) const ADDED_AFTER_V1: &[Corpus] = &[
    (
        "added/turn_failed-SettingsLocked",
        include_bytes!("added/turn_failed-SettingsLocked.jsonl"),
        include_bytes!("added/turn_failed-SettingsLocked.msgpack"),
        (339, 0x49bd_0daf_8ff8_f75b),
        (240, 0x12c1_6072_ca6b_e315),
    ),
    (
        "added/matter_relocated",
        include_bytes!("added/matter_relocated.jsonl"),
        include_bytes!("added/matter_relocated.msgpack"),
        (291, 0xa3e2_fa0b_b295_75ad),
        (220, 0x14ba_06d0_9664_8de4),
    ),
    (
        "added/turn_failed-MatterCodexFolder",
        include_bytes!("added/turn_failed-MatterCodexFolder.jsonl"),
        include_bytes!("added/turn_failed-MatterCodexFolder.msgpack"),
        (357, 0x48d1_4ed4_b6a4_4245),
        (259, 0x5cce_ad89_d5a3_ae0a),
    ),
    (
        "added/turn_failed-MatterFolderIsDrive",
        include_bytes!("added/turn_failed-MatterFolderIsDrive.jsonl"),
        include_bytes!("added/turn_failed-MatterFolderIsDrive.msgpack"),
        (338, 0xea5f_d047_8374_8993),
        (239, 0xacde_0a37_dec2_baec),
    ),
    (
        "added/turn_failed-MatterFolderIsHome",
        include_bytes!("added/turn_failed-MatterFolderIsHome.jsonl"),
        include_bytes!("added/turn_failed-MatterFolderIsHome.msgpack"),
        (335, 0x528e_9e42_e8e0_c558),
        (236, 0x74f7_b746_a02c_c194),
    ),
    (
        "added/turn_failed-MatterFolderIsSystem",
        include_bytes!("added/turn_failed-MatterFolderIsSystem.jsonl"),
        include_bytes!("added/turn_failed-MatterFolderIsSystem.msgpack"),
        (345, 0xfd09_5ac3_9878_a575),
        (246, 0xe721_7be2_4850_b119),
    ),
    (
        "added/turn_failed-MatterFolderUnreadable",
        include_bytes!("added/turn_failed-MatterFolderUnreadable.jsonl"),
        include_bytes!("added/turn_failed-MatterFolderUnreadable.msgpack"),
        (340, 0x609d_fd63_1e91_303d),
        (241, 0x4307_7d36_35a9_497c),
    ),
    (
        "added/turn_failed-MatterFolderIsMatter",
        include_bytes!("added/turn_failed-MatterFolderIsMatter.jsonl"),
        include_bytes!("added/turn_failed-MatterFolderIsMatter.msgpack"),
        (338, 0xc535_6d66_b7c6_d00a),
        (239, 0x6530_f87b_c94e_023b),
    ),
    (
        "added/turn_failed-MatterFolderInMattersRoot",
        include_bytes!("added/turn_failed-MatterFolderInMattersRoot.jsonl"),
        include_bytes!("added/turn_failed-MatterFolderInMattersRoot.msgpack"),
        (350, 0x0485_5c65_a3c5_28ee),
        (251, 0xc4fe_70c7_6452_5b8b),
    ),
    (
        "added/agent_named",
        include_bytes!("added/agent_named.jsonl"),
        include_bytes!("added/agent_named.msgpack"),
        (257, 0xe029_cb0b_9067_8a4b),
        (185, 0x151e_09ed_77c7_f901),
    ),
    (
        "added/web_search_recorded",
        include_bytes!("added/web_search_recorded.jsonl"),
        include_bytes!("added/web_search_recorded.msgpack"),
        (496, 0x7e01_f895_a8da_d14a),
        (364, 0x324f_cb9c_5039_62f9),
    ),
    (
        "added/page_visited",
        include_bytes!("added/page_visited.jsonl"),
        include_bytes!("added/page_visited.msgpack"),
        (1216, 0x651b_e7f0_6360_5b1d),
        (841, 0x6f55_1e6f_f1bf_b7d4),
    ),
    (
        "added/page_visited-WebFetch",
        include_bytes!("added/page_visited-WebFetch.jsonl"),
        include_bytes!("added/page_visited-WebFetch.msgpack"),
        (381, 0x8413_c87b_98b4_09d8),
        (259, 0xfe7c_c644_eee3_f035),
    ),
    (
        "added/page_visited-CodexOpenPage",
        include_bytes!("added/page_visited-CodexOpenPage.jsonl"),
        include_bytes!("added/page_visited-CodexOpenPage.msgpack"),
        (408, 0xac6f_6660_a6b4_970c),
        (282, 0x8b7c_4722_1d76_5193),
    ),
    (
        "added/page_visited-Browser",
        include_bytes!("added/page_visited-Browser.jsonl"),
        include_bytes!("added/page_visited-Browser.msgpack"),
        (427, 0x2d0d_c9ce_c7b4_12a5),
        (302, 0x1c6c_22ee_7005_6792),
    ),
    (
        "added/thread_renamed-Placeholder",
        include_bytes!("added/thread_renamed-Placeholder.jsonl"),
        include_bytes!("added/thread_renamed-Placeholder.msgpack"),
        (278, 0xba76_e746_8ef1_0482),
        (202, 0x28ae_babd_4580_6e30),
    ),
];
