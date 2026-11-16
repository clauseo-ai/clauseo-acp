//! `main`'s full-screen value: what a resize reads, and what its followers get.

use std::sync::Arc;

use futures::channel::mpsc::UnboundedReceiver;
use futures::{FutureExt as _, StreamExt as _};
use tauri::test::{MockRuntime, mock_builder};
use tauri::{Manager as _, PhysicalSize, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use super::*;
use crate::host_start::HostCell;

const TRUE: &str = r#"{"window_full_screen":true}"#;
const FALSE: &str = r#"{"window_full_screen":false}"#;

/// The texts `feed` holds now, without waiting.
fn sent(feed: &mut UnboundedReceiver<Arc<str>>) -> Vec<String> {
    let mut texts = Vec::new();
    while let Some(Some(text)) = feed.next().now_or_never() {
        texts.push(text.to_string());
    }
    texts
}

/// A change reaches each follower as one message; the same value again sends nothing.
#[test]
fn a_change_reaches_each_follower_once_and_a_repeat_sends_nothing() {
    let full_screen = FullScreen::default();
    let mut first = full_screen.follow();
    let mut second = full_screen.follow();
    assert!(
        sent(&mut first).is_empty(),
        "no value before the first read"
    );

    assert!(full_screen.set(true));
    assert!(!full_screen.set(true), "the same value is no change");
    assert_eq!(sent(&mut first), [TRUE]);
    assert_eq!(sent(&mut second), [TRUE]);

    assert!(full_screen.set(false));
    assert!(!full_screen.set(false));
    assert_eq!(sent(&mut first), [FALSE]);
    assert_eq!(sent(&mut second), [FALSE]);
}

/// A new follower (a `subscribe_app`, a restart) starts with the last value, then each change.
#[test]
fn a_follower_starts_with_the_last_value() {
    let full_screen = FullScreen::default();
    assert!(full_screen.set(true));
    let mut feed = full_screen.follow();
    assert_eq!(sent(&mut feed), [TRUE]);
    assert!(full_screen.set(false));
    assert_eq!(sent(&mut feed), [FALSE]);
    // A follower that is gone is dropped, and the others still hear.
    drop(feed);
    let mut later = full_screen.follow();
    assert_eq!(sent(&mut later), [FALSE]);
    assert!(full_screen.set(true));
    assert_eq!(sent(&mut later), [TRUE]);
}

/// Only a resize of `main` reads its full screen and hands it to the bridge; another window's
/// resize, or another event of `main`, reads nothing. The mock runtime's windows are never full
/// screen.
#[test]
fn a_resize_of_main_reads_its_full_screen_and_nothing_else_does() {
    let (cell, _start) = HostCell::new();
    let app = mock_builder()
        .build(crate::context::<MockRuntime>())
        .unwrap();
    app.manage(Bridge::new(Arc::clone(&cell)));
    let window = |label: &str| {
        WebviewWindowBuilder::new(&app, label, WebviewUrl::default())
            .build()
            .unwrap()
    };
    let main = window(MAIN_WINDOW);
    let settings = window(settings_window::SETTINGS_WINDOW);
    let resized = WindowEvent::Resized(PhysicalSize::new(1512, 950));

    assert_eq!(on_main_resized(&settings.as_ref().window(), &resized), None);
    assert_eq!(
        on_main_resized(&main.as_ref().window(), &WindowEvent::Focused(true)),
        None
    );
    assert_eq!(
        on_main_resized(&main.as_ref().window(), &resized),
        Some(false)
    );
    // The bridge holds it: the same value again is no change, a new one is.
    let bridge = app.state::<Bridge>();
    assert!(!bridge.window_full_screen(false));
    assert!(bridge.window_full_screen(true));
}
