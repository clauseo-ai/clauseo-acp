//! The allow-lists of `dispatch` and `query` (`Bridge::admit`): every call the R12 window
//! (`ui/app/src/internal/session.ts`) and the room make today passes, and every command the
//! bridge refuses is refused before any window has opened its stream. The settings window
//! sends `SetUiSettings`, `SetToolSettings` and `RefreshLimits` (Plan and usage, U12) and reads
//! `GetSettings`, nothing else; another label nothing (0028).

use clauseo_protocol::{MatterId, ThreadId};
use serde_json::json;

use super::*;

/// A command as the page writes it.
fn command(body: serde_json::Value) -> HostCommand {
    let text = body.to_string();
    serde_json::from_value(body).unwrap_or_else(|err| panic!("{text}: {err}"))
}

/// A query as the page writes it.
fn read(body: serde_json::Value) -> HostQuery {
    let text = body.to_string();
    serde_json::from_value(body).unwrap_or_else(|err| panic!("{text}: {err}"))
}

/// The commands the R12 window sends through `dispatch` today, as `session.ts` writes them, and
/// the room's (`matters.ts`, `display-store.ts`). Its other calls are commands of their own
/// (`answer_permission`, `pick_folder`, the stream's). A window that loses one fails here.
#[test]
fn every_command_and_read_the_windows_send_today_is_admitted() {
    let scratch = Scratch::new("admit-today", &["work"]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let home = scratch.path();
    let work = scratch.path().join("work");
    let picked = block_on(bridge.pick_folder(&Picks(Some(work)), Some(home))).unwrap();
    let folder = picked.unwrap().folder;
    let matter = MatterId::new(Uuid::now_v7()).unwrap();
    let thread = ThreadId::new(Uuid::now_v7()).unwrap();
    let turn = Uuid::now_v7();
    let commands = [
        json!({"create_matter": {"name": "Smith v Jones",
            "source": {"existing_folder": {"folder": folder}}}}),
        json!({"create_thread": {"matter": matter, "agent": "fake"}}),
        json!({"send_prompt": {"thread": thread, "content": [{"text": {"text": "Summarise"}}]}}),
        json!({"cancel": {"thread": thread}}),
        json!({"undo_step": {"thread": thread, "turn": turn,
            "files": [{"path": "brief.docx", "choice": "undo"}]}}),
        json!({"set_ui_settings": {"patch": {"theme": "dark"}}}),
        // The room's "Locate folder" (H3), with a folder this run's picker issued.
        json!({"relocate_matter": {"matter": matter, "folder": folder}}),
        // The limits block's "Try again" (R16).
        json!({"refresh_limits": {"agent": null, "why": "try_again"}}),
        // The scan before a matter (R20a): a cancel passes with any ref.
        json!({"cancel_scan": {"folder": folder}}),
        // The rail's triage (H-triage, 0045): snooze, Done and the morning sweep.
        json!({"snooze_thread": {"thread": thread, "until": 1_790_000_000_000_u64}}),
        json!({"snooze_thread": {"thread": thread}}),
        json!({"mark_thread_done": {"thread": thread, "done": true}}),
        json!({"sweep_done": {"day_start": 1_790_000_000_000_u64}}),
    ];
    for body in commands {
        let admitted = bridge.admit(MAIN, &command(body.clone()));
        assert!(admitted.is_ok(), "{body}: {admitted:?}");
    }
    // Every read the windows make answers through the bridge, with no window open.
    let reads = [
        json!("list_matters"),
        json!({"list_threads": {"matter": host.scratch()}}),
        json!("get_settings"),
        // The wall's file list (H8).
        json!({"list_papers": {"matter": host.scratch()}}),
        // A new conversation's plan choice (R15's `ListAgents`, host-wire.ts's agents part).
        json!("list_agents"),
    ];
    for body in reads {
        let answer = block_on(bridge.query(MAIN, read(body.clone())));
        assert!(answer.is_ok(), "{body}: {answer:?}");
    }
}

/// Every command `admit` refuses, refused with `command.invalid` before the window has opened
/// its stream, and nothing reaches the host: no matter is made.
#[test]
fn every_refused_command_is_refused_before_open_stream() {
    let scratch = Scratch::new("admit-refused", &["work"]);
    let host = scratch.start(Vec::new());
    let thread = host.thread();
    let bridge = host.bridge();
    let not_picked = block_on(host.host.register_folder(scratch.path().join("work"))).unwrap();
    let refused = [
        command(json!({"create_matter": {"name": "Smith v Jones",
            "source": {"existing_folder": {"folder": not_picked}}}})),
        command(json!({"create_matter": {"name": "Smith v Jones",
            "source": {"new_folder": {"folder": not_picked}}}})),
        command(json!({"relocate_matter": {"matter": host.scratch(), "folder": not_picked}})),
        command(json!({"scan_folder": {"folder": not_picked}})),
        command(json!({"answer_permission": {"thread": thread,
            "request": Uuid::now_v7(), "option_id": "allow"}})),
        command(json!({"set_mode": {"thread": thread, "mode_id": "bypassPermissions"}})),
        command(
            json!({"set_config_option": {"thread": thread, "option_id": "mode",
            "value": {"select": "bypassPermissions"}}}),
        ),
    ];
    for refused in refused {
        let refusal = block_on(bridge.dispatch(MAIN, envelope(refused.clone()))).unwrap_err();
        assert_eq!(
            refusal.code(),
            ErrorCode::CommandInvalid,
            "{refused:?}: {refusal:?}"
        );
    }
    assert!(bridge.window(MAIN).is_err(), "no window opened its stream");
    let matters = block_on(bridge.query(MAIN, HostQuery::ListMatters)).unwrap();
    let QueryResult::Matters(matters) = matters else {
        panic!("{matters:?}");
    };
    assert_eq!(matters.len(), 1, "only the Scratch matter: {matters:?}");
}

/// The settings window may send `SetUiSettings`, `SetToolSettings` and, for Plan and usage
/// (U12), `RefreshLimits`, and read `GetSettings`; it is refused every other command and read with
/// `command.invalid`, before the host sees it. A window of any other label is refused everything.
#[test]
fn the_settings_window_sets_and_reads_the_settings_and_refreshes_the_limits_only() {
    let scratch = Scratch::new("admit-settings", &[]);
    let host = scratch.start(Vec::new());
    let thread = host.thread();
    let bridge = host.bridge();
    let set = command(json!({"set_ui_settings": {"patch": {"theme": "dark"}}}));
    assert!(bridge.admit(SETTINGS, &set).is_ok());
    // The tool switches: the settings window sends them, and a window of another label may not.
    let tools = command(json!({"set_tool_settings": {"patch": {"browser": false}}}));
    assert!(bridge.admit(SETTINGS, &tools).is_ok());
    assert_eq!(
        bridge.admit("other", &tools).unwrap_err().code(),
        ErrorCode::CommandInvalid
    );
    // The main window admits `SetUiSettings` (the room changes theme and text size itself) but
    // not `SetToolSettings`: the switches live in the settings window only.
    assert!(bridge.admit(MAIN, &set).is_ok());
    assert_eq!(
        bridge.admit(MAIN, &tools).unwrap_err().code(),
        ErrorCode::CommandInvalid
    );
    let refused_from_main = command(json!({"set_mode": {"thread": thread, "mode_id": "x"}}));
    assert_eq!(
        bridge.admit(MAIN, &refused_from_main).unwrap_err().code(),
        ErrorCode::CommandInvalid
    );
    // Plan and usage: `shown` as it opens, `try_again` from its button, for one agent or all.
    let refreshes = [
        command(json!({"refresh_limits": {"agent": null, "why": "shown"}})),
        command(json!({"refresh_limits": {"agent": "claude", "why": "try_again"}})),
    ];
    for refresh in &refreshes {
        assert!(bridge.admit(SETTINGS, refresh).is_ok(), "{refresh:?}");
        let other = bridge.admit("other", refresh).unwrap_err();
        assert_eq!(other.code(), ErrorCode::CommandInvalid, "{refresh:?}");
    }
    let cancel = command(json!({"cancel": {"thread": thread}}));
    let create = command(json!({"create_thread": {"matter": host.scratch(), "agent": "fake"}}));
    // The room's seen mark (H6) is main's alone: nothing in settings marks a thread seen.
    let seen = command(json!({"mark_seen": {"thread": thread}}));
    assert!(bridge.admit(MAIN, &seen).is_ok());
    // The rail's triage (0045) is main's alone too.
    let snooze =
        command(json!({"snooze_thread": {"thread": thread, "until": 1_790_000_000_000_u64}}));
    let done = command(json!({"mark_thread_done": {"thread": thread, "done": true}}));
    let sweep = command(json!({"sweep_done": {"day_start": 1_790_000_000_000_u64}}));
    for triage in [&snooze, &done, &sweep] {
        assert!(bridge.admit(MAIN, triage).is_ok(), "{triage:?}");
    }
    for refused in [cancel, create, seen, snooze, done, sweep] {
        let refusal = block_on(bridge.dispatch(SETTINGS, envelope(refused.clone()))).unwrap_err();
        assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{refused:?}");
    }
    let outcome = block_on(bridge.dispatch(SETTINGS, envelope(set.clone())));
    assert!(outcome.is_ok(), "{outcome:?}");
    let outcome = block_on(bridge.dispatch(SETTINGS, envelope(tools)));
    assert!(outcome.is_ok(), "{outcome:?}");
    let outcome = block_on(bridge.dispatch(SETTINGS, envelope(refreshes[0].clone())));
    assert!(outcome.is_ok(), "{outcome:?}");
    let read_settings = block_on(bridge.query(SETTINGS, HostQuery::GetSettings));
    assert!(
        matches!(read_settings, Ok(QueryResult::Settings(_))),
        "{read_settings:?}"
    );
    for body in [
        json!("list_matters"),
        json!({"list_threads": {"matter": host.scratch()}}),
    ] {
        let refusal = block_on(bridge.query(SETTINGS, read(body.clone()))).unwrap_err();
        assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{body}");
    }
    let other = bridge.admit("other", &set).unwrap_err();
    assert_eq!(other.code(), ErrorCode::CommandInvalid);
    let other = block_on(bridge.query("other", HostQuery::GetSettings)).unwrap_err();
    assert_eq!(other.code(), ErrorCode::CommandInvalid);
}

/// `ScanFolder` passes `admit` only with a folder this run's picker issued; `CancelScan`
/// passes with any ref, even one never issued, and starts nothing: no scan event comes and the
/// host answers `Done`.
#[test]
fn scan_folder_needs_a_picked_folder_and_cancel_scan_starts_nothing() {
    let scratch = Scratch::new("admit-scan", &["work"]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let work = scratch.path().join("work");
    let picked = block_on(bridge.pick_folder(&Picks(Some(work)), Some(scratch.path()))).unwrap();
    let folder = picked.unwrap().folder;
    let scan = command(json!({"scan_folder": {"folder": folder}}));
    assert!(bridge.admit(MAIN, &scan).is_ok());

    let mut app = block_on(
        host.host
            .subscribe_app(clauseo_protocol::Principal::LocalDesktop),
    )
    .unwrap();
    let never_issued = Uuid::now_v7();
    let cancel = command(json!({"cancel_scan": {"folder": never_issued}}));
    let answer = block_on(bridge.dispatch(MAIN, envelope(cancel))).unwrap();
    assert_eq!(answer, CommandOutcome::Done);
    // A query after it: the host handled the cancel before answering it.
    block_on(bridge.query(MAIN, HostQuery::ListMatters)).unwrap();
    while let Some(Some(item)) =
        futures::FutureExt::now_or_never(futures::StreamExt::next(&mut app))
    {
        let event: HostEvent = item.decode().unwrap();
        assert!(
            !matches!(event, HostEvent::FolderScanned(_)),
            "a cancel started a scan: {event:?}"
        );
    }
}

/// The privacy switches and the first act (decision record 0036) are refused from every window
/// until S21's next pull request rules on them, so no window can start the usage counts yet.
#[test]
fn no_window_sends_the_privacy_switches_or_the_first_act_yet() {
    let scratch = Scratch::new("admit-privacy", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    for body in [
        json!({"set_privacy_settings": {"patch": {"counts": true}}}),
        json!({"first_act": {"counts": true}}),
    ] {
        let refused = command(body);
        for window in [MAIN, SETTINGS, "other"] {
            assert_eq!(
                bridge.admit(window, &refused).unwrap_err().code(),
                ErrorCode::CommandInvalid,
                "{window}: {refused:?}"
            );
        }
    }
}

/// Search (0043) is main's: the settings window and any other window are refused it before the
/// host sees it, and main gets a page.
#[test]
fn search_is_main_windows_alone() {
    let scratch = Scratch::new("admit-search", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let search = read(json!({"search": {
        "query": "lease",
        "scope": "all",
        "include_archived": false,
        "generation": 1
    }}));
    for label in [SETTINGS, "other"] {
        let refusal = block_on(bridge.query(label, search.clone())).unwrap_err();
        assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{label}");
    }
    let answer = block_on(bridge.query(MAIN, search)).unwrap();
    assert!(
        matches!(
            answer,
            QueryResult::Search(clauseo_protocol::SearchAnswer::Page(_))
        ),
        "{answer:?}"
    );
}

/// REVIEW REPRO: a reload (a second `open_stream`) starts the page's generation count over;
/// the reloaded page's searches must not be answered `Cancelled` for the old page's count.
#[test]
fn review_a_reloaded_main_window_can_search_again() {
    let scratch = Scratch::new("admit-search-reload", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let search = |generation: u64| {
        read(json!({"search": {
            "query": "Scratch",
            "scope": "all",
            "include_archived": false,
            "generation": generation
        }}))
    };
    let _old = open(&bridge, MAIN);
    let before = block_on(bridge.query(MAIN, search(5))).unwrap();
    assert!(matches!(
        before,
        QueryResult::Search(clauseo_protocol::SearchAnswer::Page(_))
    ));
    let _new = open(&bridge, MAIN);
    let after = block_on(bridge.query(MAIN, search(1))).unwrap();
    assert!(
        matches!(
            after,
            QueryResult::Search(clauseo_protocol::SearchAnswer::Page(_))
        ),
        "the reloaded page's first search: {after:?}"
    );
}
