//! The app-wide feed on the window's channel (`subscribe_app`): what reaches it, a restart the
//! bridge makes, a reload, and two windows (`main` and `settings`) opening and closing.

use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use clauseo_protocol::{SettingsView, Theme, UiPatch};

use super::*;
use crate::test_support::{Messages, collector};

/// A `SetUiSettings` of `theme`.
fn set_theme(theme: Theme) -> CommandEnvelope {
    envelope(HostCommand::SetUiSettings {
        patch: UiPatch {
            theme: Some(theme),
            ..UiPatch::default()
        },
    })
}

/// The host's first frames of an app subscription (0023): the settings, the whole matter list,
/// and the whole thread list of its one matter (Scratch). Returns the settings.
fn first_frames(messages: &Messages) -> SettingsView {
    let current = settings(messages.next_event());
    let event = messages.next_event();
    assert!(
        matches!(&event, HostEvent::MattersChanged(changed) if changed.all),
        "{event:?}"
    );
    let event = messages.next_event();
    assert!(
        matches!(&event, HostEvent::ThreadsChanged(changed) if changed.all),
        "{event:?}"
    );
    current
}

/// The settings a `settings_changed` message carries.
fn settings(event: HostEvent) -> SettingsView {
    let HostEvent::SettingsChanged(settings) = event else {
        panic!("not settings_changed: {event:?}");
    };
    settings
}

#[test]
fn subscribe_app_needs_open_stream_then_carries_each_settings_change() {
    let scratch = Scratch::new("app-feed", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let refusal = block_on(bridge.subscribe_app(MAIN)).unwrap_err();
    assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{refusal:?}");

    let messages = open(&bridge, MAIN);
    block_on(bridge.subscribe_app(MAIN)).unwrap();
    // It starts with the host's first frames: the settings as they are, then the lists.
    assert_eq!(first_frames(&messages).ui.theme, Theme::System);
    block_on(bridge.dispatch(MAIN, set_theme(Theme::Dark))).unwrap();
    let text = messages.next_text();
    assert!(text.starts_with(r#"{"settings_changed":"#), "{text}");
    assert_eq!(settings(messages_event(&text)).ui.theme, Theme::Dark);
    // The message counts like a stream's: an ack of it is not more than was sent.
    block_on(bridge.ack(MAIN, Vec::new(), 1)).unwrap();
}

/// `text` decoded as an event.
fn messages_event(text: &str) -> HostEvent {
    serde_json::from_str(text).unwrap_or_else(|err| panic!("not an event ({err}): {text}"))
}

/// A pause drops the app feed's host subscription with the streams'; the ack that ends it
/// subscribes again, reads the settings and sends one `settings_changed` with them, so a change
/// made while paused shows. Later changes follow on the new subscription.
#[test]
fn a_pause_s_end_resubscribes_and_sends_the_settings_then() {
    let scratch = Scratch::new("app-restart", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let messages = open(&bridge, MAIN);
    block_on(bridge.subscribe_app(MAIN)).unwrap();
    first_frames(&messages);
    let window = bridge.window(MAIN).unwrap();

    // Past the cap without the host: a large app event, counted as sent, pauses the window and
    // drops every host subscription.
    let big = format!("{{\"settings_changed\":\"{}\"}}", "x".repeat(300 * 1024));
    let paused = pump::count_feed_event(&window, Feed::App, &big);
    let window::Forward::Send {
        messages: sent,
        pause: true,
    } = paused
    else {
        panic!("not paused: {paused:?}");
    };
    // While paused, a change reaches no message.
    block_on(bridge.dispatch(MAIN, set_theme(Theme::Light))).unwrap();
    assert_eq!(messages.drain(), Vec::<String>::new());

    let count = u32::try_from(sent.len()).unwrap() + messages.limits_received();
    block_on(bridge.ack(MAIN, Vec::new(), count)).unwrap();
    let current = settings(messages.next_event());
    assert_eq!(current.ui.theme, Theme::Light);
    // Then the new subscription's first frames.
    assert_eq!(first_frames(&messages).ui.theme, Theme::Light);
    block_on(bridge.dispatch(MAIN, set_theme(Theme::Dark))).unwrap();
    assert_eq!(settings(messages.next_event()).ui.theme, Theme::Dark);
}

/// A reload (a second `open_stream`) drops the app feed: the old window's pump is aborted, so
/// its host subscription goes at once, and a change reaches neither channel until the new page
/// subscribes.
#[test]
fn a_reload_drops_the_app_feed() {
    let scratch = Scratch::new("app-reload", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let old = open(&bridge, MAIN);
    block_on(bridge.subscribe_app(MAIN)).unwrap();
    first_frames(&old);
    let dropped = pump::feed_dropped(&bridge.window(MAIN).unwrap(), Feed::App).unwrap();
    let new = open(&bridge, MAIN);
    // The pump never sees another event, so only the abort can end it.
    assert_eq!(
        dropped.recv_timeout(Duration::from_secs(20)),
        Err(RecvTimeoutError::Disconnected),
        "the old window's app pump outlived the reload"
    );
    assert_eq!(
        lock(&bridge.window(MAIN).unwrap())
            .state()
            .feeds()
            .restart_id(Feed::App),
        None
    );
    block_on(bridge.dispatch(MAIN, set_theme(Theme::Dark))).unwrap();
    // The host answers in order: a read after the write comes after any event of it.
    let _read = block_on(bridge.query(MAIN, HostQuery::GetSettings)).unwrap();
    assert_eq!(old.drain(), Vec::<String>::new());
    assert_eq!(new.drain(), Vec::<String>::new());

    block_on(bridge.subscribe_app(MAIN)).unwrap();
    assert_eq!(first_frames(&new).ui.theme, Theme::Dark);
    block_on(bridge.dispatch(MAIN, set_theme(Theme::Light))).unwrap();
    assert_eq!(settings(new.next_event()).ui.theme, Theme::Light);
    assert_eq!(old.drain(), Vec::<String>::new());
}

/// The watchdog, with bytes unacknowledged for 5 s, restarts the app feed through the driver's
/// tick: it subscribes again and sends the settings, then follows changes on the new
/// subscription.
#[test]
fn a_watchdog_fire_resubscribes_and_sends_the_settings_then() {
    let scratch = Scratch::new("app-watchdog", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let messages = open(&bridge, MAIN);
    block_on(bridge.subscribe_app(MAIN)).unwrap();
    first_frames(&messages);
    let window = bridge.window(MAIN).unwrap();
    // One message the page never acks.
    block_on(bridge.dispatch(MAIN, set_theme(Theme::Dark))).unwrap();
    assert_eq!(settings(messages.next_event()).ui.theme, Theme::Dark);
    let before = lock(&window).state().feeds().restart_id(Feed::App);

    pump::tick_at(&window, window::WATCHDOG * 2);
    let resent = settings(messages.next_event());
    assert_eq!(resent.ui.theme, Theme::Dark);
    assert_eq!(first_frames(&messages).ui.theme, Theme::Dark);
    assert_ne!(lock(&window).state().feeds().restart_id(Feed::App), before);
    block_on(bridge.dispatch(MAIN, set_theme(Theme::Light))).unwrap();
    assert_eq!(settings(messages.next_event()).ui.theme, Theme::Light);
}

/// A reload (a second `open_stream`) drops the old window's limits pump, so its host
/// subscription goes at once; the new window follows the limits from its own first view.
#[test]
fn a_reload_drops_the_limits_feed() {
    let scratch = Scratch::new("limits-reload", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let old = open(&bridge, MAIN);
    let dropped = pump::feed_dropped(&bridge.window(MAIN).unwrap(), Feed::Limits).unwrap();
    // `open` checks the new window's first message is the view.
    let _new = open(&bridge, MAIN);
    // The pump never sees another event, so only the abort can end it.
    assert_eq!(
        dropped.recv_timeout(Duration::from_secs(20)),
        Err(RecvTimeoutError::Disconnected),
        "the old window's limits pump outlived the reload"
    );
    assert_eq!(old.limits_received(), 1);
    assert_eq!(old.drain(), Vec::<String>::new());
}

/// `main` and `settings` each follow the app feed on their own channel: a change the settings
/// window makes reaches both.
#[test]
fn both_windows_get_the_settings_change() {
    let scratch = Scratch::new("app-two-windows", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let (main_channel, main) = collector();
    let (settings_channel, settings_page) = collector();
    block_on(bridge.open_stream(MAIN, main_channel)).unwrap();
    block_on(bridge.open_stream(SETTINGS, settings_channel)).unwrap();
    block_on(bridge.subscribe_app(MAIN)).unwrap();
    block_on(bridge.subscribe_app(SETTINGS)).unwrap();
    block_on(bridge.dispatch(SETTINGS, set_theme(Theme::Dark))).unwrap();
    assert_eq!(settings(main.next_event()).ui.theme, Theme::Dark);
    assert_eq!(settings(settings_page.next_event()).ui.theme, Theme::Dark);
    block_on(bridge.ack(SETTINGS, Vec::new(), 1)).unwrap();
}

/// A settings page reopened (a second `open_stream` from `settings`) replaces only its own
/// channel: `main`'s stays, and keeps its app feed.
#[test]
fn reopening_settings_leaves_mains_channel() {
    let scratch = Scratch::new("app-reopen-settings", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let (main_channel, main) = collector();
    block_on(bridge.open_stream(MAIN, main_channel)).unwrap();
    block_on(bridge.subscribe_app(MAIN)).unwrap();
    let main_window = bridge.window(MAIN).unwrap();
    let (first, old) = collector();
    block_on(bridge.open_stream(SETTINGS, first)).unwrap();
    let (second, new) = collector();
    block_on(bridge.open_stream(SETTINGS, second)).unwrap();
    assert!(Arc::ptr_eq(&main_window, &bridge.window(MAIN).unwrap()));
    block_on(bridge.dispatch(MAIN, set_theme(Theme::Light))).unwrap();
    assert_eq!(settings(main.next_event()).ui.theme, Theme::Light);
    let _read = block_on(bridge.query(MAIN, HostQuery::GetSettings)).unwrap();
    assert_eq!(old.drain(), Vec::<String>::new());
    assert_eq!(new.drain(), Vec::<String>::new());
}

/// Closing a window drops its entry whole: the channel (nothing more reaches it, and its
/// commands are refused), the app pump (its host subscription), and the window itself, whose
/// drop aborts the watchdog. `main` carries on.
#[test]
fn closing_a_window_drops_its_channel_app_pump_and_watchdog() {
    let scratch = Scratch::new("app-close", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let (main_channel, main) = collector();
    block_on(bridge.open_stream(MAIN, main_channel)).unwrap();
    block_on(bridge.subscribe_app(MAIN)).unwrap();
    let built = bridge.window_built(SETTINGS);
    let (channel, page) = collector();
    block_on(bridge.open_stream(SETTINGS, channel)).unwrap();
    block_on(bridge.subscribe_app(SETTINGS)).unwrap();
    let window = bridge.window(SETTINGS).unwrap();
    let dropped = pump::feed_dropped(&window, Feed::App).unwrap();
    let gone = Arc::downgrade(&window);
    drop(window);

    bridge.close_window(SETTINGS, built);
    assert_eq!(
        dropped.recv_timeout(Duration::from_secs(20)),
        Err(RecvTimeoutError::Disconnected),
        "the closed window's app pump outlived it"
    );
    // No task holds the window strongly: once the bridge lets go, it drops, and with it the
    // watchdog (`Window`'s `Drop`). A watchdog tick in flight holds it for that tick only.
    let (_keep, idle) = std::sync::mpsc::channel::<()>();
    for _ in 0..2000 {
        if gone.strong_count() == 0 {
            break;
        }
        let _tick = idle.recv_timeout(Duration::from_millis(10));
    }
    assert_eq!(gone.strong_count(), 0, "the closed window is still alive");
    let refusal = block_on(bridge.subscribe_app(SETTINGS)).unwrap_err();
    assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{refusal:?}");
    let refusal = block_on(bridge.ack(SETTINGS, Vec::new(), 0)).unwrap_err();
    assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{refusal:?}");

    block_on(bridge.dispatch(MAIN, set_theme(Theme::Dark))).unwrap();
    assert_eq!(settings(main.next_event()).ui.theme, Theme::Dark);
    assert_eq!(page.drain(), Vec::<String>::new());
}

/// Closing `main` drops its limits pump with the window, so its host subscription goes.
#[test]
fn closing_main_drops_its_limits_feed() {
    let scratch = Scratch::new("limits-close", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let built = bridge.window_built(MAIN);
    let page = open(&bridge, MAIN);
    let dropped = pump::feed_dropped(&bridge.window(MAIN).unwrap(), Feed::Limits).unwrap();
    bridge.close_window(MAIN, built);
    // The pump never sees another event, so only the abort can end it.
    assert_eq!(
        dropped.recv_timeout(Duration::from_secs(20)),
        Err(RecvTimeoutError::Disconnected),
        "the closed window's limits pump outlived it"
    );
    assert!(
        bridge.window(MAIN).is_err(),
        "the closed window kept its stream"
    );
    assert_eq!(page.drain(), Vec::<String>::new());
}

/// A close that arrives after the window was built again removes only a stream opened before
/// that build: never the newer page's. The newer window's own close removes it.
#[test]
fn a_late_close_of_an_old_window_leaves_the_newer_one() {
    let scratch = Scratch::new("app-late-close", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let first_build = bridge.window_built(SETTINGS);
    let (first, _old) = collector();
    block_on(bridge.open_stream(SETTINGS, first)).unwrap();
    // Rebuilt before the old window's close lands: the old stream is stale and goes.
    let second_build = bridge.window_built(SETTINGS);
    bridge.close_window(SETTINGS, first_build);
    assert!(
        bridge.window(SETTINGS).is_err(),
        "the old window's stream survived"
    );

    let (second, new) = collector();
    block_on(bridge.open_stream(SETTINGS, second)).unwrap();
    // The old window's close again, later still: the newer page keeps its stream.
    bridge.close_window(SETTINGS, first_build);
    block_on(bridge.subscribe_app(SETTINGS)).unwrap();
    block_on(bridge.dispatch(SETTINGS, set_theme(Theme::Dark))).unwrap();
    assert_eq!(settings(new.next_event()).ui.theme, Theme::Dark);

    bridge.close_window(SETTINGS, second_build);
    assert!(
        bridge.window(SETTINGS).is_err(),
        "the newer window's own close kept it"
    );
}
