//! The browser's commands (0029) are the main window's alone: the Chrome for Testing download
//! and its cancel, and the "Try again" that re-reads the firm's browser policy. Split from
//! `admit.rs` for its length.

use serde_json::json;

use super::*;

/// A command as the page writes it.
fn command(body: serde_json::Value) -> HostCommand {
    let text = body.to_string();
    serde_json::from_value(body).unwrap_or_else(|err| panic!("{text}: {err}"))
}

/// The Chrome for Testing download (0029) is the main window's: its consent dialog sends
/// `install_browser` and its Cancel `cancel_browser_install`. The settings window and any other
/// label are refused both with `command.invalid`, before the host sees them.
#[test]
fn only_the_main_window_starts_or_cancels_the_browser_download() {
    let scratch = Scratch::new("admit-browser", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let install = command(json!({"install_browser": {}}));
    let cancel = command(json!({"cancel_browser_install": {}}));
    for browser in [&install, &cancel] {
        assert!(bridge.admit(MAIN, browser).is_ok(), "{browser:?}");
        for label in [SETTINGS, "other"] {
            let refusal = block_on(bridge.dispatch(label, envelope(browser.clone()))).unwrap_err();
            assert_eq!(
                refusal.code(),
                ErrorCode::CommandInvalid,
                "{label} {browser:?}"
            );
        }
    }
    // With no download running, main's cancel reaches the host and does nothing.
    let outcome = block_on(bridge.dispatch(MAIN, envelope(cancel)));
    assert!(matches!(outcome, Ok(CommandOutcome::Done)), "{outcome:?}");
}

/// The browser row's "Try again" on a policy Clauseo could not read (0029) is the main
/// window's: `recheck_browser_policy` passes from main and reaches the host, which answers
/// `Done`; the settings window and any other label are refused with `command.invalid`, before
/// the host sees it.
#[test]
fn only_the_main_window_rechecks_the_browser_policy() {
    let scratch = Scratch::new("admit-recheck", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let recheck = command(json!({"recheck_browser_policy": {}}));
    let outcome = block_on(bridge.dispatch(MAIN, envelope(recheck.clone())));
    assert!(matches!(outcome, Ok(CommandOutcome::Done)), "{outcome:?}");
    for label in [SETTINGS, "other"] {
        let refusal = block_on(bridge.dispatch(label, envelope(recheck.clone()))).unwrap_err();
        assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{label}");
    }
}
