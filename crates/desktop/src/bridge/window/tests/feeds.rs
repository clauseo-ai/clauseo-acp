//! The feeds of no stream (`feeds.rs`): they count with the streams', and a pause's end or a
//! watchdog fire restarts them.

use super::*;

/// The app-wide feed's events count with the streams' and pause the window; a pause drops them,
/// and its end restarts the feed once, under a new restart id, for the driver to take.
#[test]
fn the_app_feed_counts_pauses_and_restarts_with_the_streams() {
    let mut state = WindowState::new();
    let stream = thread();
    let r1 = state.follow(stream, None);
    assert_eq!(state.feeds_mut().take_restarts(), [], "not followed");
    let app = state.follow_feed(Feed::App);
    assert_eq!(
        state.feeds_mut().take_restarts(),
        [],
        "a follow is not a restart"
    );
    let Forward::Send { messages, pause } =
        state.forward_feed(Feed::App, app, &event(100), Duration::ZERO)
    else {
        panic!("not sent");
    };
    assert!(!pause);
    let mut sent = u32::try_from(messages.len()).unwrap();
    while !state.is_paused() {
        let (n, _) = send(&mut state, stream, r1, 8000);
        sent += n;
    }
    assert_eq!(
        state.forward_feed(Feed::App, app, &event(100), SECOND),
        Forward::Drop
    );

    let acked = state.ack(&[], sent, SECOND).unwrap();
    assert_eq!(acked.restarts.len(), 1);
    let [(Feed::App, restarted)] = state.feeds_mut().take_restarts()[..] else {
        panic!("the app feed did not restart alone");
    };
    assert_ne!(restarted, app);
    assert_eq!(state.feeds_mut().take_restarts(), [], "taken once");
    // The old subscription's events are dropped; the new one's are sent.
    assert_eq!(
        state.forward_feed(Feed::App, app, &event(100), SECOND),
        Forward::Drop
    );
    assert!(matches!(
        state.forward_feed(Feed::App, restarted, &event(100), SECOND),
        Forward::Send { pause: false, .. }
    ));
}

/// The watchdog restarts the app-wide feed with the streams; a failed restart unfollows it.
#[test]
fn the_watchdog_restarts_the_app_feed_and_a_failed_restart_unfollows_it() {
    let mut state = WindowState::new();
    let app = state.follow_feed(Feed::App);
    let Forward::Send { .. } = state.forward_feed(Feed::App, app, &event(100), Duration::ZERO)
    else {
        panic!("not sent");
    };
    assert_eq!(state.tick(WATCHDOG), []);
    let [(Feed::App, restarted)] = state.feeds_mut().take_restarts()[..] else {
        panic!("the app feed did not restart alone");
    };
    assert!(
        !state.feeds_mut().unfollow_restart(Feed::App, app),
        "an older restart unfollows nothing"
    );
    assert!(state.feeds_mut().unfollow_restart(Feed::App, restarted));
    assert_eq!(state.feeds().restart_id(Feed::App), None);
    assert_eq!(
        state.forward_feed(Feed::App, restarted, &event(100), SECOND),
        Forward::Drop
    );
}

/// The limits feed counts and restarts beside the app feed: one watchdog fire restarts both,
/// each under a new restart id, and each restart is taken once.
#[test]
fn the_limits_feed_counts_and_restarts_beside_the_app_feed() {
    let mut state = WindowState::new();
    let limits = state.follow_feed(Feed::Limits);
    let app = state.follow_feed(Feed::App);
    let Forward::Send { messages, .. } =
        state.forward_feed(Feed::Limits, limits, &event(100), Duration::ZERO)
    else {
        panic!("not sent");
    };
    let total: usize = messages.iter().map(Frame::len).sum();
    assert_eq!(state.unacked_bytes(), total as u64);
    assert_eq!(
        state.forward_feed(Feed::Limits, app, &event(100), Duration::ZERO),
        Forward::Drop,
        "another feed's restart id"
    );

    assert_eq!(state.tick(WATCHDOG), []);
    let restarts = state.feeds_mut().take_restarts();
    let [(Feed::App, new_app), (Feed::Limits, new_limits)] = restarts[..] else {
        panic!("not both feeds: {restarts:?}");
    };
    assert!(new_app != app && new_limits != limits && new_app != new_limits);
    assert_eq!(state.feeds_mut().take_restarts(), [], "taken once");
    assert_eq!(
        state.forward_feed(Feed::Limits, limits, &event(100), SECOND),
        Forward::Drop
    );
    assert!(matches!(
        state.forward_feed(Feed::Limits, new_limits, &event(100), SECOND),
        Forward::Send { pause: false, .. }
    ));
}
