//! Windows: Quit Clauseo in the main window's system menu (Alt+Space), below a separator, with
//! "Ctrl+Q" as its key text (the page's own Ctrl+Q, `ui/app/src/room/quit-key.ts`).
//!
//! Contract:
//! - Choosing it quits as `quit_app` and the Mac's Cmd+Q do: `AppHandle::exit(0)`, so
//!   `RunEvent::ExitRequested` stops the installs, sign-ins, agents and host first (`lib.rs`,
//!   `quit.rs`). No confirm: the Mac's quit asks none either.
//! - Its words are the Mac bar's Quit (MN-07, `menu::words::quit`): first in the language
//!   the installer recorded (`ui_language::saved`), then `menu::follow_language` renames it
//!   in the settings' language once the host has started and on each change.
//! - The window's subclass (`windows/quit_item.rs`) catches `WM_SYSCOMMAND` with [`QUIT_ID`], its
//!   low four bits masked as Windows asks ([`is_quit`]); every other message goes on unchanged.
//! - Only the main window, only Windows; best effort: a failed call leaves the menu as it was,
//!   and Ctrl+Q and the window's close still quit.
//!
//! Right-click on the page's title bar opens no system menu: the bar is the webview's (wry's
//! `WebView2` child window takes the clicks), Tauri's drag script answers the left button only,
//! and the maximise cover drops the right button's non-client messages (`windows.rs`).

use clauseo_protocol::Language;

use crate::menu::words;

#[cfg(windows)]
pub(crate) use super::windows::quit_item::{add, relabel};

/// The item's command id: below the system's own commands (`SC_SIZE`, 0xF000, and up), its low
/// four bits clear, since Windows uses those itself in `WM_SYSCOMMAND`.
pub(crate) const QUIT_ID: u32 = 0x0C50;

/// Whether a `WM_SYSCOMMAND`'s `wparam` is the Quit item, its low four bits masked.
pub(crate) const fn is_quit(wparam: usize) -> bool {
    wparam & 0xFFF0 == QUIT_ID as usize
}

/// The item's text in `language`: Quit Clauseo, a tab, then its key, which Windows draws
/// right-aligned.
pub(crate) fn label(language: Language) -> String {
    format!("{}\tCtrl+Q", words::quit(language))
}

/// `text` as a NUL-terminated UTF-16 string, for the Win32 menu calls.
pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::words::Key;

    #[test]
    fn the_id_is_below_the_system_commands_with_its_low_bits_clear() {
        const { assert!(QUIT_ID.trailing_zeros() >= 4 && QUIT_ID > 0 && QUIT_ID < 0xF000) };
    }

    #[test]
    fn the_quit_is_found_with_any_low_bits_and_nothing_else_is() {
        let id = QUIT_ID as usize;
        for low in 0..16 {
            assert!(is_quit(id | low), "{low}");
        }
        assert!(!is_quit(id + 0x10));
        assert!(!is_quit(id - 0x10));
        assert!(!is_quit(0));
        // SC_CLOSE, SC_MAXIMIZE, SC_RESTORE, SC_KEYMENU
        for system in [0xF060, 0xF030, 0xF120, 0xF100] {
            assert!(!is_quit(system), "{system:#x}");
        }
    }

    #[test]
    fn the_label_is_the_menu_bars_quit_in_each_language_with_its_key() {
        assert_eq!(label(Language::En), "Quit Clauseo\tCtrl+Q");
        assert_eq!(label(Language::Hi), "Clauseo बंद करें\tCtrl+Q");
        for language in [Language::En, Language::Hi] {
            // an exhaustive match: a new language must be added above
            match language {
                Language::En | Language::Hi => {}
            }
            assert_eq!(
                label(language),
                format!("{}\tCtrl+Q", words::text(language, Key::Quit))
            );
        }
    }

    #[test]
    fn the_wide_text_ends_in_one_nul() {
        let text = wide("Clauseo बंद करें");
        assert_eq!(text.last(), Some(&0));
        assert_eq!(text.iter().filter(|&&unit| unit == 0).count(), 1);
        assert_eq!(
            String::from_utf16(text.split_last().unwrap().1).unwrap(),
            "Clauseo बंद करें"
        );
    }
}
