//! The allow-lists of `dispatch` and `query` (`Bridge::admit`): every call the R12 window
//! (`ui/app/src/internal/session.ts`) and the room make today passes, and every command the
//! bridge refuses is refused before any window has opened its stream. The settings window
//! sends `SetUiSettings`, `SetToolSettings`, `SetPrivacySettings` and `RefreshLimits` (Plan and
//! usage, U12) and reads `GetSettings`, nothing else; `FirstAct` comes from `main` only; another
//! label sends nothing (0028, 0036).
//! The Chrome for Testing download's two commands are main's alone (0029). The updater's
//! commands and `GetUpdate` go by the table in `the_update_commands_go_by_window` (0037).

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

/// `PreviewUndo` (Undo's ask, #360) reaches the host from the main window, which answers for
/// a turn with no step with its own refusal, and the settings window is refused it by the
/// bridge.
#[test]
fn preview_undo_is_read_from_main_and_refused_from_settings() {
    let scratch = Scratch::new("admit-preview-undo", &[]);
    let host = scratch.start(Vec::new());
    let thread = host.thread();
    let bridge = host.bridge();
    let body = json!({"preview_undo": {"thread": thread, "turn": Uuid::now_v7()}});
    let refusal = block_on(bridge.query(MAIN, read(body.clone()))).unwrap_err();
    assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{refusal:?}");
    assert!(
        refusal.to_string().contains("has no step to undo"),
        "the host's answer, not the bridge's: {refusal}"
    );
    let refusal = block_on(bridge.query(SETTINGS, read(body))).unwrap_err();
    assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{refusal:?}");
    assert!(
        refusal.to_string().contains("reads only the settings"),
        "{refusal}"
    );
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

/// `privacy.counts` as the settings window reads it.
/// The New matter dialog's folder warning takes only a ref this run's picker issued, as
/// `CreateMatter` does, for both sources that name one; the default location names none.
#[test]
fn folder_support_needs_a_picked_folder() {
    let scratch = Scratch::new("admit-folder-support", &["work", "other"]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let work = scratch.path().join("work");
    let picked = block_on(bridge.pick_folder(&Picks(Some(work)), Some(scratch.path()))).unwrap();
    let folder = picked.unwrap().folder;
    let not_picked = block_on(host.host.register_folder(scratch.path().join("other"))).unwrap();
    for kind in ["existing_folder", "new_folder"] {
        let ask = |folder| read(json!({"folder_support": {"source": {kind: {"folder": folder}}}}));
        let answer = block_on(bridge.query(MAIN, ask(folder)));
        // Admitted: it reaches the host, which answers or refuses the folder as create would
        // (the scratch's folders sit in its data folder, so an import's is
        // `matter.folder_in_matters_root`); never the bridge's `command.invalid`.
        assert!(
            matches!(answer, Ok(QueryResult::FolderSupport(_)))
                || answer
                    .as_ref()
                    .is_err_and(|error| error.code() == ErrorCode::MatterFolderInMattersRoot),
            "{kind}: {answer:?}"
        );
        let refusal = block_on(bridge.query(MAIN, ask(not_picked))).unwrap_err();
        assert_eq!(
            refusal.code(),
            ErrorCode::CommandInvalid,
            "{kind}: {refusal:?}"
        );
    }
    let default = read(json!({"folder_support": {"source": "default_location"}}));
    let answer = block_on(bridge.query(MAIN, default));
    assert!(
        matches!(answer, Ok(QueryResult::FolderSupport(_))),
        "{answer:?}"
    );
}

fn counts_switch(bridge: &Bridge) -> bool {
    match block_on(bridge.query(SETTINGS, HostQuery::GetSettings)) {
        Ok(QueryResult::Settings(view)) => view.privacy.counts,
        other => panic!("{other:?}"),
    }
}

/// The welcome's `FirstAct` (0036) comes from `main` only: the settings window, and a window of
/// any other label, is refused it before the host sees it.
#[test]
fn the_first_act_comes_from_the_main_window_only() {
    let scratch = Scratch::new("admit-first-act", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let first_act = command(json!({"first_act": {"counts": false}}));
    assert!(bridge.admit(MAIN, &first_act).is_ok());
    for window in [SETTINGS, "other"] {
        let refusal = block_on(bridge.dispatch(window, envelope(first_act.clone()))).unwrap_err();
        assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{window}");
    }
    let outcome = block_on(bridge.dispatch(MAIN, envelope(first_act)));
    assert!(outcome.is_ok(), "{outcome:?}");
}

/// The privacy switches (0036) are set from the settings window only, as the tool switches are:
/// `main`, and a window of any other label, is refused them before the host sees them, and the
/// switch stays as it was.
#[test]
fn the_privacy_switches_come_from_the_settings_window_only() {
    let scratch = Scratch::new("admit-privacy", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let off = command(json!({"set_privacy_settings": {"patch": {"counts": false}}}));
    assert!(bridge.admit(SETTINGS, &off).is_ok());
    assert!(counts_switch(&bridge));
    for window in [MAIN, "other"] {
        let refusal = block_on(bridge.dispatch(window, envelope(off.clone()))).unwrap_err();
        assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{window}");
    }
    assert!(counts_switch(&bridge), "a refused patch changed nothing");
    let outcome = block_on(bridge.dispatch(SETTINGS, envelope(off)));
    assert!(outcome.is_ok(), "{outcome:?}");
    assert!(!counts_switch(&bridge));
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

/// The Chrome for Testing download (0029) is the main window's: its consent dialog sends
/// `install_browser` and its Cancel `cancel_browser_install`. The settings window and any other
/// label are refused both with `command.invalid`, before the host sees them.
#[test]
fn only_the_main_window_starts_or_cancels_the_browser_download() {
    let scratch = Scratch::new("admit-browser", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let install = command(json!({"install_browser": {}}));
    let cancel = command(json!({"cancel_browser_install": {}}));
    for browser in [&install, &cancel] {
        assert!(bridge.admit(MAIN, browser).is_ok(), "{browser:?}");
        for label in [SETTINGS, "other"] {
            let refusal = block_on(bridge.dispatch(label, envelope(browser.clone()))).unwrap_err();
            assert_eq!(
                refusal.code(),
                ErrorCode::CommandInvalid,
                "{label} {browser:?}"
            );
        }
    }
    // With no download running, main's cancel reaches the host and does nothing.
    let outcome = block_on(bridge.dispatch(MAIN, envelope(cancel)));
    assert!(matches!(outcome, Ok(CommandOutcome::Done)), "{outcome:?}");
}

/// The updater (0037), one row per window and command: main installs, cancels a scheduled
/// install and checks; the settings window sets the mode and checks; neither sends the other's,
/// and a window of any other label sends none. `true` is admitted, `false` refused with
/// `command.invalid`. Both windows read `GetUpdate`; another label may not.
#[test]
fn the_update_commands_go_by_window() {
    let scratch = Scratch::new("admit-update", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let install_now = json!({"install_update": {"when": "now"}});
    let install_after = json!({"install_update": {"when": "after_turn"}});
    let cancel = json!({"cancel_scheduled_update": {}});
    let check = json!({"check_for_update": {}});
    let auto = json!({"set_update_mode": {"mode": "auto"}});
    let notify = json!({"set_update_mode": {"mode": "notify"}});
    let rows = [
        (MAIN, &install_now, true),
        (MAIN, &install_after, true),
        (MAIN, &cancel, true),
        (MAIN, &check, true),
        (MAIN, &auto, false),
        (MAIN, &notify, false),
        (SETTINGS, &install_now, false),
        (SETTINGS, &install_after, false),
        (SETTINGS, &cancel, false),
        (SETTINGS, &check, true),
        (SETTINGS, &auto, true),
        (SETTINGS, &notify, true),
        ("other", &install_now, false),
        ("other", &install_after, false),
        ("other", &cancel, false),
        ("other", &check, false),
        ("other", &auto, false),
        ("other", &notify, false),
    ];
    for (window, body, allowed) in rows {
        let admitted = bridge.admit(window, &command(body.clone()));
        match admitted {
            Ok(()) => assert!(allowed, "{window} {body} was admitted"),
            Err(refusal) => {
                assert!(!allowed, "{window} {body}: {refusal:?}");
                assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{window} {body}");
            }
        }
    }
    for window in [MAIN, SETTINGS] {
        let answer = block_on(bridge.query(window, HostQuery::GetUpdate));
        assert!(
            matches!(answer, Ok(QueryResult::Update(_))),
            "{window}: {answer:?}"
        );
    }
    let other = block_on(bridge.query("other", HostQuery::GetUpdate)).unwrap_err();
    assert_eq!(other.code(), ErrorCode::CommandInvalid);
}
