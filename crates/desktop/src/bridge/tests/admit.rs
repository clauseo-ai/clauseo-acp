//! The allow-lists of `dispatch` and `query` (`Bridge::admit`): every call the R12 window
//! (`ui/app/src/internal/session.ts`) and the room make today passes, and every command the
//! bridge refuses is refused before any window has opened its stream.

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
    ];
    for body in commands {
        let admitted = bridge.admit(&command(body.clone()));
        assert!(admitted.is_ok(), "{body}: {admitted:?}");
    }
    // Every read the windows make answers through the bridge, with no window open.
    let reads = [
        json!("list_matters"),
        json!({"list_threads": {"matter": host.scratch()}}),
        json!("get_settings"),
        // The wall's file list (H8).
        json!({"list_papers": {"matter": host.scratch()}}),
    ];
    for body in reads {
        let answer = block_on(bridge.query(read(body.clone())));
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
        command(json!({"answer_permission": {"thread": thread,
            "request": Uuid::now_v7(), "option_id": "allow"}})),
        command(json!({"set_mode": {"thread": thread, "mode_id": "bypassPermissions"}})),
        command(
            json!({"set_config_option": {"thread": thread, "option_id": "mode",
            "value": {"select": "bypassPermissions"}}}),
        ),
    ];
    for refused in refused {
        let refusal = block_on(bridge.dispatch(envelope(refused.clone()))).unwrap_err();
        assert_eq!(
            refusal.code(),
            ErrorCode::CommandInvalid,
            "{refused:?}: {refusal:?}"
        );
    }
    assert!(bridge.window(MAIN).is_err(), "no window opened its stream");
    let matters = block_on(bridge.query(HostQuery::ListMatters)).unwrap();
    let QueryResult::Matters(matters) = matters else {
        panic!("{matters:?}");
    };
    assert_eq!(matters.len(), 1, "only the Scratch matter: {matters:?}");
}
