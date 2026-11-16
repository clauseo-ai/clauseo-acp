//! The app's UI language in the Windows registry, for the NSIS uninstaller and any repair or
//! re-run of the installer, which read it to speak the lawyer's language.
//!
//! Contract:
//! - Windows only: `HKCU\Software\Clauseo`, value `UiLanguage`, `REG_SZ`, the effective
//!   `ui.language` (a firm policy's locked one included) as the settings crate serializes it
//!   (`en`, `hi`). [`tag`] is an exhaustive match, so a new language does not compile without
//!   one. Elsewhere [`follow`] keeps the language in memory and writes nothing.
//! - [`follow`] runs on each settings read of `setup/start.rs` `follow_settings`: the first, at
//!   start (which covers first run), and one per `SettingsChanged`. It writes when the language
//!   differs from the last one it wrote or tried, never when it is unchanged.
//! - [`seed_from_registry`] reads it once, at start, before the host and the first [`follow`]
//!   see the settings: when the lawyer's `settings.json` holds no `ui.language` at all
//!   (`clauseo_settings::user_sets`, so a written `en` counts) and no firm policy locks it, a
//!   known tag is saved there as their `ui.language` (`Effective::apply_ui_patch`). A fresh
//!   data dir then speaks the language the installer recorded, instead of writing `en` over it.
//!   An empty or unknown value is ignored. When it is absent or unknown (a per-machine install
//!   elevated with another admin's credentials writes the other account's key), Windows'
//!   display language (`GetUserDefaultUILanguage`) is seeded the same way: `hi` when its
//!   primary language is Hindi, `en` otherwise. Precedence: the lawyer's value, a policy lock,
//!   `UiLanguage`, the display language, the default. Off Windows nothing is read.
//! - Best effort: it never fails or blocks the follower beyond one local `HKCU` write on the
//!   setup host thread (not the UI thread). A failed write logs one warning with the Win32 code
//!   and is not retried; the next different language tries again.
//! - [`first_paint_script`] reads it (`saved`, Windows) on the main thread before the main
//!   window exists, and [`init_script`] gives the page that language before any page script
//!   runs (`window.__CLAUSEO_UI_LANGUAGE__` and `<html lang>`), so the first paint is already in
//!   the language the app settles on (`ui/app/src/display/connect.ts`). The host's settings
//!   still win once they arrive. Only a known tag makes a script; none or an unknown one gives
//!   none, and the page starts as before.
//! - Unit-test builds never touch the real key: their [`follow`] writes nowhere and their
//!   [`seed_from_registry`] reads neither the key nor the display language; the Windows test
//!   writes and reads a scratch key through [`windows::write_value`] and
//!   [`windows::read_value`], whose key is a parameter.
//! - Readers, for the words Rust draws without the bridge: [`current`] is the language the last
//!   [`follow`] saw (English before the first), for the setup confirms; `saved` (Windows) reads
//!   the value back before Tauri starts, for the `WebView2` check's dialog, `None` when it is
//!   missing or no language's tag.

use std::sync::{Mutex, PoisonError};

use clauseo_settings::{Effective, Files, Language, SystemFiles, UiPatch, user_sets};

#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;

/// The key under `HKCU`, which the installer's `MANUKEY` also names.
#[cfg_attr(
    not(windows),
    expect(dead_code, reason = "only Windows writes and reads")
)]
const KEY: &str = "Software\\Clauseo";

/// `language`'s tag, as `Language`'s serde form writes it.
pub(crate) fn tag(language: Language) -> &'static str {
    match language {
        Language::En => "en",
        Language::Hi => "hi",
    }
}

/// The language whose [`tag`] is `tag`, `None` for any other text.
pub(crate) fn language(tag: &str) -> Option<Language> {
    match tag {
        "en" => Some(Language::En),
        "hi" => Some(Language::Hi),
        _ => None,
    }
}

/// `language` as the words Rust draws name it.
fn shown(language: Language) -> clauseo_protocol::Language {
    match language {
        Language::En => clauseo_protocol::Language::En,
        Language::Hi => clauseo_protocol::Language::Hi,
    }
}

/// Where a tag is written. `Err` carries the Win32 code.
pub(crate) trait Writer {
    /// Writes `tag` as the value.
    fn write(&mut self, tag: &str) -> Result<(), u32>;
}

/// Writes when the language changes.
pub(crate) struct Follower<W> {
    /// The language last written or tried, `None` before the first.
    written: Option<Language>,
    writer: W,
}

impl<W: Writer> Follower<W> {
    pub(crate) const fn new(writer: W) -> Self {
        Self {
            written: None,
            writer,
        }
    }

    /// The language last written or tried, `None` before the first.
    pub(crate) fn current(&self) -> Option<Language> {
        self.written
    }

    /// Writes `language` unless it is the one last written or tried.
    pub(crate) fn follow(&mut self, language: Language) {
        if self.written == Some(language) {
            return;
        }
        self.written = Some(language);
        if let Err(code) = self.writer.write(tag(language)) {
            tracing::warn!(code, "the UI language was not written to the registry");
        }
    }
}

/// The app's writer: `HKCU\`[`KEY`] on Windows outside unit tests, nowhere otherwise.
struct App;

impl Writer for App {
    #[cfg(all(windows, not(test)))]
    fn write(&mut self, tag: &str) -> Result<(), u32> {
        windows::write_value(KEY, tag)
    }

    #[cfg(not(all(windows, not(test))))]
    fn write(&mut self, _tag: &str) -> Result<(), u32> {
        Ok(())
    }
}

/// The app's follower, one for the process.
static APP: Mutex<Follower<App>> = Mutex::new(Follower::new(App));

/// Writes the effective `language` to the registry if it changed (module docs).
pub(crate) fn follow(language: Language) {
    APP.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .follow(language);
}

/// Where the recorded tag and Windows' display language are read.
pub(crate) trait Reader {
    /// The recorded value, as written; `None` when there is none or it cannot be read.
    fn read(&self) -> Option<String>;

    /// Windows' display language as a `LANGID`; `None` where there is none to read.
    fn display_language(&self) -> Option<u16>;
}

/// `LANG_HINDI`, the primary language of a Hindi `LANGID` (`0x0439`).
const LANG_HINDI: u16 = 0x39;

/// The language a display `LANGID` gives: Hindi when its primary language (`PRIMARYLANGID`,
/// the low 10 bits) is Hindi, English otherwise.
pub(crate) const fn display_language(langid: u16) -> Language {
    if langid & 0x3ff == LANG_HINDI {
        Language::Hi
    } else {
        Language::En
    }
}

impl Reader for App {
    #[cfg(all(windows, not(test)))]
    fn read(&self) -> Option<String> {
        windows::read_value(KEY)
    }

    #[cfg(not(all(windows, not(test))))]
    fn read(&self) -> Option<String> {
        None
    }

    #[cfg(all(windows, not(test)))]
    fn display_language(&self) -> Option<u16> {
        Some(windows::display_language())
    }

    #[cfg(not(all(windows, not(test))))]
    fn display_language(&self) -> Option<u16> {
        None
    }
}

/// Saves the recorded language, or else the display language, as the lawyer's `ui.language`
/// when they have none and no policy locks it (module docs). Returns the language it saved.
pub(crate) fn seed(
    effective: &mut Effective,
    files: &dyn Files,
    reader: &dyn Reader,
) -> Option<Language> {
    const KEY: &str = "ui.language";
    if effective.is_locked(KEY) {
        return None;
    }
    let recorded = reader.read().as_deref().and_then(language);
    let from_display = recorded.is_none();
    let language = recorded.or_else(|| reader.display_language().map(display_language))?;
    if user_sets(effective.locations(), files, KEY) {
        return None;
    }
    let patch = UiPatch {
        language: Some(language),
        ..UiPatch::default()
    };
    match effective.apply_ui_patch(files, &patch) {
        Ok(()) => {
            if from_display {
                tracing::info!("the UI language came from Windows' display language");
            } else {
                tracing::info!("the UI language came from the installer's record");
            }
            Some(language)
        }
        Err(err) => {
            tracing::warn!(%err, "the first-run UI language was not saved to the settings");
            None
        }
    }
}

/// The script that gives the page `language` (`saved`'s, on Windows) before any page script
/// runs: the global the display store starts from, and `<html lang>` where the element already
/// exists. `None` for no language, so the page starts as it did without one. The tag is the
/// language's serde form (`en`, `hi`), written as a JSON string.
pub(crate) fn init_script(language: Option<clauseo_protocol::Language>) -> Option<String> {
    let quoted = serde_json::to_string(&language?).ok()?;
    Some(format!(
        "window.__CLAUSEO_UI_LANGUAGE__ = {quoted};\n\
         if (document.documentElement) document.documentElement.lang = {quoted};\n"
    ))
}

/// [`seed`] from `HKCU\`[`KEY`] or the display language into `settings.json`, at start. Off
/// Windows it reads nothing.
pub(crate) fn seed_from_registry(effective: &mut Effective) {
    let _seeded = seed(effective, &SystemFiles, &App);
}

/// [`init_script`] for the language the registry records (`saved`), for the main window. Only
/// Windows records one; elsewhere there is none, and the page starts as before.
pub(crate) fn first_paint_script() -> Option<String> {
    #[cfg(windows)]
    let recorded = saved();
    #[cfg(not(windows))]
    let recorded = None;
    init_script(recorded)
}

/// The effective language the last [`follow`] saw, English before the first (module docs).
pub(crate) fn current() -> clauseo_protocol::Language {
    let follower = APP.lock().unwrap_or_else(PoisonError::into_inner);
    shown(follower.current().unwrap_or(Language::En))
}

/// The language the registry holds, read before Tauri starts: `None` when the value is missing,
/// unreadable or no language's tag. It reads the real key, never in unit tests.
#[cfg(windows)]
pub(crate) fn saved() -> Option<clauseo_protocol::Language> {
    windows::read_value(KEY)
        .as_deref()
        .and_then(language)
        .map(shown)
}
