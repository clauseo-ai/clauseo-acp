//! R24's case-law hint through the host, against S28's fake connector (Q8: once per matter,
//! dismissible). A turn whose web search asks "Supreme Court judgment section 138 NI Act"
//! publishes one `CaseLawHint { matter, thread, turn }`; a second such turn in the same matter
//! publishes none; a new matter publishes one; `DismissCaseLawHint` then a turn publishes none.
//! With the connector added, the switch off, or the connector not live, none.
//!
//! Each "none" is checked against a later hint the same host does publish (in a fresh matter):
//! the app stream is in order, and the refusals happen on the actor at the turn's end, before
//! anything later, so a hint the test does not expect would arrive first and fail it.

use clauseo_connectors::fake::Fixture;
use clauseo_protocol::{
    ConnectorCommand, HostCommand, HostEvent, MatterId, StopReason, StreamId, ThreadId, ToolsPatch,
    TurnId,
};
use futures::StreamExt as _;

use crate::connector_rig::{Rig, Start};
use crate::settings::decode;
use crate::support::{Dir, Scenario, matter_with_thread, wait};

const CLAUDE: &str = "claude-acp";

/// One hint, as published.
type Hint = (MatterId, ThreadId, TurnId);

fn dir(test: &str) -> Dir {
    Dir::new(test, &[(CLAUDE, Scenario::Local("case-law-hint"))])
}

/// The next `CaseLawHint` on the app stream; the other events are kept for the secrets check.
fn next_hint(rig: &mut Rig, what: &str) -> Hint {
    loop {
        let item = wait(what, rig.sub.next()).expect("the app subscription ended");
        rig.seen.push(item.json().to_owned());
        if let HostEvent::CaseLawHint {
            matter,
            thread,
            turn,
        } = decode(&item)
        {
            return (matter, thread, turn);
        }
    }
}

/// A new matter and thread named `name`.
fn matter(dir: &Dir, rig: &Rig, name: &str) -> (MatterId, ThreadId) {
    let (matter, thread, _) = matter_with_thread(dir, &rig.host, name, &[], CLAUDE);
    (matter, thread)
}

/// One prompt of `thread` to its end; its turn.
fn turn(rig: &Rig, thread: ThreadId) -> TurnId {
    let mut events = rig.host.events(StreamId::Thread(thread));
    let (turn, _) = rig
        .host
        .prompt(thread, "Find Supreme Court judgments on section 138.");
    let ended = events.until_turn_ended(turn);
    assert_eq!(
        ended.stop,
        StopReason::EndTurn,
        "{ended:?}\n{}",
        events.dump()
    );
    turn
}

/// The host has read the credential (and liveness): `Check` answers once both are applied.
fn checked(rig: &mut Rig) {
    rig.ok(ConnectorCommand::Check {});
}

/// The hint file's text.
fn hints_json(dir: &Dir) -> serde_json::Value {
    let path = dir.data().join("connectors").join("hints.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn the_hint_shows_once_per_matter_and_never_after_a_dismiss() {
    let dir = dir("case_law_hint_once");
    let mut rig = Rig::start(&dir, Start::default());
    checked(&mut rig);
    let (a, a_thread) = matter(&dir, &rig, "a");
    let first = turn(&rig, a_thread);
    assert_eq!(next_hint(&mut rig, "the first hint"), (a, a_thread, first));
    // The same matter again: no hint.
    turn(&rig, a_thread);
    let (b, b_thread) = matter(&dir, &rig, "b");
    let in_b = turn(&rig, b_thread);
    assert_eq!(next_hint(&mut rig, "matter b's hint"), (b, b_thread, in_b));
    // Dismissed before its first turn: no hint.
    let (c, c_thread) = matter(&dir, &rig, "c");
    rig.ok(ConnectorCommand::DismissCaseLawHint { matter: c });
    turn(&rig, c_thread);
    let (d, d_thread) = matter(&dir, &rig, "d");
    let in_d = turn(&rig, d_thread);
    assert_eq!(next_hint(&mut rig, "matter d's hint"), (d, d_thread, in_d));
    let file = hints_json(&dir);
    assert_eq!(file["v"], 1);
    assert_eq!(file["matters"][a.to_string()], "shown");
    assert_eq!(file["matters"][b.to_string()], "shown");
    assert_eq!(file["matters"][c.to_string()], "dismissed");
    assert_eq!(file["matters"][d.to_string()], "shown");
    // Dismissing a matter that was shown keeps it away too.
    rig.ok(ConnectorCommand::DismissCaseLawHint { matter: a });
    assert_eq!(hints_json(&dir)["matters"][a.to_string()], "dismissed");
    // Never signed in: no secret was issued to look for.
    rig.host.shutdown();
}

/// With the connector added, no hint; removed, the next matter's turn gets one.
#[test]
fn no_hint_while_the_connector_is_added() {
    let dir = dir("case_law_hint_added");
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let (_, added) = matter(&dir, &rig, "added");
    turn(&rig, added);
    rig.ok(ConnectorCommand::Remove {
        id: "case-law".to_owned(),
    });
    let (after, after_thread) = matter(&dir, &rig, "after");
    let in_after = turn(&rig, after_thread);
    assert_eq!(
        next_hint(&mut rig, "the hint once removed"),
        (after, after_thread, in_after)
    );
    rig.finish();
}

/// With the switch off (the lawyer's own), no hint; on again, the next matter's turn gets one.
#[test]
fn no_hint_while_the_switch_is_off() {
    let dir = dir("case_law_hint_off");
    let mut rig = Rig::start(&dir, Start::default());
    checked(&mut rig);
    let set = |connectors| HostCommand::SetToolSettings {
        patch: ToolsPatch {
            connectors: Some(connectors),
            ..ToolsPatch::default()
        },
    };
    rig.host.ok(set(false));
    let (_, off) = matter(&dir, &rig, "off");
    turn(&rig, off);
    rig.host.ok(set(true));
    let (on, on_thread) = matter(&dir, &rig, "on");
    let in_on = turn(&rig, on_thread);
    assert_eq!(
        next_hint(&mut rig, "the hint once on"),
        (on, on_thread, in_on)
    );
    // Never signed in: no secret was issued to look for.
    rig.host.shutdown();
}

/// With the price list in preview (not live), the turn's check asks liveness and shows
/// nothing. `Check`'s answer settles every matter waiting on liveness (with the same cached
/// "not live"), so once it is in, the price list turns live (a day later, past the cache) and the
/// next matter's turn shows the one hint.
#[test]
fn no_hint_while_the_connector_is_not_live() {
    let dir = dir("case_law_hint_not_live");
    let mut rig = Rig::start(&dir, Start::default());
    rig.fake.set_pricing(Fixture::File("pricing-preview.json"));
    checked(&mut rig);
    let (_, quiet) = matter(&dir, &rig, "quiet");
    turn(&rig, quiet);
    checked(&mut rig);
    rig.fake.set_pricing(Fixture::File("pricing-live.json"));
    rig.host.clock.advance(std::time::Duration::from_hours(25));
    let (live, live_thread) = matter(&dir, &rig, "live");
    let in_live = turn(&rig, live_thread);
    assert_eq!(
        next_hint(&mut rig, "the hint once live"),
        (live, live_thread, in_live)
    );
    // Never signed in: no secret was issued to look for.
    rig.host.shutdown();
}
