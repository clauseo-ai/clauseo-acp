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
//! - Best effort: it never fails or blocks the follower beyond one local `HKCU` write on the
//!   setup host thread (not the UI thread). A failed write logs one warning with the Win32 code
//!   and is not retried; the next different language tries again.
//! - Unit-test builds never write the real key: their [`follow`] writes nowhere, and the Windows
//!   test writes a scratch key through [`windows::write_value`], whose key is a parameter.
//! - Readers, for the words Rust draws without the bridge: [`current`] is the language the last
//!   [`follow`] saw (English before the first), for the setup confirms; `saved` (Windows) reads
//!   the value back before Tauri starts, for the `WebView2` check's dialog, `None` when it is
//!   missing or no language's tag.

use std::sync::{Mutex, PoisonError};

use clauseo_settings::Language;

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

/// The language whose [`tag`] is `tag`, as the words Rust draws name it.
#[cfg(any(windows, test))]
pub(crate) fn from_tag(tag: &str) -> Option<clauseo_protocol::Language> {
    match tag {
        "en" => Some(clauseo_protocol::Language::En),
        "hi" => Some(clauseo_protocol::Language::Hi),
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

/// The effective language the last [`follow`] saw, English before the first (module docs).
pub(crate) fn current() -> clauseo_protocol::Language {
    let follower = APP.lock().unwrap_or_else(PoisonError::into_inner);
    shown(follower.current().unwrap_or(Language::En))
}

/// The language the registry holds, read before Tauri starts: `None` when the value is missing,
/// unreadable or no language's tag. It reads the real key, never in unit tests.
#[cfg(windows)]
pub(crate) fn saved() -> Option<clauseo_protocol::Language> {
    windows::read_value(KEY).as_deref().and_then(from_tag)
}
