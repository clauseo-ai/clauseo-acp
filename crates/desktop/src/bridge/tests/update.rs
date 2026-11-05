//! The updater's view on the app feed (0037): `update_changed` reaches the settings window as it
//! reaches `main`, and a restart the bridge makes sends the current view after the settings.

use std::sync::Arc;

use clauseo_host::{InstallKind, Offer, ProgressSink, Updater, UpdaterError};
use clauseo_protocol::{UpdateState, UpdateView};
use futures::future::BoxFuture;

use super::*;
use crate::test_support::{Messages, collector};

/// Installed per user, and never finds a newer version.
struct NothingNewer;

impl Updater for NothingNewer {
    fn install_kind(&self) -> InstallKind {
        InstallKind::CurrentUser
    }

    fn check(&self) -> BoxFuture<'static, Result<Option<Offer>, UpdaterError>> {
        Box::pin(futures::future::ready(Ok(None)))
    }

    fn download(
        &self,
        _offer: Offer,
        _progress: ProgressSink,
    ) -> BoxFuture<'static, Result<clauseo_host::Downloaded, UpdaterError>> {
        Box::pin(futures::future::ready(Err(UpdaterError::Other(
            "nothing to download".to_owned(),
        ))))
    }
}

/// The view an `update_changed` message carries.
fn update(event: HostEvent) -> UpdateView {
    let HostEvent::UpdateChanged(view) = event else {
        panic!("not update_changed: {event:?}");
    };
    view
}

/// The next `update_changed` on `page`, past any other app event (`main`'s lists).
fn next_update(page: &Messages) -> UpdateView {
    loop {
        if let HostEvent::UpdateChanged(view) = page.next_event() {
            return view;
        }
    }
}

/// A check the settings window asks for reaches both windows' app feeds as `update_changed`:
/// `checking`, then `idle` with the check's time, in order.
#[test]
fn a_check_from_the_settings_window_reaches_both_windows_as_update_changed() {
    let scratch = Scratch::new("update-feed", &[]);
    let host = scratch.start_updating(Arc::new(NothingNewer));
    let bridge = host.bridge();
    let (main_channel, main) = collector();
    let (settings_channel, settings_page) = collector();
    block_on(bridge.open_stream(MAIN, main_channel)).unwrap();
    block_on(bridge.open_stream(SETTINGS, settings_channel)).unwrap();
    block_on(bridge.subscribe_app(MAIN)).unwrap();
    block_on(bridge.subscribe_app(SETTINGS)).unwrap();
    // The settings window's one first frame: the settings.
    assert!(matches!(
        settings_page.next_event(),
        HostEvent::SettingsChanged(_)
    ));

    let check = envelope(HostCommand::CheckForUpdate {});
    block_on(bridge.dispatch(SETTINGS, check)).unwrap();
    for (label, page) in [("main", &main), ("settings", &settings_page)] {
        assert_eq!(next_update(page).state, UpdateState::Checking, "{label}");
        let done = next_update(page);
        assert_eq!(done.state, UpdateState::Idle, "{label}");
        assert!(done.checked_at.is_some(), "{label}");
    }
    assert_eq!(settings_page.drain(), Vec::<String>::new());
}

/// The settings window's feed, restarted after a pause, reads the settings and the updater's
/// view again and sends both before the new subscription's first frame, so a check made while
/// it was paused shows.
#[test]
fn the_settings_window_s_restart_sends_the_settings_then_the_update() {
    let scratch = Scratch::new("update-restart", &[]);
    let host = scratch.start_updating(Arc::new(NothingNewer));
    let bridge = host.bridge();
    let (channel, page) = collector();
    block_on(bridge.open_stream(SETTINGS, channel)).unwrap();
    block_on(bridge.subscribe_app(SETTINGS)).unwrap();
    assert!(matches!(page.next_event(), HostEvent::SettingsChanged(_)));
    let window = bridge.window(SETTINGS).unwrap();

    let big = format!("{{\"update_changed\":\"{}\"}}", "x".repeat(300 * 1024));
    let window::Forward::Send {
        messages: sent,
        pause: true,
    } = pump::count_feed_event(&window, Feed::App, &big)
    else {
        panic!("not paused");
    };
    block_on(bridge.dispatch(SETTINGS, envelope(HostCommand::CheckForUpdate {}))).unwrap();
    // The host answers in order: the check's events are out before this read's answer.
    let _read = block_on(bridge.query(SETTINGS, HostQuery::GetUpdate)).unwrap();
    assert_eq!(page.drain(), Vec::<String>::new());

    let count = u32::try_from(sent.len()).unwrap() + page.limits_received();
    block_on(bridge.ack(SETTINGS, Vec::new(), count)).unwrap();
    assert!(matches!(page.next_event(), HostEvent::SettingsChanged(_)));
    // Checking, or done by now; not the idle view from before the check.
    let current = update(page.next_event());
    assert!(
        current.state == UpdateState::Checking || current.checked_at.is_some(),
        "{current:?}"
    );
    // Then the new subscription's first frame.
    assert!(matches!(page.next_event(), HostEvent::SettingsChanged(_)));
}
