//! Updates that set fields of an existing entry: each field a tool call update, a usage update
//! or a late `FilesChanged` carries is shown with one `Upsert` when it changes the entry, and a
//! repeat of it changes nothing.

use clauseo_protocol::{
    Cost, Count, FilesChanged, Finite, SkippedFile, SkippedFileReason, TextOrBlob, ToolCallPatch,
    ToolCallUpdated, ToolKind, ToolLocation, UsageUpdated, ViewEntry,
};
use clauseo_reducer::{StreamState, ViewChange, apply};

use crate::fixtures::{
    Log, doc_path, file_changes, files_changed, short, start_turn, thread_created, tool_started,
    turn_ended, usage,
};
use crate::helpers::{first_upsert, fold};

/// The only change among `changes`, which must be one `Upsert`.
#[cfg(test)]
fn only_upsert(changes: &[ViewChange]) -> ViewEntry {
    assert_eq!(changes.len(), 1, "{changes:?}");
    first_upsert(changes)
}

#[test]
fn each_tool_call_field_an_update_changes_is_shown() {
    let patches = [
        ToolCallPatch {
            title: Some(short("Edit clause 9")),
            ..ToolCallPatch::default()
        },
        ToolCallPatch {
            kind: Some(ToolKind::Read),
            ..ToolCallPatch::default()
        },
        ToolCallPatch {
            tool_name: Some(short("Write")),
            ..ToolCallPatch::default()
        },
        ToolCallPatch {
            locations: Some(vec![ToolLocation::Inside {
                path: doc_path(2).unwrap(),
                line: None,
            }]),
            ..ToolCallPatch::default()
        },
        ToolCallPatch {
            raw_input: Some(TextOrBlob::Inline("{\"path\":\"docs/1.md\"}".to_owned())),
            ..ToolCallPatch::default()
        },
        ToolCallPatch {
            raw_output: Some(TextOrBlob::Inline("Edited.".to_owned())),
            ..ToolCallPatch::default()
        },
    ];
    for patch in patches {
        let mut log = Log::thread(22).unwrap();
        log.push(thread_created().unwrap()).unwrap();
        let turn = start_turn(&mut log, "Edit clause 1.").unwrap();
        let tool_call = log.fresh().unwrap();
        log.push(tool_started(turn, tool_call, 1).unwrap()).unwrap();
        let mut state = fold(log.events());
        let before = state.entries().clone();
        let update = Box::new(ToolCallUpdated { tool_call, patch });

        let out = apply(&mut state, &log.push(update.clone()).unwrap()).unwrap();
        let shown = only_upsert(&out.changes);
        assert!(matches!(shown, ViewEntry::ToolCall(_)), "{shown:?}");
        assert_eq!(state.entries().get(&shown.order()), Some(&shown));
        assert_ne!(
            before.get(&shown.order()),
            Some(&shown),
            "{:?}",
            update.patch
        );

        let again = apply(&mut state, &log.push(update).unwrap()).unwrap();
        assert!(again.changes.is_empty(), "{:?}", again.changes);
    }
}

/// A `UsageUpdated` carrying only the fields `set` fills.
#[cfg(test)]
fn usage_with(set: impl FnOnce(&mut UsageUpdated)) -> UsageUpdated {
    let mut event = UsageUpdated {
        context_used: None,
        context_size: None,
        cost: None,
    };
    set(&mut event);
    event
}

#[test]
fn each_usage_field_an_update_carries_replaces_the_shown_one() {
    let cost = Cost {
        amount: Finite::new(0.42).unwrap(),
        currency: "USD".to_owned(),
    };
    let updates = [
        usage_with(|u| u.context_used = Some(Count::new(5000).unwrap())),
        usage_with(|u| u.context_size = Some(Count::new(1_000_000).unwrap())),
        usage_with(|u| u.cost = Some(cost)),
    ];
    for update in updates {
        let mut log = Log::thread(22).unwrap();
        log.push(thread_created().unwrap()).unwrap();
        log.push(usage(1000).unwrap()).unwrap();
        let mut state = fold(log.events());
        let before = header_usage(&state);

        let out = apply(&mut state, &log.push(update.clone()).unwrap()).unwrap();
        let ViewEntry::Header(header) = only_upsert(&out.changes) else {
            panic!("{:?}", out.changes);
        };
        let shown = header.usage.unwrap();
        assert_eq!(Some(&shown), header_usage(&state).as_ref());
        // The fields the update carries replace the shown ones; the others stay.
        let before = before.unwrap();
        let used = update.context_used.or(before.context_used);
        let size = update.context_size.or(before.context_size);
        assert_eq!(shown.context_used, used, "{update:?}");
        assert_eq!(shown.context_size, size, "{update:?}");
        assert_eq!(
            shown.cost,
            update.cost.clone().or(before.cost),
            "{update:?}"
        );

        let again = apply(&mut state, &log.push(update).unwrap()).unwrap();
        assert!(again.changes.is_empty(), "{:?}", again.changes);
    }
}

/// The usage the thread's header shows.
#[cfg(test)]
fn header_usage(state: &StreamState) -> Option<clauseo_protocol::ContextUsage> {
    state.as_thread().unwrap().header().unwrap().usage.clone()
}

#[test]
fn files_changed_after_the_turn_ended_updates_its_step() {
    let mut log = Log::thread(22).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Tidy the lease.").unwrap();
    log.push(files_changed(turn).unwrap()).unwrap();
    let checkpoint = log.fresh().unwrap();
    log.push(turn_ended(turn, checkpoint).unwrap()).unwrap();
    let mut state = fold(log.events());
    let skipped = vec![SkippedFile {
        path: doc_path(3).unwrap(),
        reason: SkippedFileReason::OverSizeCap,
    }];
    // Both lists change, then only the files.
    let updates = [
        FilesChanged {
            turn,
            files: vec![],
            skipped: skipped.clone(),
        },
        FilesChanged {
            turn,
            files: file_changes().unwrap(),
            skipped,
        },
    ];
    for update in updates {
        let out = apply(&mut state, &log.push(update.clone()).unwrap()).unwrap();
        let ViewEntry::Step(step) = only_upsert(&out.changes) else {
            panic!("{:?}", out.changes);
        };
        assert_eq!(step.files, update.files);
        assert_eq!(step.skipped, update.skipped);
        let kept = state.entries().get(&step.order);
        assert_eq!(kept, Some(&ViewEntry::Step(step)));
    }
}
