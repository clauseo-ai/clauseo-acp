//! macOS: a widening question as an `NSAlert`, run modally on the main thread: Don't allow and
//! Allow, with the suppression checkbox titled "Remember for this matter". Escape presses Don't
//! allow: its key equivalent, which takes the place of the Return `NSAlert` gives its first
//! button, so Return presses nothing and never allows. The alert answers which button ended it,
//! not how, so Escape honours the checkbox as a click on Don't allow does (Don't allow for the
//! matter when it is ticked). Any other end of the modal session refuses. objc2-app-kit's typed
//! API, no `unsafe` (decision record 0054).

use clauseo_host::DialogAnswer;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSAlert, NSAlertStyle, NSControlStateValueOn};
use objc2_foundation::NSString;
use tauri::{AppHandle, Runtime};

use super::answer::{alert_press, from_press};
use super::words::AskWords;

/// Asks `words`'s question on the main thread and waits for the answer. An alert that never
/// shows (the event loop gone) refuses.
pub(super) fn ask<R: Runtime>(app: &AppHandle<R>, words: &AskWords) -> DialogAnswer {
    let words = words.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    // Not posted: the closure and its sender drop, and the receive below refuses.
    let _unposted = app.run_on_main_thread(move || {
        let answer = MainThreadMarker::new().map_or(DialogAnswer::Refuse, |mtm| run(mtm, &words));
        // The caller waits for this; it may have gone, and then nobody reads it.
        let _gone = tx.send(answer);
    });
    rx.recv().unwrap_or(DialogAnswer::Refuse)
}

/// Builds the alert and runs it modally.
fn run(mtm: MainThreadMarker, words: &AskWords) -> DialogAnswer {
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.setMessageText(&NSString::from_str(&words.title));
    alert.setInformativeText(&NSString::from_str(&words.message));
    // The first button added is ALERT_REFUSE, the second ALERT_ALLOW.
    let refuse = alert.addButtonWithTitle(&NSString::from_str(&words.refuse));
    // Escape in place of Return: Escape refuses, Return presses nothing.
    refuse.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    let _allow = alert.addButtonWithTitle(&NSString::from_str(&words.allow));
    alert.setShowsSuppressionButton(true);
    let checkbox = alert.suppressionButton();
    if let Some(checkbox) = &checkbox {
        checkbox.setTitle(&NSString::from_str(&words.remember));
    }
    let response = alert.runModal();
    let remember = checkbox.is_some_and(|checkbox| checkbox.state() == NSControlStateValueOn);
    from_press(alert_press(response), remember)
}
