//! `preview_undo` and its answer, with the wire names the app reads.

use clauseo_protocol::{HostQuery, QueryResult, UndoPreview};

use super::fixtures::{path, round_trips, thread_id, turn_id};

#[test]
fn preview_undo_and_its_answer_on_the_wire() {
    let query = HostQuery::PreviewUndo {
        thread: thread_id(1),
        turn: turn_id(2),
    };
    round_trips(&query);
    assert_eq!(
        serde_json::to_string(&query).unwrap(),
        concat!(
            r#"{"preview_undo":{"thread":"01900000-0000-7000-8000-000000000001","#,
            r#""turn":"01900000-0000-7000-8000-000000000002"}}"#
        )
    );
    // A query with a field it does not know is refused, as every query is.
    let extra = concat!(
        r#"{"preview_undo":{"thread":"01900000-0000-7000-8000-000000000001","#,
        r#""turn":"01900000-0000-7000-8000-000000000002","x":1}}"#
    );
    assert!(serde_json::from_str::<HostQuery>(extra).is_err());
    // A missing turn is refused.
    let short = r#"{"preview_undo":{"thread":"01900000-0000-7000-8000-000000000001"}}"#;
    assert!(serde_json::from_str::<HostQuery>(short).is_err());

    let answer = QueryResult::UndoPreview(UndoPreview {
        edited_since: vec![path("notes.txt")],
    });
    round_trips(&answer);
    assert_eq!(
        serde_json::to_string(&answer).unwrap(),
        r#"{"undo_preview":{"edited_since":["notes.txt"]}}"#
    );
    let none = QueryResult::UndoPreview(UndoPreview {
        edited_since: Vec::new(),
    });
    assert_eq!(
        serde_json::to_string(&none).unwrap(),
        r#"{"undo_preview":{"edited_since":[]}}"#
    );
    // A later field (such as the files open in another program) still decodes here.
    let later = r#"{"undo_preview":{"edited_since":[],"locked":["a.docx"]}}"#;
    assert_eq!(serde_json::from_str::<QueryResult>(later).unwrap(), none);
}
