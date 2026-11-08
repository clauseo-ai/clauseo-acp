//! The frozen files R18 (denials and widening) added, with their pins: its nine kinds and its
//! three error codes. Apart from `added_rows.rs` so that file stays under the line limit; a
//! later addition's rows go here, in the order the writer prints them.

use super::Corpus;

/// R18's frozen files, read after [`super::ADDED_AFTER_V1`] and in the same form.
pub(crate) const ADDED_IN_R18: &[Corpus] = &[
    (
        "added/denial_recorded",
        include_bytes!("added/denial_recorded.jsonl"),
        include_bytes!("added/denial_recorded.msgpack"),
        (1893, 0xd5b9_03d3_2f7b_91f2),
        (1278, 0x516a_193a_65a7_a2be),
    ),
    (
        "added/escalation_answered",
        include_bytes!("added/escalation_answered.jsonl"),
        include_bytes!("added/escalation_answered.msgpack"),
        (1441, 0xed56_e885_b0dc_604d),
        (966, 0x218f_44c9_e2db_8d86),
    ),
    (
        "added/denial_dismissed",
        include_bytes!("added/denial_dismissed.jsonl"),
        include_bytes!("added/denial_dismissed.msgpack"),
        (284, 0x49d5_d59d_77a2_f4f2),
        (193, 0xa409_556e_a8e6_c6d3),
    ),
    (
        "added/denials_paused",
        include_bytes!("added/denials_paused.jsonl"),
        include_bytes!("added/denials_paused.msgpack"),
        (292, 0xb300_7f57_9847_58d5),
        (198, 0x91a1_b209_8d5a_b6ac),
    ),
    (
        "added/allowance_recorded",
        include_bytes!("added/allowance_recorded.jsonl"),
        include_bytes!("added/allowance_recorded.msgpack"),
        (1585, 0xc161_dee4_0a08_8c24),
        (1109, 0x1d72_bac7_f793_cf32),
    ),
    (
        "added/allowance_forgotten",
        include_bytes!("added/allowance_forgotten.jsonl"),
        include_bytes!("added/allowance_forgotten.msgpack"),
        (290, 0x386f_fce6_27a1_c193),
        (199, 0xfa4c_84ae_ff1b_107c),
    ),
    (
        "added/read_granted_to",
        include_bytes!("added/read_granted_to.jsonl"),
        include_bytes!("added/read_granted_to.msgpack"),
        (362, 0x395d_c4af_63f4_e18f),
        (244, 0x5896_a4de_f07b_23ec),
    ),
    (
        "added/read_grant_forgotten",
        include_bytes!("added/read_grant_forgotten.jsonl"),
        include_bytes!("added/read_grant_forgotten.msgpack"),
        (342, 0xd722_fb0c_61ff_6af9),
        (228, 0xc974_ffd1_a7b6_3fdb),
    ),
    (
        "added/protection_switched",
        include_bytes!("added/protection_switched.jsonl"),
        include_bytes!("added/protection_switched.msgpack"),
        (250, 0x6940_adde_8839_8448),
        (176, 0xb7ec_8085_ac9d_2e4e),
    ),
    (
        "added/turn_failed-EscalationNotPending",
        include_bytes!("added/turn_failed-EscalationNotPending.jsonl"),
        include_bytes!("added/turn_failed-EscalationNotPending.msgpack"),
        (359, 0xc056_dcc9_458f_b5f0),
        (261, 0x6c19_d222_6cec_b37d),
    ),
    (
        "added/turn_failed-EscalationNotOfferable",
        include_bytes!("added/turn_failed-EscalationNotOfferable.jsonl"),
        include_bytes!("added/turn_failed-EscalationNotOfferable.msgpack"),
        (357, 0x9ee6_e4bb_21be_4365),
        (259, 0xde40_5bb3_8680_ed82),
    ),
    (
        "added/turn_failed-SandboxUnprotectedAutoForbidden",
        include_bytes!("added/turn_failed-SandboxUnprotectedAutoForbidden.jsonl"),
        include_bytes!("added/turn_failed-SandboxUnprotectedAutoForbidden.msgpack"),
        (372, 0x0955_bc86_460d_cb23),
        (275, 0x6848_0df3_5c6b_f996),
    ),
];
