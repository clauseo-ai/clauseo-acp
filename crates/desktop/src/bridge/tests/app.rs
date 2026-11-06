//! The app-wide feed on the window's channel (`subscribe_app`): what reaches it, a restart the
//! bridge makes, a reload, and two windows (`main` and `settings`) opening and closing.

use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use clauseo_protocol::{SettingsView, Theme, ToolsPatch, ToolsSettingsView, UiPatch};

use super::*;
use crate::test_support::{Messages, STEP_WAIT, collector};

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

/// A build of the window `label` that worked: numbered, then recorded.
fn build(bridge: &Bridge, label: &str) -> u64 {
    let number = bridge.number_build();
    bridge.window_built(label, number);
    number
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
        dropped.recv_timeout(STEP_WAIT),
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
        dropped.recv_timeout(STEP_WAIT),
        Err(RecvTimeoutError::Disconnected),
        "the old window's limits pump outlived the reload"
    );
    assert_eq!(old.limits_received(), 1);
    assert_eq!(old.drain(), Vec::<String>::new());
}

/// The settings window's `SetToolSettings` reaches both windows' app feeds as `settings_changed`
/// carrying the switches, as a `SetUiSettings` does, and changes the switch it names alone. The
/// settings window gets no list with it.
#[test]
fn a_tool_switch_from_the_settings_window_reaches_both_windows() {
    let scratch = Scratch::new("app-tools", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let (main_channel, main) = collector();
    let (settings_channel, settings_page) = collector();
    block_on(bridge.open_stream(MAIN, main_channel)).unwrap();
    block_on(bridge.open_stream(SETTINGS, settings_channel)).unwrap();
    block_on(bridge.subscribe_app(MAIN)).unwrap();
    block_on(bridge.subscribe_app(SETTINGS)).unwrap();
    first_frames(&main);
    let before = settings(settings_page.next_event()).tools;

    let patch = ToolsPatch {
        web_search: Some(!before.web_search),
        ..ToolsPatch::default()
    };
    let command = HostCommand::SetToolSettings { patch };
    block_on(bridge.dispatch(SETTINGS, envelope(command))).unwrap();
    let expected = ToolsSettingsView {
        web_search: !before.web_search,
        browser: before.browser,
    };
    assert_eq!(settings(main.next_event()).tools, expected);
    assert_eq!(settings(settings_page.next_event()).tools, expected);
    assert_eq!(settings_page.drain(), Vec::<String>::new());
}

/// `main` and `settings` each follow the app feed on their own channel, and a change the
/// settings window makes reaches both. `main` gets every app event: the three first frames and
/// the thread list's changes. The settings window gets `settings_changed` alone of the app feed
/// (0028): the first frame's settings and each change, never a matter or thread list. Both get
/// the limits (U12), and the settings window's acks count its settings and limits messages.
#[test]
fn both_windows_get_the_settings_change_and_the_limits_and_only_main_the_lists() {
    let scratch = Scratch::new("app-two-windows", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let (main_channel, main) = collector();
    let (settings_channel, settings_page) = collector();
    block_on(bridge.open_stream(MAIN, main_channel)).unwrap();
    block_on(bridge.open_stream(SETTINGS, settings_channel)).unwrap();
    block_on(bridge.subscribe_app(MAIN)).unwrap();
    block_on(bridge.subscribe_app(SETTINGS)).unwrap();
    assert_eq!(first_frames(&main).ui.theme, Theme::System);
    assert_eq!(settings(settings_page.next_event()).ui.theme, Theme::System);

    // A new thread changes the lists: main hears of it, the settings window does not.
    let _thread = host.thread();
    block_on(bridge.dispatch(SETTINGS, set_theme(Theme::Dark))).unwrap();
    let mut lists = 0;
    let theme = loop {
        match main.next_event() {
            HostEvent::SettingsChanged(changed) => break changed.ui.theme,
            HostEvent::ThreadsChanged(_) | HostEvent::MattersChanged(_) => lists += 1,
            other => panic!("main got {other:?}"),
        }
    };
    assert_eq!(theme, Theme::Dark);
    assert!(lists > 0, "main got no change to the lists");
    // In order: had a list change reached the settings window, it would come before this.
    assert_eq!(settings(settings_page.next_event()).ui.theme, Theme::Dark);
    assert_eq!(settings_page.drain(), Vec::<String>::new());
    // The limits pass on both channels since U12: each starts with the current view.
    for (label, page) in [("main", &main), ("settings", &settings_page)] {
        let view = page.next_limits();
        assert!(
            matches!(view, HostEvent::LimitsViewChanged(_)),
            "{label}: {view:?}"
        );
    }
    // Three messages sent: the first frame, the change and the limits view.
    assert_eq!(settings_page.limits_received(), 1);
    let refusal = block_on(bridge.ack(SETTINGS, Vec::new(), 4)).unwrap_err();
    assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{refusal:?}");
    block_on(bridge.ack(SETTINGS, Vec::new(), 3)).unwrap();
    let window = bridge.window(SETTINGS).unwrap();
    assert_eq!(lock(&window).state().unacked_bytes(), 0);
}

/// What each label's channel carries (`AppEvents`, 0028, U12): `main` every app and limits
/// event, the settings window `settings_changed`, `connectors_changed` and the limits, and any
/// other label neither.
/// So a window of another label that follows the app feed gets nothing of it, while the
/// settings window gets each settings change.
#[test]
fn main_carries_every_feed_event_settings_its_own_and_the_limits_another_label_none() {
    let settings_changed = r#"{"settings_changed":{}}"#;
    let threads_changed = r#"{"threads_changed":{}}"#;
    // The folder scan's progress (R20a): the room in `main` is its only reader.
    let folder_scanned = r#"{"folder_scanned":{}}"#;
    let limits = r#"{"limits_view_changed":{}}"#;
    // R24: Settings' Connectors section reads the connector state; the hint is the room's.
    let connectors_changed = r#"{"connectors_changed":{}}"#;
    let case_law_hint = r#"{"case_law_hint":{}}"#;
    let carries = |label: &str, feed, json| pump::AppEvents::of(label).carries(feed, json);
    for (feed, json) in [
        (Feed::App, settings_changed),
        (Feed::App, threads_changed),
        (Feed::App, folder_scanned),
        (Feed::App, connectors_changed),
        (Feed::App, case_law_hint),
        (Feed::Limits, limits),
    ] {
        assert!(carries(MAIN, feed, json), "main: {json}");
        assert!(!carries("other", feed, json), "other: {json}");
    }
    assert!(carries(SETTINGS, Feed::App, settings_changed));
    assert!(carries(SETTINGS, Feed::App, connectors_changed));
    assert!(!carries(SETTINGS, Feed::App, case_law_hint));
    assert!(!carries(SETTINGS, Feed::App, threads_changed));
    assert!(!carries(SETTINGS, Feed::App, folder_scanned));
    assert!(carries(SETTINGS, Feed::Limits, limits));

    let scratch = Scratch::new("app-other-label", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let (settings_channel, settings_page) = collector();
    let (other_channel, other) = collector();
    block_on(bridge.open_stream(SETTINGS, settings_channel)).unwrap();
    block_on(bridge.open_stream("other", other_channel)).unwrap();
    block_on(bridge.subscribe_app("other")).unwrap();
    block_on(bridge.subscribe_app(SETTINGS)).unwrap();
    assert_eq!(settings(settings_page.next_event()).ui.theme, Theme::System);
    block_on(bridge.dispatch(SETTINGS, set_theme(Theme::Dark))).unwrap();
    assert_eq!(settings(settings_page.next_event()).ui.theme, Theme::Dark);
    // Subscribed first, the other window's pump has had the change at least as long; the table
    // above pins the rule itself.
    assert_eq!(other.drain(), Vec::<String>::new());
    assert_eq!(other.limits_received(), 0);
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
    first_frames(&main);
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
    first_frames(&main);
    let built = build(&bridge, SETTINGS);
    let (channel, page) = collector();
    block_on(bridge.open_stream(SETTINGS, channel)).unwrap();
    block_on(bridge.subscribe_app(SETTINGS)).unwrap();
    // Its one first frame: the settings.
    let _current = settings(page.next_event());
    let window = bridge.window(SETTINGS).unwrap();
    let dropped = pump::feed_dropped(&window, Feed::App).unwrap();
    let gone = Arc::downgrade(&window);
    drop(window);

    bridge.close_window(SETTINGS, built);
    assert_eq!(
        dropped.recv_timeout(STEP_WAIT),
        Err(RecvTimeoutError::Disconnected),
        "the closed window's app pump outlived it"
    );
    // No task holds the window strongly: once the bridge lets go, it drops, and with it the
    // watchdog (`Window`'s `Drop`). A watchdog tick in flight holds it for that tick only.
    let (_keep, idle) = std::sync::mpsc::channel::<()>();
    let tick = Duration::from_millis(10);
    for _ in 0..STEP_WAIT.as_millis() / tick.as_millis() {
        if gone.strong_count() == 0 {
            break;
        }
        let _tick = idle.recv_timeout(tick);
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
    let built = bridge.number_build();
    bridge.window_built(MAIN, built);
    let page = open(&bridge, MAIN);
    let dropped = pump::feed_dropped(&bridge.window(MAIN).unwrap(), Feed::Limits).unwrap();
    bridge.close_window(MAIN, built);
    // The pump never sees another event, so only the abort can end it.
    assert_eq!(
        dropped.recv_timeout(STEP_WAIT),
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
    let first_build = build(&bridge, SETTINGS);
    let (first, _old) = collector();
    block_on(bridge.open_stream(SETTINGS, first)).unwrap();
    // Rebuilt before the old window's close lands: the old stream is stale and goes.
    let second_build = build(&bridge, SETTINGS);
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
    assert_eq!(settings(new.next_event()).ui.theme, Theme::System);
    block_on(bridge.dispatch(SETTINGS, set_theme(Theme::Dark))).unwrap();
    assert_eq!(settings(new.next_event()).ui.theme, Theme::Dark);

    bridge.close_window(SETTINGS, second_build);
    assert!(
        bridge.window(SETTINGS).is_err(),
        "the newer window's own close kept it"
    );
}
