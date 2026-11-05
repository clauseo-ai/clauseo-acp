//! A folder's support for Codex's sandbox on the wire: `FolderSupportKind`'s `check_failed`,
//! and the `folder_support` query of a create's source with its answer. Every kind's round trip,
//! and an unknown kind read as `unknown`, are the enum's own test (`open_unit_enum`).

use clauseo_protocol::{
    FolderSupportKind, HostQuery, MatterSource, PickedFolderSupport, QueryResult,
};

use super::fixtures::{folder_ref_id, round_trips};

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

#[test]
fn the_folder_support_query_names_a_matter_source_and_answers_a_kind_or_nothing() {
    let folder = folder_ref_id(1);
    for (source, json) in [
        (
            MatterSource::ExistingFolder { folder },
            format!(
                r#"{{"folder_support":{{"source":{{"existing_folder":{{"folder":"{folder}"}}}}}}}}"#
            ),
        ),
        (
            MatterSource::NewFolder { folder },
            format!(
                r#"{{"folder_support":{{"source":{{"new_folder":{{"folder":"{folder}"}}}}}}}}"#
            ),
        ),
        (
            MatterSource::DefaultLocation,
            r#"{"folder_support":{"source":"default_location"}}"#.to_owned(),
        ),
    ] {
        let query = HostQuery::FolderSupport { source };
        assert_eq!(serde_json::to_string(&query).unwrap(), json);
        round_trips(&query);
    }
    let extra = r#"{"folder_support":{"source":"default_location","path":"/x"}}"#;
    assert!(serde_json::from_str::<HostQuery>(extra).is_err());
    let answer = |support| QueryResult::FolderSupport(PickedFolderSupport { support });
    for (answer, json) in [
        (
            answer(Some(FolderSupportKind::Network)),
            r#"{"folder_support":{"support":"network"}}"#,
        ),
        (
            answer(Some(FolderSupportKind::CheckFailed)),
            r#"{"folder_support":{"support":"check_failed"}}"#,
        ),
        // A fine folder leaves `support` out: the wire has no `null`s.
        (answer(None), r#"{"folder_support":{}}"#),
    ] {
        assert_eq!(serde_json::to_string(&answer).unwrap(), json);
        round_trips(&answer);
    }
    // A field from a newer host is ignored.
    let newer: QueryResult =
        serde_json::from_str(r#"{"folder_support":{"support":"network","x":1}}"#).unwrap();
    assert_eq!(newer, answer(Some(FolderSupportKind::Network)));
}
