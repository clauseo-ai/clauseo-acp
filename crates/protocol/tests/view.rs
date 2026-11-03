//! View entries and patches: boxed large variants, entry ids, and how an entry's inline cost
//! counts; the folder scan's app event.

#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]

#[cfg(test)]
#[path = "support/fixtures.rs"]
pub mod fixtures;

#[cfg(test)]
#[path = "support/examples.rs"]
pub mod examples;

#[cfg(test)]
#[path = "support/entries.rs"]
pub mod entries;

use std::mem::size_of;

use clauseo_protocol::{
    AppendTarget, CompactionStatus, ContentBlock, Count, Cursor, DomainEvent, ElicitationEntry,
    ElicitationRequest, ElicitationState, EntryId, ExpiryReason, Finite, FolderPlace, FolderRefId,
    FolderScanned, HeaderEntry, HostEvent, INLINE_LIMIT_BYTES, LimitWindow, LimitWindowView,
    MAX_RAW_ENCODE_DEPTH, MarkerEntry, MarkerKind, MatterHeaderEntry, MatterKind, MeterTone,
    NoticeEntry, NoticeLevel, PatchBatch, RawFrom, RawPayload, StepEntry, StopReason, StreamHeader,
    StreamId, TextOrBlob, ThreadSummary, Timestamp, ToolCallContent, ToolCallEntry, ToolCallStatus,
    ToolKind, ToolLocation, ViewEntry, ViewEpoch, ViewPatch, ViewPatchBody, ViewWindow,
    WindowFreshness, WindowKind,
};
use clauseo_protocol::{ClauseoTool, ToolAvailability, ToolState, ToolsUnavailableReason};

use entries::header_entry;
use fixtures::{
    blob, checkpoint_id, elicitation_id, event_id, json_cost, matter_id, message_id, path,
    round_trips, seq, short, thread_id, tool_call_id, turn_id, v7,
};

fn inline(text: &str) -> TextOrBlob {
    TextOrBlob::Inline(text.to_owned())
}

#[cfg(test)]
fn tool_call() -> ToolCallEntry {
    ToolCallEntry {
        order: seq(5),
        tool_call: tool_call_id(3),
        turn: turn_id(2),
        title: short("Edit draft"),
        kind: ToolKind::Edit,
        tool_name: None,
        status: ToolCallStatus::Completed,
        locations: vec![ToolLocation::Inside {
            path: path("draft.docx"),
            line: None,
        }],
        raw_input: Some(inline(r#"{"path":"draft.docx","note":"line1\nline2"}"#)),
        raw_output: Some(inline("ok\t\"done\"")),
        content: vec![
            ToolCallContent::Text {
                text: inline("tab\there\u{1}"),
            },
            ToolCallContent::Content {
                content: ContentBlock::Image {
                    data: TextOrBlob::Blob(blob(9, 100_000)),
                    mime_type: short("image/png"),
                    uri: None,
                },
            },
            ToolCallContent::Diff {
                path: ToolLocation::Inside {
                    path: path("draft.docx"),
                    line: None,
                },
                old: Some(inline("before \\ slash")),
                new: inline("after é"),
            },
            ToolCallContent::Content {
                content: ContentBlock::Resource {
                    uri: "file:///x".to_owned(),
                    mime_type: None,
                    text: Some(inline("resource\r\n")),
                    blob: Some(TextOrBlob::Blob(blob(8, 50))),
                },
            },
            ToolCallContent::Content {
                content: ContentBlock::Unsupported {
                    kind: "later".to_owned(),
                    raw_from: RawFrom::Acp,
                    raw: Some(inline(r#"{"type":"later"}"#)),
                },
            },
        ],
        started_at: Timestamp::new(1_760_000_004_000).unwrap(),
        finished_at: None,
        background: None,
    }
}

#[test]
fn entry_inline_len_sums_vec_items() {
    let entry = ViewEntry::ToolCall(Box::new(tool_call()));
    let inline_parts = [
        r#"{"path":"draft.docx","note":"line1\nline2"}"#,
        "ok\t\"done\"",
        "tab\there\u{1}",
        "before \\ slash",
        "after é",
        "resource\r\n",
        r#"{"type":"later"}"#,
    ];
    let expected: usize = inline_parts.iter().map(|t| json_cost(t)).sum();
    assert_eq!(entry.inline_len(), expected);
    // Escapes count: the raw bytes are fewer than the escaped ones.
    let raw_bytes: usize = inline_parts.iter().map(|t| t.len()).sum();
    assert!(expected > raw_bytes, "{expected} vs {raw_bytes}");
    assert!(entry.inline_len() <= INLINE_LIMIT_BYTES);

    // Every Vec item counts: dropping the last item lowers the cost by exactly its share.
    let mut fewer = tool_call();
    fewer.content.pop();
    let fewer = ViewEntry::ToolCall(Box::new(fewer));
    assert_eq!(
        fewer.inline_len(),
        expected - json_cost(r#"{"type":"later"}"#)
    );

    // A blob counts 0, whatever its size; the inline form of the same field counts its text.
    let mut as_blob = tool_call();
    as_blob.raw_input = Some(TextOrBlob::Blob(blob(1, 1_000_000)));
    let as_blob = ViewEntry::ToolCall(Box::new(as_blob));
    assert_eq!(
        as_blob.inline_len(),
        expected - json_cost(r#"{"path":"draft.docx","note":"line1\nline2"}"#)
    );

    // A compaction marker counts its summary; other markers and a header count nothing.
    let marker = |kind| {
        ViewEntry::Marker(MarkerEntry {
            order: seq(9),
            event: event_id(9),
            kind,
            ts: Timestamp::ZERO,
        })
    };
    let compaction = |summary| MarkerKind::Compaction {
        status: CompactionStatus::Completed,
        summary,
    };
    let summary = Some(inline("short \"summary\""));
    assert_eq!(
        marker(compaction(summary)).inline_len(),
        json_cost("short \"summary\"")
    );
    assert_eq!(marker(compaction(None)).inline_len(), 0);
    let undone = MarkerKind::StepUndone {
        turn: turn_id(3),
        checkpoint: checkpoint_id(15),
        restored: vec![path("a.docx")],
        kept: vec![],
    };
    assert_eq!(marker(undone).inline_len(), 0);
    assert_eq!(header().inline_len(), 0);
}

fn header() -> ViewEntry {
    ViewEntry::Header(Box::new(header_entry()))
}

#[test]
fn a_resource_block_counts_both_of_its_inline_fields() {
    // Both fields inline: their costs add, neither subtracts nor multiplies.
    let mut tool = tool_call();
    tool.raw_input = None;
    tool.raw_output = None;
    tool.content = vec![ToolCallContent::Content {
        content: ContentBlock::Resource {
            uri: "file:///x".to_owned(),
            mime_type: None,
            text: Some(inline("abc")),
            blob: Some(inline("de")),
        },
    }];
    let entry = ViewEntry::ToolCall(Box::new(tool));
    assert_eq!(entry.inline_len(), json_cost("abc") + json_cost("de"));
    assert_eq!(json_cost("abc") + json_cost("de"), 5);
}

#[test]
fn entry_id_and_order_come_from_the_entry() {
    let entry = ViewEntry::ToolCall(Box::new(tool_call()));
    assert_eq!(entry.id(), EntryId::ToolCall(tool_call_id(3)));
    assert_eq!(entry.order(), seq(5));
    assert_eq!(header().id(), EntryId::Header(thread_id(2)));
    assert_eq!(header().id().to_string(), format!("header:{}", v7(2)));
    assert_ne!(header().id(), EntryId::Header(thread_id(1)));
}

#[test]
fn large_variants_are_boxed() {
    let sizes = [
        ("ViewEntry", size_of::<ViewEntry>()),
        ("ViewPatch", size_of::<ViewPatch>()),
        ("ViewPatchBody", size_of::<ViewPatchBody>()),
        ("HostEvent", size_of::<HostEvent>()),
        ("DomainEvent", size_of::<DomainEvent>()),
        ("ToolCallEntry", size_of::<ToolCallEntry>()),
        ("HeaderEntry", size_of::<HeaderEntry>()),
    ];
    println!("{sizes:?}");
    assert!(size_of::<ViewPatch>() <= 64, "{sizes:?}");
    // What the boxes save: either entry inline would put `ViewPatch` far over the bound.
    assert!(size_of::<ToolCallEntry>() > 64, "{sizes:?}");
    assert!(size_of::<ViewWindow>() > 64, "{sizes:?}");
}

#[test]
fn patches_round_trip() {
    let epoch = ViewEpoch::new(v7(30)).unwrap();
    let batch = PatchBatch {
        stream: StreamId::Thread(thread_id(1)),
        epoch,
        patches: vec![
            ViewPatch {
                cursor: Cursor::new(1).unwrap(),
                body: ViewPatchBody::Upsert {
                    entry: Box::new(header()),
                },
            },
            ViewPatch {
                cursor: Cursor::new(2).unwrap(),
                body: ViewPatchBody::Reset {
                    view_window: Box::new(ViewWindow {
                        header: Some(StreamHeader::Thread(Box::new(header_entry()))),
                        entries: vec![ViewEntry::ToolCall(Box::new(tool_call()))],
                        has_older: false,
                    }),
                },
            },
        ],
    };
    round_trips(&HostEvent::Patches(batch));
    let json = serde_json::to_string(&ViewPatch {
        cursor: Cursor::new(3).unwrap(),
        body: ViewPatchBody::Remove {
            entry_id: EntryId::Notice(event_id(5)),
        },
    })
    .unwrap();
    assert_eq!(
        json,
        format!(
            r#"{{"cursor":3,"body":{{"remove":{{"entry_id":"notice:{}"}}}}}}"#,
            v7(5)
        )
    );
}

/// An elicitation entry whose schema nests [`MAX_RAW_ENCODE_DEPTH`] lists round-trips; one level
/// more fails to encode in JSON and in `MessagePack`, so it never reaches a subscriber.
#[test]
fn elicitation_entry_with_a_too_deep_schema_fails_to_encode() {
    let entry = |depth: usize| {
        let schema = (0..depth).fold(RawPayload::Null, |inner, _| {
            RawPayload::Seq(vec![inner].into())
        });
        ViewEntry::Elicitation(ElicitationEntry {
            order: seq(7),
            elicitation: elicitation_id(12),
            request: ElicitationRequest::Form {
                message: "Which party?".to_owned(),
                schema,
            },
            state: ElicitationState::Expired {
                reason: ExpiryReason::TurnEnded,
            },
            answered_at: None,
        })
    };
    round_trips(&entry(MAX_RAW_ENCODE_DEPTH));
    let too_deep = entry(MAX_RAW_ENCODE_DEPTH + 1);
    let errors = [
        serde_json::to_string(&too_deep).unwrap_err().to_string(),
        rmp_serde::to_vec_named(&too_deep).unwrap_err().to_string(),
    ];
    for error in errors {
        assert!(error.contains("deeper than 92 levels"), "{error}");
    }
}

#[test]
fn agent_name_defaults_to_none() {
    // A stored header from before `agent_name` decodes with it `None`, and `None` is left out.
    let header = serde_json::to_value(header()).unwrap();
    assert!(header["header"].get("agent_name").is_none());
    let mut fields = header["header"].as_object().unwrap().clone();
    fields.insert("agent_name".to_owned(), "Gemini CLI".into());
    let named: HeaderEntry = serde_json::from_value(fields.clone().into()).unwrap();
    assert_eq!(named.agent_name.as_deref(), Some("Gemini CLI"));
    fields.remove("agent_name");
    let entry: HeaderEntry = serde_json::from_value(fields.into()).unwrap();
    assert_eq!(entry.agent_name, None);
}

#[test]
fn a_header_without_tools_decodes_with_none_and_writes_none() {
    let header = serde_json::to_value(header()).unwrap();
    let fields = header["header"].as_object().unwrap().clone();
    // Empty is left out, so a stored header from before `tools` writes back the same.
    assert!(!fields.contains_key("tools"));
    let entry: HeaderEntry = serde_json::from_value(fields.into()).unwrap();
    assert_eq!(entry.tools, []);
    assert_eq!(entry, header_entry());
}

#[test]
fn tool_states_round_trip_in_a_header_with_every_reason() {
    let mut entry = header_entry();
    entry.tools.push(ToolState {
        tool: ClauseoTool::WebSearch,
        availability: ToolAvailability::Available,
    });
    for reason in ToolsUnavailableReason::ALL
        .into_iter()
        .filter(|reason| *reason != ToolsUnavailableReason::Unknown)
    {
        entry.tools.push(ToolState {
            tool: ClauseoTool::Browser,
            availability: ToolAvailability::Unavailable { reason },
        });
    }
    round_trips(&entry);
    let json = serde_json::to_value(&entry).unwrap();
    assert_eq!(
        json["tools"][0],
        serde_json::json!({"tool": "web_search", "availability": "available"})
    );
    let reasons: Vec<&str> = json["tools"].as_array().unwrap()[1..]
        .iter()
        .map(|state| {
            assert_eq!(state["tool"], "browser");
            state["availability"]["unavailable"]["reason"]
                .as_str()
                .unwrap()
        })
        .collect();
    assert_eq!(
        reasons,
        [
            "off",
            "locked_off",
            "no_search_on_this_login",
            "managed_by_firm",
            "no_proxy",
            "browser_not_installed",
            "too_many_browsers",
        ]
    );
}

#[test]
fn an_unknown_tool_reason_or_availability_decodes() {
    let state: ToolState = serde_json::from_str(
        r#"{"tool": "scanner", "availability": {"unavailable": {"reason": "solar_flare"}}}"#,
    )
    .unwrap();
    assert_eq!(state.tool, ClauseoTool::Unknown);
    assert_eq!(
        state.availability,
        ToolAvailability::Unavailable {
            reason: ToolsUnavailableReason::Unknown
        }
    );
    let state: ToolState =
        serde_json::from_str(r#"{"tool": "browser", "availability": {"paused": {"until": 3}}}"#)
            .unwrap();
    assert_eq!(state.tool, ClauseoTool::Browser);
    assert!(matches!(
        state.availability,
        ToolAvailability::Unknown { .. }
    ));
}

#[test]
fn queue_held_defaults_to_false() {
    // A stored header or thread summary from before `queue_held` decodes with it false.
    let header = serde_json::to_value(header()).unwrap();
    let mut fields = header["header"].as_object().unwrap().clone();
    assert!(fields.remove("queue_held").is_some());
    let entry: HeaderEntry = serde_json::from_value(fields.into()).unwrap();
    assert!(!entry.queue_held);
    let summary = format!(
        r#"{{"thread":"{}","matter":"{}","agent":"claude-acp","status":"idle"}}"#,
        v7(1),
        v7(2)
    );
    let summary: ThreadSummary = serde_json::from_str(&summary).unwrap();
    assert!(!summary.queue_held);
    assert_eq!(summary.thread, thread_id(1));
}

#[test]
fn steer_joins_turn_and_background_default_when_missing() {
    // A stored header from before `steer_joins_turn` decodes with it false.
    let header = serde_json::to_value(header()).unwrap();
    let mut fields = header["header"].as_object().unwrap().clone();
    assert_eq!(fields.remove("steer_joins_turn"), Some(true.into()));
    let entry: HeaderEntry = serde_json::from_value(fields.into()).unwrap();
    assert!(!entry.steer_joins_turn);
    // A tool call without a background mark writes no field and reads back `None`.
    let tool = serde_json::to_value(tool_call()).unwrap();
    assert!(tool.get("background").is_none(), "{tool}");
    let read: ToolCallEntry = serde_json::from_value(tool).unwrap();
    assert_eq!(read.background, None);
}

#[test]
fn step_undo_possible_is_a_checkpoint_not_undone() {
    let step = |checkpoint_after, undone| StepEntry {
        order: seq(8),
        turn: turn_id(3),
        stop: StopReason::EndTurn,
        failure: None,
        files: vec![],
        skipped: vec![],
        checkpoint_after,
        undone,
        usage: None,
        ts: Timestamp::ZERO,
    };
    assert!(step(Some(checkpoint_id(14)), false).undo_possible());
    assert!(!step(Some(checkpoint_id(14)), true).undo_possible());
    assert!(!step(None, false).undo_possible());
    assert!(!step(None, true).undo_possible());
    // The UI derives the rule from the other fields; the wire form does not carry it.
    let json = serde_json::to_string(&step(Some(checkpoint_id(14)), false)).unwrap();
    assert!(!json.contains("undo_possible"), "{json}");
}

/// A snapshot's header is a thread's or a matter's header entry, in the entry's JSON form; any
/// other entry kind there is an error.
#[test]
fn a_snapshot_header_is_only_a_header_entry() {
    let snapshot =
        |header: &str| format!(r#"{{"header":{header},"entries":[],"has_older":false}}"#);
    let thread = serde_json::to_string(&header()).unwrap();
    let decoded: ViewWindow = serde_json::from_str(&snapshot(&thread)).unwrap();
    assert_eq!(
        decoded.header,
        Some(StreamHeader::Thread(Box::new(header_entry())))
    );
    let matter = MatterHeaderEntry {
        order: seq(1),
        matter: matter_id(2),
        name: "Smith v Jones".to_owned(),
        kind: MatterKind::Folder,
        import: None,
    };
    let json = serde_json::to_string(&ViewEntry::MatterHeader(matter.clone())).unwrap();
    let decoded: ViewWindow = serde_json::from_str(&snapshot(&json)).unwrap();
    assert_eq!(decoded.header, Some(StreamHeader::Matter(matter)));
    round_trips(&decoded);

    let notice = ViewEntry::Notice(NoticeEntry {
        order: seq(3),
        event: event_id(4),
        level: NoticeLevel::Info,
        text: "hello".to_owned(),
        ts: Timestamp::ZERO,
    });
    let json = serde_json::to_string(&notice).unwrap();
    let error = serde_json::from_str::<ViewWindow>(&snapshot(&json)).unwrap_err();
    assert!(
        error.to_string().contains("unknown variant `notice`"),
        "{error}"
    );
}

/// `AppendText` names the text it extends: the agent text, or one block of a user message. A
/// patch without a target, or with the earlier `block` field instead, is an error.
#[test]
fn append_text_names_its_target() {
    let patch = |target| ViewPatchBody::AppendText {
        message_id: message_id(9),
        target,
        from_len: 3,
        text: "abc".to_owned(),
    };
    let agent = serde_json::to_value(patch(AppendTarget::AgentText)).unwrap();
    assert_eq!(agent["append_text"]["target"], "agent_text");
    let user = serde_json::to_value(patch(AppendTarget::UserBlock(2))).unwrap();
    assert_eq!(
        user["append_text"]["target"],
        serde_json::json!({ "user_block": 2 })
    );
    round_trips(&patch(AppendTarget::UserBlock(2)));

    let id = v7(9);
    for fields in [
        format!(r#""message_id":"{id}","from_len":3,"text":"abc""#),
        format!(r#""message_id":"{id}","block":2,"from_len":3,"text":"abc""#),
    ] {
        let json = format!(r#"{{"append_text":{{{fields}}}}}"#);
        let error = serde_json::from_str::<ViewPatchBody>(&json).unwrap_err();
        assert!(
            error.to_string().contains("missing field `target`"),
            "{error}"
        );
    }
}

#[test]
fn a_limit_window_and_its_row_leave_out_model_when_none() {
    let mut window = LimitWindow {
        id: "model_scoped:0".to_owned(),
        kind: WindowKind::Model,
        label: "Fable weekly limit".to_owned(),
        used_percent: Finite::new(12.0).unwrap(),
        resets_at: None,
        duration_mins: None,
        warning: false,
        model: None,
    };
    assert!(
        serde_json::to_value(&window)
            .unwrap()
            .get("model")
            .is_none()
    );
    window.model = Some("Fable".to_owned());
    assert_eq!(serde_json::to_value(&window).unwrap()["model"], "Fable");
    round_trips(&window);

    let row = LimitWindowView {
        id: "model_scoped:0".to_owned(),
        kind: WindowKind::Model,
        label: "Fable weekly limit".to_owned(),
        model: Some("Fable".to_owned()),
        used_percent: Some(Finite::new(12.0).unwrap()),
        tone: MeterTone::Neutral,
        resets_at: None,
        freshness: WindowFreshness::Fresh,
    };
    round_trips(&row);
    // A window or row written before the field reads back with none.
    let mut old = serde_json::to_value(&window).unwrap();
    old.as_object_mut().unwrap().remove("model");
    assert_eq!(
        serde_json::from_value::<LimitWindow>(old).unwrap().model,
        None
    );
    let mut old = serde_json::to_value(&row).unwrap();
    old.as_object_mut().unwrap().remove("model");
    assert_eq!(
        serde_json::from_value::<LimitWindowView>(old)
            .unwrap()
            .model,
        None
    );
}

#[test]
fn folder_scanned_names_each_place_and_leaves_out_no_file() {
    let scanned = |place: FolderPlace| {
        HostEvent::FolderScanned(FolderScanned {
            folder: FolderRefId::new(v7(5)).unwrap(),
            files: 2,
            bytes: Count::new(10).unwrap(),
            now_reading: None,
            place,
            cloud_only: 0,
            done: true,
            cancelled: true,
        })
    };
    for (place, json) in [
        (FolderPlace::Local, serde_json::json!("local")),
        (FolderPlace::Network, serde_json::json!("network")),
        (
            FolderPlace::Synced {
                service: "iCloud Drive".to_owned(),
            },
            serde_json::json!({"synced": {"service": "iCloud Drive"}}),
        ),
    ] {
        let event = scanned(place);
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["folder_scanned"]["place"], json);
        // The last event names no file.
        assert!(
            value["folder_scanned"].get("now_reading").is_none(),
            "{value}"
        );
        round_trips(&event);
    }
}
