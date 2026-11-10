//! A turn's end and the matter's papers (decision record 0055): with no folder watch (Windows,
//! `papers_live` off), every end, cancelled or failed alike, sends `PapersChanged` for the turn's
//! matter, so the app lists its papers again; with the watch (macOS, Linux) an end sends none,
//! since the watch tells of each change itself.

use clauseo_protocol::{ErrorCode, HostEvent, MatterId, Principal};
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

/// Ends a running turn with `end`, the watch `live` or not: the `PapersChanged` the end sent,
/// and the turn's matter.
fn papers_changed_by(test: &str, live: bool, end: End) -> (Vec<MatterId>, MatterId) {
    let mut rig = Rig::start(test, "");
    rig.actor.papers_live = live;
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
    let (told, matter) = papers_changed_by("papers-end-cancelled", false, End::cancelled(None));
    assert_eq!(told, [matter]);
    let failed = End::failed(ErrorCode::AgentProtocolError, None, None);
    let (told, matter) = papers_changed_by("papers-end-failed", false, failed);
    assert_eq!(told, [matter]);
}

#[test]
fn with_the_watch_a_turn_end_sends_no_papers_changed() {
    let (told, _) = papers_changed_by("papers-end-watched", true, End::cancelled(None));
    assert_eq!(told, []);
}

#[test]
fn the_watch_is_off_on_windows_only() {
    assert_eq!(crate::actor::papers::follow::live_watch(), !cfg!(windows));
}
