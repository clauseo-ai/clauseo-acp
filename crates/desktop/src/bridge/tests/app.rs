//! The app-wide feed on the window's channel (`subscribe_app`): what reaches it, a restart the
//! bridge makes, and a reload.

use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use clauseo_protocol::{SettingsView, Theme, UiPatch};

use super::*;

/// A `SetUiSettings` of `theme`.
fn set_theme(theme: Theme) -> CommandEnvelope {
    envelope(HostCommand::SetUiSettings {
        patch: UiPatch {
            theme: Some(theme),
            ..UiPatch::default()
        },
    })
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

    let (channel, messages) = collector();
    bridge.open_stream(MAIN, channel);
    block_on(bridge.subscribe_app(MAIN)).unwrap();
    // It starts with nothing: the window reads the settings itself.
    assert_eq!(messages.drain(), Vec::<String>::new());
    block_on(bridge.dispatch(set_theme(Theme::Dark))).unwrap();
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
    let (channel, messages) = collector();
    bridge.open_stream(MAIN, channel);
    block_on(bridge.subscribe_app(MAIN)).unwrap();
    let window = bridge.window(MAIN).unwrap();

    // Past the cap without the host: a large app event, counted as sent, pauses the window and
    // drops every host subscription.
    let big = format!("{{\"settings_changed\":\"{}\"}}", "x".repeat(300 * 1024));
    let paused = pump::count_app_event(&window, &big);
    let window::Forward::Send {
        messages: sent,
        pause: true,
    } = paused
    else {
        panic!("not paused: {paused:?}");
    };
    // While paused, a change reaches no message.
    block_on(bridge.dispatch(set_theme(Theme::Light))).unwrap();
    assert_eq!(messages.drain(), Vec::<String>::new());

    block_on(bridge.ack(MAIN, Vec::new(), u32::try_from(sent.len()).unwrap())).unwrap();
    let current = settings(messages.next_event());
    assert_eq!(current.ui.theme, Theme::Light);
    block_on(bridge.dispatch(set_theme(Theme::Dark))).unwrap();
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
    let (first, old) = collector();
    bridge.open_stream(MAIN, first);
    block_on(bridge.subscribe_app(MAIN)).unwrap();
    let dropped = pump::app_dropped(&bridge.window(MAIN).unwrap()).unwrap();
    let (second, new) = collector();
    bridge.open_stream(MAIN, second);
    // The pump never sees another event, so only the abort can end it.
    assert_eq!(
        dropped.recv_timeout(Duration::from_secs(20)),
        Err(RecvTimeoutError::Disconnected),
        "the old window's app pump outlived the reload"
    );
    assert_eq!(
        lock(&bridge.window(MAIN).unwrap()).state().app_restart_id(),
        None
    );
    block_on(bridge.dispatch(set_theme(Theme::Dark))).unwrap();
    // The host answers in order: a read after the write comes after any event of it.
    let _read = block_on(bridge.query(HostQuery::GetSettings)).unwrap();
    assert_eq!(old.drain(), Vec::<String>::new());
    assert_eq!(new.drain(), Vec::<String>::new());

    block_on(bridge.subscribe_app(MAIN)).unwrap();
    block_on(bridge.dispatch(set_theme(Theme::Light))).unwrap();
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
    let (channel, messages) = collector();
    bridge.open_stream(MAIN, channel);
    block_on(bridge.subscribe_app(MAIN)).unwrap();
    let window = bridge.window(MAIN).unwrap();
    // One message the page never acks.
    block_on(bridge.dispatch(set_theme(Theme::Dark))).unwrap();
    assert_eq!(settings(messages.next_event()).ui.theme, Theme::Dark);
    let before = lock(&window).state().app_restart_id();

    pump::tick_at(&window, window::WATCHDOG * 2);
    let resent = settings(messages.next_event());
    assert_eq!(resent.ui.theme, Theme::Dark);
    assert_ne!(lock(&window).state().app_restart_id(), before);
    block_on(bridge.dispatch(set_theme(Theme::Light))).unwrap();
    assert_eq!(settings(messages.next_event()).ui.theme, Theme::Light);
}
