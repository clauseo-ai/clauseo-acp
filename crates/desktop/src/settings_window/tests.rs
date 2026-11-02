//! The settings window on the mock runtime: one window however often it is opened, its page,
//! and what its destruction does to the bridge. The mock runtime of tauri 2.12.4 never removes
//! a destroyed window from the app (`test/mock_runtime.rs`: `DestroyWindow` drops only its own
//! record, and no `Destroyed` event is sent), so a rebuild after a close is not reachable here;
//! `bridge/tests/app.rs` covers the streams of a rebuilt window.

use tauri::test::{MockRuntime, mock_builder};

use super::*;
use crate::test_support::{Scratch, collector};

/// An app on the mock runtime with the app's context and `bridge` managed.
fn app(bridge: Option<Bridge>) -> tauri::App<MockRuntime> {
    let app = mock_builder()
        .build(crate::context::<MockRuntime>())
        .unwrap();
    if let Some(bridge) = bridge {
        app.manage(bridge);
    }
    app
}

#[test]
fn opening_twice_gives_one_settings_window_showing_its_page() {
    let app = app(None);
    open_or_focus(&app).unwrap();
    let first = app.get_webview_window(SETTINGS_WINDOW).unwrap();
    open_or_focus(&app).unwrap();
    let windows = app.webview_windows();
    assert_eq!(windows.len(), 1, "{:?}", windows.keys());
    let second = &windows[SETTINGS_WINDOW];
    assert_eq!(first.label(), second.label());
    let url = second.url().unwrap();
    assert!(url.path().ends_with("/settings.html"), "{url}");
    assert!(
        app.get_webview_window(MAIN_WINDOW).is_none(),
        "settings is not main"
    );
}

/// The settings window's destruction drops its stream in the bridge and leaves main's.
#[test]
fn a_destroyed_window_drops_its_stream_only() {
    let scratch = Scratch::new("settings-closed", &[]);
    let host = scratch.start(Vec::new());
    let app = app(Some(host.bridge()));
    let bridge = app.state::<Bridge>();
    let main_built = number_build(&app, MAIN_WINDOW);
    let (main_channel, _main) = collector();
    futures::executor::block_on(bridge.open_stream(MAIN_WINDOW, main_channel)).unwrap();
    let built = number_build(&app, SETTINGS_WINDOW);
    assert!(built > main_built, "{built:?} {main_built:?}");
    let (channel, _page) = collector();
    futures::executor::block_on(bridge.open_stream(SETTINGS_WINDOW, channel)).unwrap();

    closed(&app, SETTINGS_WINDOW, built);
    let refusal = futures::executor::block_on(bridge.subscribe_app(SETTINGS_WINDOW));
    assert!(refusal.is_err(), "the closed window kept its stream");
    futures::executor::block_on(bridge.subscribe_app(MAIN_WINDOW)).unwrap();

    closed(&app, MAIN_WINDOW, main_built);
    assert!(futures::executor::block_on(bridge.subscribe_app(MAIN_WINDOW)).is_err());
}
