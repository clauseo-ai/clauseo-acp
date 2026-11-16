//! A matter on the list inside the conversation's own (`agent::matter_fence::held`), through the
//! host's API with Claude's and Codex's launches: stage C's F4 case (matter O holds registered
//! `Sub2` at `O/W/Sub2`) launches no conversation in O, so no command (an `rg NOTE .`) ever runs
//! there; a live O conversation is refused at its next prompt once `Sub2` appears inside O; and
//! `Sub2`'s own conversation runs. The layout comes as it does outside Clauseo: `Sub2` is made
//! elsewhere, reached through a link, then moved into O and the link pointed after it. Links
//! need no privilege on macOS and Linux only; the unit rig (`actor/turn/tests/nested_refuse.rs`)
//! covers the rows on every OS.
#![cfg(unix)]

use std::path::PathBuf;

use clauseo_protocol::{DomainEvent, ErrorCode, NoticeKind, StopReason, StreamId, ThreadId};

use crate::codex::{CODEX, run_turn, turn_failed};
use crate::observe::Events;
use crate::support::{Dir, Running, Scenario};
use crate::write;

const CLAUDE: &str = "claude-acp";

fn dir(test: &str) -> Dir {
    Dir::new(
        test,
        &[
            (CLAUDE, Scenario::Bundled("claude-prompt-stream")),
            (CODEX, Scenario::Bundled("codex-handshake")),
        ],
    )
}

/// Matters O and `Sub2`, `Sub2` registered through the link `L` that leads elsewhere.
struct Layout {
    o: clauseo_protocol::MatterId,
    sub2: clauseo_protocol::MatterId,
    o_folder: PathBuf,
    elsewhere: PathBuf,
    link: PathBuf,
}

fn layout(dir: &Dir, host: &Running) -> Layout {
    let o_folder = dir.folder("O", &[("n-note.txt", "NOTE of O\n")]);
    let elsewhere = dir.folder("E", &[]);
    let _sub2 = dir.folder("E/Sub2", &[("sub-note.txt", "NOTE of Sub2\n")]);
    let link = o_folder.parent().unwrap().join("L");
    write::symlink(&elsewhere, &link);
    let sub2 = host.create_matter("Sub2", &link.join("Sub2"));
    let o = host.create_matter("O", &o_folder);
    Layout {
        o,
        sub2,
        o_folder,
        elsewhere,
        link,
    }
}

/// `Sub2`'s folder moved by hand into `O/W`, the link pointed after it: its folder as stored now
/// lies inside O's.
fn nest(layout: &Layout) {
    let w = layout.o_folder.join("W");
    write::create_dir_all(&w);
    write::rename(&layout.elsewhere.join("Sub2"), &w.join("Sub2"));
    write::remove_file(&layout.link);
    write::symlink(&w, &layout.link);
}

/// The `MatterInside` notices' matters and names, in order.
fn inside(events: &Events) -> Vec<(clauseo_protocol::MatterId, String)> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) => match &notice.kind {
                Some(NoticeKind::MatterInside { matter, name }) => Some((*matter, name.clone())),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// `thread`'s next turn, asserted refused before any prompt, the notice naming `Sub2`.
fn refused(host: &Running, events: &mut Events, thread: ThreadId, layout: &Layout) {
    let turn = run_turn(host, events, thread);
    let failed = turn_failed(events, turn).unwrap_or_else(|| panic!("{}", events.dump()));
    assert_eq!(
        failed.code,
        ErrorCode::MatterHoldsOtherMatter,
        "{}",
        events.dump()
    );
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::Failed);
    assert_eq!(events.turn_text(turn), "", "nothing ran in O");
    assert_eq!(
        inside(events).last(),
        Some(&(layout.sub2, "Sub2".to_owned()))
    );
}

#[test]
fn no_conversation_launches_in_a_matter_holding_a_registered_matter_for_either_agent() {
    let dir = dir("nested_refuse_launch");
    let host = dir.start(0);
    let layout = layout(&dir, &host);
    nest(&layout);
    for agent in [CLAUDE, CODEX] {
        let thread = host.create_thread(layout.o, agent);
        let mut events = host.events(StreamId::Thread(thread));
        refused(&host, &mut events, thread, &layout);
    }
    // The inner matter's conversation runs, with either agent.
    for agent in [CLAUDE, CODEX] {
        let thread = host.create_thread(layout.sub2, agent);
        let mut events = host.events(StreamId::Thread(thread));
        let turn = run_turn(&host, &mut events, thread);
        assert_eq!(
            turn_failed(&events, turn),
            None,
            "{agent}: {}",
            events.dump()
        );
        assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    }
    host.shutdown();
}

#[test]
fn a_live_conversation_is_refused_at_its_next_prompt_once_a_matter_appears_inside() {
    let dir = dir("nested_refuse_next");
    let host = dir.start(0);
    let layout = layout(&dir, &host);
    let mut live = Vec::new();
    for agent in [CLAUDE, CODEX] {
        let thread = host.create_thread(layout.o, agent);
        let mut events = host.events(StreamId::Thread(thread));
        let turn = run_turn(&host, &mut events, thread);
        assert_eq!(
            turn_failed(&events, turn),
            None,
            "{agent}: {}",
            events.dump()
        );
        live.push((thread, events));
    }
    nest(&layout);
    for (thread, mut events) in live {
        refused(&host, &mut events, thread, &layout);
    }
    host.shutdown();
}
