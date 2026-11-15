//! `NoticeKind::AlwaysNotKept` and its `NotKeptReason`: each reason keeps its wire form, an
//! unknown one decodes to the catch-all.

use clauseo_protocol::{NotKeptReason, NoticeKind};

use super::unknown_variants::unit_enum;
use crate::fixtures::{decodes_in_both, round_trips};

#[test]
fn not_kept_reason() {
    let cases = [
        (NotKeptReason::NoRule, "no_rule"),
        (NotKeptReason::Refused, "refused"),
        (NotKeptReason::OutsideMatter, "outside_matter"),
        (NotKeptReason::Full, "full"),
    ];
    unit_enum(&NotKeptReason::ALL, &cases, NotKeptReason::Unknown);
}

#[test]
fn always_not_kept() {
    let kind = NoticeKind::AlwaysNotKept {
        reason: NotKeptReason::OutsideMatter,
    };
    decodes_in_both(r#"{"always_not_kept":{"reason":"outside_matter"}}"#, &kind);
    round_trips(&kind);
}
