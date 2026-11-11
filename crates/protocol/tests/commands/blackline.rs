//! `word_blackline` and its answer, with the wire names the app reads: `to` is a checkpoint, or
//! missing or `null` for the folder now, as for `CompareVersions`.

use clauseo_protocol::{
    BlacklineParagraph, BlacklineRun, HostQuery, ParagraphChange, QueryResult, RunKind,
    WordBlackline,
};

use super::fixtures::{checkpoint_id, matter_id, path, round_trips, v7_text};

#[test]
fn word_blackline_to_is_a_checkpoint_or_the_folder_now() {
    let (matter, from, to) = (matter_id(1), checkpoint_id(2), checkpoint_id(3));
    let with_to = HostQuery::WordBlackline {
        matter,
        path: path("Lease.docx"),
        from,
        to: Some(to),
    };
    let folder_now = HostQuery::WordBlackline {
        matter,
        path: path("Lease.docx"),
        from,
        to: None,
    };
    round_trips(&with_to);
    round_trips(&folder_now);

    let text = |to: &str| {
        format!(
            r#"{{"word_blackline":{{"matter":"{}","path":"Lease.docx","from":"{}"{to}}}}}"#,
            v7_text(1),
            v7_text(2)
        )
    };
    // A `None` is not written at all.
    assert_eq!(serde_json::to_string(&folder_now).unwrap(), text(""));
    let parsed = |json: &str| serde_json::from_str::<HostQuery>(json).unwrap();
    assert_eq!(
        parsed(&text(&format!(r#","to":"{}""#, v7_text(3)))),
        with_to
    );
    assert_eq!(parsed(&text(r#","to":null"#)), folder_now);
    assert_eq!(parsed(&text("")), folder_now);
    assert!(
        serde_json::from_str::<HostQuery>(&text(r#","to":"not-an-id""#)).is_err(),
        "a `to` that is not an id"
    );
    // A field it does not know is refused, as in every query, and so is a path no matter holds.
    assert!(serde_json::from_str::<HostQuery>(&text(r#","x":1"#)).is_err());
    let outside = text("").replace("Lease.docx", "../Lease.docx");
    assert!(serde_json::from_str::<HostQuery>(&outside).is_err());
}

#[test]
fn a_word_blackline_answer_on_the_wire() {
    let answer = QueryResult::WordBlackline(WordBlackline {
        paragraphs: vec![
            BlacklineParagraph {
                change: ParagraphChange::Changed,
                runs: vec![
                    run(RunKind::Same, "Rent of $1,"),
                    run(RunKind::Del, "000"),
                    run(RunKind::Ins, "200"),
                ],
            },
            BlacklineParagraph {
                change: ParagraphChange::Same,
                runs: Vec::new(),
            },
        ],
        truncated: false,
    });
    round_trips(&answer);
    assert_eq!(
        serde_json::to_string(&answer).unwrap(),
        concat!(
            r#"{"word_blackline":{"paragraphs":[{"change":"changed","runs":["#,
            r#"{"kind":"same","text":"Rent of $1,"},{"kind":"del","text":"000"},"#,
            r#"{"kind":"ins","text":"200"}]},{"change":"same","runs":[]}],"truncated":false}}"#
        )
    );
    // A later field still decodes, and a later kind of run or change reads as `unknown`.
    let later = concat!(
        r#"{"word_blackline":{"paragraphs":[{"change":"moved","runs":["#,
        r#"{"kind":"moved_in","text":"x"}]}],"truncated":true,"pages":2}}"#
    );
    let QueryResult::WordBlackline(read) = serde_json::from_str::<QueryResult>(later).unwrap()
    else {
        panic!("not a word_blackline answer");
    };
    assert!(read.truncated);
    assert_eq!(read.paragraphs[0].change, ParagraphChange::Unknown);
    assert_eq!(read.paragraphs[0].runs[0].kind, RunKind::Unknown);
}

/// A run of `kind` with `text`.
fn run(kind: RunKind, text: &str) -> BlacklineRun {
    BlacklineRun {
        kind,
        text: text.to_owned(),
    }
}
