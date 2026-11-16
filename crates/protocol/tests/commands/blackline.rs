//! `word_blackline` and its answer, with the wire names the app reads: `to` is a checkpoint, or
//! missing or `null` for the folder now, as for `CompareVersions`.

use clauseo_protocol::{
    BlacklineNotice, BlacklineObject, BlacklineParagraph, BlacklinePart, BlacklinePartKind,
    BlacklineRun, ChangedTable, HeaderSlot, HeaderUse, HostQuery, ParagraphChange, QueryResult,
    RunKind, TableFormat, TablePlace, WordBlackline,
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
                hidden: false,
            },
            BlacklineParagraph {
                change: ParagraphChange::Same,
                runs: Vec::new(),
                hidden: false,
            },
        ],
        changed_links: Vec::new(),
        truncated: false,
        notices: Vec::new(),
        parts: Vec::new(),
        changed_tables: Vec::new(),
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

/// The notices go on the wire by their snake-case names, left out when there are none (above); a
/// notice from a newer Clauseo reads as `unknown`, and an answer without the field as none.
#[test]
fn a_word_blackline_answer_with_notices_on_the_wire() {
    let answer = QueryResult::WordBlackline(WordBlackline {
        paragraphs: Vec::new(),
        changed_links: Vec::new(),
        truncated: false,
        notices: vec![
            BlacklineNotice::PartNotShown,
            BlacklineNotice::HiddenText,
            BlacklineNotice::NumbersMayDiffer,
            BlacklineNotice::MayDiffer,
        ],
        parts: Vec::new(),
        changed_tables: Vec::new(),
    });
    round_trips(&answer);
    assert_eq!(
        serde_json::to_string(&answer).unwrap(),
        concat!(
            r#"{"word_blackline":{"paragraphs":[],"truncated":false,"notices":["#,
            r#""part_not_shown","hidden_text","numbers_may_differ","may_differ"]}}"#
        )
    );
    let later = r#"{"word_blackline":{"paragraphs":[],"truncated":false,"notices":["zz_later"]}}"#;
    let QueryResult::WordBlackline(read) = serde_json::from_str::<QueryResult>(later).unwrap()
    else {
        panic!("not a word_blackline answer");
    };
    assert_eq!(read.notices, [BlacklineNotice::Unknown]);
    let QueryResult::WordBlackline(none) = serde_json::from_str::<QueryResult>(
        r#"{"word_blackline":{"paragraphs":[],"truncated":false}}"#,
    )
    .unwrap() else {
        panic!("not a word_blackline answer");
    };
    assert_eq!(none.notices, []);
}

/// A paragraph that holds hidden text says so, a format change and a place not shown have
/// their own changes, and the outside-text notice its name; `hidden` false is left out.
#[test]
fn hidden_text_formatting_and_places_not_shown_on_the_wire() {
    let answer = QueryResult::WordBlackline(WordBlackline {
        paragraphs: vec![
            BlacklineParagraph {
                change: ParagraphChange::Formatted,
                runs: vec![run(RunKind::Same, "x")],
                hidden: true,
            },
            BlacklineParagraph {
                change: ParagraphChange::NotShown,
                runs: Vec::new(),
                hidden: false,
            },
        ],
        changed_links: Vec::new(),
        truncated: false,
        notices: vec![BlacklineNotice::ChangesOutsideText],
        parts: Vec::new(),
        changed_tables: Vec::new(),
    });
    round_trips(&answer);
    assert_eq!(
        serde_json::to_string(&answer).unwrap(),
        concat!(
            r#"{"word_blackline":{"paragraphs":[{"change":"formatted","runs":[{"kind":"same","#,
            r#""text":"x"}],"hidden":true},{"change":"not_shown","runs":[]}],"truncated":false,"#,
            r#""notices":["changes_outside_text"]}}"#
        )
    );
}

/// An inserted picture is a run with no text, its object named; a text run writes no
/// `object`, and a later kind of object reads as `unknown`.
#[test]
fn an_object_run_on_the_wire() {
    let answer = QueryResult::WordBlackline(WordBlackline {
        paragraphs: vec![BlacklineParagraph {
            change: ParagraphChange::Changed,
            runs: vec![
                run(RunKind::Same, "Signed: "),
                BlacklineRun {
                    kind: RunKind::Ins,
                    text: String::new(),
                    object: Some(BlacklineObject::Picture),
                },
            ],
            hidden: false,
        }],
        changed_links: Vec::new(),
        truncated: false,
        notices: Vec::new(),
        parts: Vec::new(),
        changed_tables: Vec::new(),
    });
    round_trips(&answer);
    assert_eq!(
        serde_json::to_string(&answer).unwrap(),
        concat!(
            r#"{"word_blackline":{"paragraphs":[{"change":"changed","runs":[{"kind":"same","#,
            r#""text":"Signed: "},{"kind":"ins","text":"","object":"picture"}]}],"#,
            r#""truncated":false}}"#
        )
    );
    let later = r#"{"kind":"del","text":"","object":"zz_later"}"#;
    let read: BlacklineRun = serde_json::from_str(later).unwrap();
    assert_eq!(read.object, Some(BlacklineObject::Unknown));
}

/// A footnote and a header that changed, and a table whose formatting changed, on the wire: each
/// part's kind is a one-key object, its slot and the table's formats snake-case names; an answer
/// without the fields reads as none, and a newer kind of part as `unknown`.
#[test]
fn parts_and_changed_tables_on_the_wire() {
    let changed = BlacklineParagraph {
        change: ParagraphChange::Changed,
        runs: vec![run(RunKind::Same, "Note "), run(RunKind::Ins, "two")],
        hidden: false,
    };
    let answer = QueryResult::WordBlackline(WordBlackline {
        paragraphs: Vec::new(),
        truncated: false,
        notices: vec![
            BlacklineNotice::HeaderOrFooterInOneVersion,
            BlacklineNotice::CellFormatChanged,
        ],
        changed_links: Vec::new(),
        parts: vec![
            BlacklinePart {
                kind: BlacklinePartKind::Footnote { number: 1 },
                paragraphs: vec![changed.clone()],
            },
            BlacklinePart {
                kind: BlacklinePartKind::Header {
                    uses: vec![HeaderUse {
                        slot: HeaderSlot::FirstPage,
                        sections: vec![1, 2],
                    }],
                },
                paragraphs: vec![changed],
            },
        ],
        changed_tables: vec![ChangedTable {
            place: TablePlace::Body,
            number: 2,
            formats: vec![TableFormat::Shading, TableFormat::MergedCells],
        }],
    });
    round_trips(&answer);
    let para = concat!(
        r#"{"change":"changed","runs":[{"kind":"same","text":"Note "},"#,
        r#"{"kind":"ins","text":"two"}]}"#
    );
    assert_eq!(
        serde_json::to_string(&answer).unwrap(),
        format!(
            concat!(
                r#"{{"word_blackline":{{"paragraphs":[],"truncated":false,"#,
                r#""notices":["header_or_footer_in_one_version","cell_format_changed"],"#,
                r#""parts":[{{"kind":{{"footnote":{{"number":1}}}},"paragraphs":[{para}]}},"#,
                r#"{{"kind":{{"header":{{"uses":[{{"slot":"first_page","sections":[1,2]}}]}}}},"#,
                r#""paragraphs":[{para}]}}],"#,
                r#""changed_tables":[{{"place":"body","number":2,"#,
                r#""formats":["shading","merged_cells"]}}]}}}}"#
            ),
            para = para
        )
    );
    let later = r#"{"kind":{"zz_later":{"x":1}},"paragraphs":[]}"#;
    let read: BlacklinePart = serde_json::from_str(later).unwrap();
    assert!(matches!(read.kind, BlacklinePartKind::Unknown { .. }));
    let QueryResult::WordBlackline(none) = serde_json::from_str::<QueryResult>(
        r#"{"word_blackline":{"paragraphs":[],"truncated":false}}"#,
    )
    .unwrap() else {
        panic!("not a word_blackline answer");
    };
    assert_eq!((none.parts.len(), none.changed_tables.len()), (0, 0));
}

/// A run of `kind` with `text`.
fn run(kind: RunKind, text: &str) -> BlacklineRun {
    BlacklineRun {
        kind,
        text: text.to_owned(),
        object: None,
    }
}
