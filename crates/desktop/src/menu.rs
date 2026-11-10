//! The Mac's menu bar (D-062): Tauri's default bar, with the app's own commands and its words in
//! the app's language.
//!
//! Contract:
//! - macOS only. Windows and Linux get none: a menu would be a bar on the undecorated window;
//!   there the page's own keys do the same (`ui/app/src/room/room-commands.ts`, `find-key.ts`,
//!   `settings-key.ts`).
//! - The app menu: About, Settings… on Cmd+, (the settings window, `settings_window.rs`),
//!   Services, Hide, Hide Others, Show All, Quit. File: New conversation on Cmd+N, New matter on
//!   Shift+Cmd+N, Close Window. Edit: Undo, Redo, Cut, Copy, Paste, Select All (the system's
//!   own, so the webview's fields keep them), then Find on Cmd+K. View: Enter Full Screen.
//!   Window: Minimize, Zoom, Close Window. Help, which macOS fills.
//! - New conversation, New matter and Find are the main window's page's: the menu raises that
//!   window and sends its page a `clauseo-menu` event whose detail names the command
//!   ([`script`], a fixed string, never the caller's text), as `settings_window.rs` sets a hash.
//!   The page runs the same code as its keys and buttons, and does nothing where it can't (no
//!   matter open, a dialog holding the keys). No capability is granted for it: the script runs
//!   from Rust.
//! - The words are English (`words`).

pub(crate) mod words;

#[cfg(test)]
mod tests;

/// A command the menu sends the main window's page (`MenuCommand` in `room-commands.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageCommand {
    /// New conversation in the open matter.
    NewConversation,
    /// The New matter dialog.
    NewMatter,
    /// The focus in the rail's Find.
    Find,
}

impl PageCommand {
    /// Every command.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 3] = [Self::NewConversation, Self::NewMatter, Self::Find];

    /// Its menu item's id, which is also the name the page reads.
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::NewConversation => "new_conversation",
            Self::NewMatter => "new_matter",
            Self::Find => "find",
        }
    }

    /// The command whose item has the id `id`, `None` for any other item.
    pub(crate) fn of_id(id: &str) -> Option<Self> {
        [Self::NewConversation, Self::NewMatter, Self::Find]
            .into_iter()
            .find(|command| command.id() == id)
    }
}

/// The page's event, `MENU_EVENT` in `room-commands.ts`.
pub(crate) const EVENT: &str = "clauseo-menu";

/// The script that sends `command` to the page.
pub(crate) fn script(command: PageCommand) -> String {
    format!(
        "window.dispatchEvent(new CustomEvent({EVENT:?}, {{ detail: {:?} }}));",
        command.id()
    )
}

/// The id of the app menu's Settings… item.
#[cfg(target_os = "macos")]
const SETTINGS_ID: &str = "settings";

/// `builder` with the menu bar and its items' handler.
#[cfg(target_os = "macos")]
pub(crate) fn on_builder<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder
        .menu(|app| bar(app, clauseo_protocol::Language::En))
        .on_menu_event(|app, event| on_menu_event(app, event.id().as_ref()))
}

/// The menu bar in `language` (the contract's items).
#[cfg(target_os = "macos")]
pub(crate) fn bar<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    language: clauseo_protocol::Language,
) -> tauri::Result<tauri::menu::Menu<R>> {
    use tauri::menu::{
        AboutMetadata, HELP_SUBMENU_ID, Menu, MenuItem, PredefinedMenuItem as P, Submenu,
        WINDOW_SUBMENU_ID,
    };
    use words::Key;
    let w = |key| Some(words::text(language, key));
    let item = |id: &str, key, accelerator| {
        MenuItem::with_id(app, id, words::text(language, key), true, Some(accelerator))
    };
    let package = app.package_info();
    let config = app.config();
    let about = AboutMetadata {
        name: Some(package.name.clone()),
        version: Some(package.version.to_string()),
        copyright: config.bundle.copyright.clone(),
        authors: config.bundle.publisher.clone().map(|p| vec![p]),
        ..AboutMetadata::default()
    };
    let app_menu = Submenu::with_items(
        app,
        package.name.clone(),
        true,
        &[
            &P::about(app, w(Key::About), Some(about))?,
            &P::separator(app)?,
            &item(SETTINGS_ID, Key::Settings, "CmdOrCtrl+,")?,
            &P::separator(app)?,
            &P::services(app, w(Key::Services))?,
            &P::separator(app)?,
            &P::hide(app, w(Key::Hide))?,
            &P::hide_others(app, w(Key::HideOthers))?,
            &P::show_all(app, w(Key::ShowAll))?,
            &P::separator(app)?,
            &P::quit(app, w(Key::Quit))?,
        ],
    )?;
    let file = Submenu::with_items(
        app,
        words::text(language, Key::File),
        true,
        &[
            &item(
                PageCommand::NewConversation.id(),
                Key::NewConversation,
                "CmdOrCtrl+N",
            )?,
            &item(
                PageCommand::NewMatter.id(),
                Key::NewMatter,
                "CmdOrCtrl+Shift+N",
            )?,
            &P::separator(app)?,
            &P::close_window(app, w(Key::CloseWindow))?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        words::text(language, Key::Edit),
        true,
        &[
            &P::undo(app, w(Key::Undo))?,
            &P::redo(app, w(Key::Redo))?,
            &P::separator(app)?,
            &P::cut(app, w(Key::Cut))?,
            &P::copy(app, w(Key::Copy))?,
            &P::paste(app, w(Key::Paste))?,
            &P::select_all(app, w(Key::SelectAll))?,
            &P::separator(app)?,
            &item(PageCommand::Find.id(), Key::Find, "CmdOrCtrl+K")?,
        ],
    )?;
    let view = Submenu::with_items(
        app,
        words::text(language, Key::View),
        true,
        &[&P::fullscreen(app, w(Key::FullScreen))?],
    )?;
    let window = Submenu::with_id_and_items(
        app,
        WINDOW_SUBMENU_ID,
        words::text(language, Key::Window),
        true,
        &[
            &P::minimize(app, w(Key::Minimize))?,
            &P::maximize(app, w(Key::Zoom))?,
            &P::separator(app)?,
            &P::close_window(app, w(Key::CloseWindow))?,
        ],
    )?;
    let help = Submenu::with_id_and_items(
        app,
        HELP_SUBMENU_ID,
        words::text(language, Key::Help),
        true,
        &[],
    )?;
    Menu::with_items(app, &[&app_menu, &file, &edit, &view, &window, &help])
}

/// What the menu item `id` does: Settings… opens or focuses the settings window; a page command
/// goes to the main window's page ([`send`]). Best effort: a window that cannot open or take
/// the script leaves the app as it was.
#[cfg(target_os = "macos")]
pub(crate) fn on_menu_event<R: tauri::Runtime>(app: &tauri::AppHandle<R>, id: &str) {
    if id == SETTINGS_ID {
        let _opened = crate::settings_window::open_or_focus(app, None);
    } else if let Some(command) = PageCommand::of_id(id) {
        let _sent = send(app, command);
    }
}

/// Raises the main window and sends its page `command` ([`script`]); nothing without one.
pub(crate) fn send<R: tauri::Runtime, M: tauri::Manager<R>>(
    manager: &M,
    command: PageCommand,
) -> tauri::Result<()> {
    let Some(main) = manager.get_webview_window(crate::MAIN_WINDOW) else {
        return Ok(());
    };
    main.unminimize()?;
    main.show()?;
    main.set_focus()?;
    main.eval(script(command))
}
