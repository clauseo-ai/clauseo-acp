//! macOS and Linux: a widening question as rfd's message dialog with three buttons (rfd draws
//! no checkbox), built on the main thread as `dialogs::confirm` builds its own, and awaited on
//! the calling thread.

use clauseo_host::DialogAnswer;
use rfd::MessageLevel;
use tauri::{AppHandle, Runtime, WebviewWindow};

use super::answer::{ThreeButtons, order};
use super::words::AskWords;

/// Asks `words`'s question over `main`, when there is one, and waits for the answer. A dialog
/// that never shows (the event loop gone) refuses.
pub(super) fn ask<R: Runtime>(
    app: &AppHandle<R>,
    main: Option<WebviewWindow<R>>,
    words: &AskWords,
) -> DialogAnswer {
    let three = ThreeButtons::new(
        order(cfg!(target_os = "macos")),
        &words.allow_once,
        &words.allow_always,
        &words.refuse,
    );
    let buttons = three.buttons();
    let (title, message) = (words.title.clone(), words.message.clone());
    let (tx, rx) = std::sync::mpsc::channel();
    // Not posted: the closure and its sender drop, and the receive below refuses.
    let _unposted = app.run_on_main_thread(move || {
        let mut dialog = rfd::AsyncMessageDialog::new()
            .set_title(&title)
            .set_description(&message)
            .set_level(MessageLevel::Warning)
            .set_buttons(buttons);
        if let Some(main) = &main {
            dialog = dialog.set_parent(main);
        }
        // The caller waits for this; it may have gone, and then nobody reads it.
        let _gone = tx.send(dialog.show());
    });
    match rx.recv() {
        Ok(shown) => three.answer(&futures::executor::block_on(shown)),
        Err(_) => DialogAnswer::Refuse,
    }
}
