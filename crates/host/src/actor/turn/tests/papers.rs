//! A turn's end and the matter's papers (decision record 0055): with no folder watch (Windows,
//! `papers_live` off), every end, cancelled or failed alike, sends `PapersChanged` for the turn's
//! matter, so the app lists its papers again; with the watch (macOS, Linux) an end sends none,
//! since the watch tells of each change itself. A listing follows its matter (the one the app
//! reopens at launch is never opened), and forgetting the matter ends the watch.

use clauseo_protocol::{
    ErrorCode, ExpiryReason, HostEvent, HostQuery, MatterId, Principal, StopReason,
};
use futures::channel::oneshot;
use futures::{FutureExt as _, StreamExt as _};

use super::failures::until_ended;
use super::rig::Rig;
use crate::AppSubscription;
use crate::actor::input::Request;
use crate::actor::turn::{End, end};

/// An app subscription on the rig's actor.
fn subscribe(rig: &mut Rig) -> AppSubscription {
    let (reply, mut answer) = oneshot::channel();
    let principal = Principal::LocalDesktop;
    rig.actor
        .on_request(Request::SubscribeApp { principal, reply });
    let mut got = None;
    rig.run_until("the app subscription", |_| {
        if let Ok(Some(result)) = answer.try_recv() {
            got = Some(result);
        }
        got.is_some()
    });
    got.unwrap().unwrap()
}

/// The matters of the `PapersChanged` events `app` holds now.
fn papers_changed(app: &mut AppSubscription) -> Vec<MatterId> {
    let mut matters = Vec::new();
    while let Some(Some(item)) = app.next().now_or_never() {
        if let HostEvent::PapersChanged { matter } = item.decode().unwrap() {
            matters.push(matter);
        }
    }
    matters
}

/// Ends a running turn with `end`, the watch `live` or not, and the matter's watch started
/// first when `watched`: the `PapersChanged` the end sent, and the turn's matter.
fn papers_changed_by(test: &str, live: bool, watched: bool, end: End) -> (Vec<MatterId>, MatterId) {
    let mut rig = Rig::start(test, "");
    rig.actor.papers_live = live;
    if watched {
        list_papers(&mut rig);
        until_live(&mut rig);
    }
    let thread = rig.thread();
    rig.running_turn(thread);
    let mut app = subscribe(&mut rig);
    assert_eq!(papers_changed(&mut app), [], "none before the end");
    end::end_turn(&mut rig.actor, thread, end);
    until_ended(&mut rig, thread);
    let told = papers_changed(&mut app);
    let matter = rig.matter();
    rig.finish();
    (told, matter)
}

#[test]
fn with_no_watch_each_turn_end_lists_the_matters_papers_again() {
    let (told, matter) =
        papers_changed_by("papers-end-cancelled", false, false, End::cancelled(None));
    assert_eq!(told, [matter]);
    let failed = End::failed(ErrorCode::AgentProtocolError, None, None);
    let (told, matter) = papers_changed_by("papers-end-failed", false, false, failed);
    assert_eq!(told, [matter]);
    let done = End {
        stop: StopReason::EndTurn,
        failure: None,
        expiry: ExpiryReason::TurnEnded,
        detach: None,
    };
    let (told, matter) = papers_changed_by("papers-end-done", false, false, done);
    assert_eq!(told, [matter]);
}

#[cfg(not(windows))]
#[test]
fn with_the_matters_watch_live_a_turn_end_sends_no_papers_changed() {
    let (told, _) = papers_changed_by("papers-end-watched", true, true, End::cancelled(None));
    assert_eq!(told, []);
}

/// A matter whose watch is not live (not followed, as in a second window; or a start that
/// failed) is listed again at each turn's end, as on Windows.
#[test]
fn with_the_watch_on_but_not_live_for_the_matter_a_turn_end_lists_again() {
    let (told, matter) =
        papers_changed_by("papers-end-unfollowed", true, false, End::cancelled(None));
    assert_eq!(told, [matter]);
}

/// No watch goes live on Windows.
#[cfg(windows)]
fn until_live(_rig: &mut Rig) {
    panic!("no folder watch on Windows");
}

/// Waits until the followed matter's watch is live.
#[cfg(not(windows))]
fn until_live(rig: &mut Rig) {
    rig.run_until("the watch started", |actor| {
        actor
            .papers_follow
            .as_ref()
            .is_some_and(|follow| follow.is_live())
    });
}

/// A quick A, B, A: A's second start finds its first still running. It reads gone, so the next
/// listing that finds the folder starts it again, rather than never.
#[cfg(not(windows))]
#[test]
fn a_start_behind_one_still_running_is_tried_again_by_the_next_listing() {
    use crate::actor::papers::{WatchStarted, follow, on_started, watch::Started};
    let mut rig = Rig::start("papers-busy-start", "");
    rig.actor.papers_live = true;
    let matter = rig.matter();
    let folder = std::path::PathBuf::from(&rig.actor.matters[&matter].row.folder_path);
    follow::opened(&mut rig.actor, matter);
    let generation = rig.actor.papers_follow.as_ref().unwrap().generation();
    let busy = WatchStarted {
        generation,
        started: Started::Busy,
    };
    on_started(&mut rig.actor, busy);
    assert!(rig.actor.papers_follow.as_ref().unwrap().is_gone());
    rig.run_until("the first start's thread ended", |_| {
        !crate::actor::papers::watch::is_starting(&folder)
    });
    follow::listed(&mut rig.actor, matter, &folder, true);
    until_live(&mut rig);
    rig.finish();
}

/// A `ListPapers` of the rig's matter, answered.
fn list_papers(rig: &mut Rig) {
    let (reply, mut answer) = oneshot::channel();
    let query = HostQuery::ListPapers {
        matter: rig.matter(),
    };
    let principal = Principal::LocalDesktop;
    rig.actor.on_request(Request::Query {
        principal,
        query,
        reply,
    });
    let mut got = false;
    rig.run_until("the listing", |_| {
        got = got || matches!(answer.try_recv(), Ok(Some(_)));
        got
    });
}

#[cfg(not(windows))]
#[test]
fn a_listing_with_no_open_starts_the_watch_and_forgetting_the_matter_ends_it() {
    let mut rig = Rig::start("papers-list-follows", "");
    rig.actor.papers_live = true;
    assert!(rig.actor.papers_follow.is_none());
    list_papers(&mut rig);
    until_live(&mut rig);
    let matter = rig.matter();
    crate::actor::papers::follow::forgotten(&mut rig.actor, matter);
    assert!(rig.actor.papers_follow.is_none());
    rig.finish();
}

#[test]
fn with_no_watch_a_listing_follows_nothing() {
    let mut rig = Rig::start("papers-list-unwatched", "");
    rig.actor.papers_live = false;
    list_papers(&mut rig);
    assert!(rig.actor.papers_follow.is_none());
    rig.finish();
}

#[test]
fn the_watch_is_off_on_windows_only() {
    assert_eq!(crate::actor::papers::follow::live_watch(), !cfg!(windows));
}
