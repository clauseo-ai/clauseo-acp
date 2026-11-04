//! A folder's support for Codex's sandbox on the wire: `FolderSupportKind`'s `check_failed`.
//! Every kind's round trip, and an unknown kind read as `unknown`, are the enum's own test
//! (`open_unit_enum`).

use clauseo_protocol::FolderSupportKind;

use super::fixtures::round_trips;

#[test]
fn a_failed_check_is_check_failed_and_an_older_reader_takes_a_newer_kind_as_unknown() {
    assert_eq!(
        serde_json::to_string(&FolderSupportKind::CheckFailed).unwrap(),
        r#""check_failed""#
    );
    round_trips(&FolderSupportKind::CheckFailed);
    // What an app before `check_failed` does with it, as this one does with a newer kind.
    let newer: FolderSupportKind = serde_json::from_str(r#""fenced_later""#).unwrap();
    assert_eq!(newer, FolderSupportKind::Unknown);
}
