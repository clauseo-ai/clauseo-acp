//! The `settings` window: `ui/app`'s `settings.html`, opened beside `main`
//! (`docs/decisions/0028-settings-window.md`).
//!
//! Contract:
//! - Made on open, destroyed on close; never hidden. [`open_or_focus`] brings an open one
//!   forward (unminimised, shown, focused), else builds it: 960 by 700 fitted to the monitor
//!   and centred (`window_size.rs`; window-state never restores it), the https scheme like
//!   `main` (one origin, so one local storage), the room's frame (`window_frame.rs`: on the
//!   Mac the overlay title bar with the lights where the page draws its corner lights; on
//!   Windows no decorations, the page's three caption buttons and Snap Layouts on maximise),
//!   the page's CSP on its scripts (`csp.rs`), and in the e2e build the same webview arguments
//!   and init script as `main`, since `WebView2` webviews sharing a profile must match.
//! - Callers: the `open_settings` command (granted to `main` only) and, on the Mac, the app
//!   menu's "Settings…" item on Cmd+, ([`app_menu`]). No menu on Windows or Linux.
//! - Each window's destruction drops its bridge stream ([`on_destroyed`]); destroying `main`
//!   destroys `settings` too, so closing `main` still ends the app.
//! - Nothing here is main-like: main-only behaviour stays keyed on `MAIN_WINDOW`.

use tauri::{Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

use crate::bridge::Bridge;
use crate::{MAIN_WINDOW, csp, window_frame, window_size};

/// The settings window's label. `capabilities/settings.json` grants by it
/// (`"windows": ["settings"]`), and `capabilities/frame.json` names it beside `main`.
pub(crate) const SETTINGS_WINDOW: &str = "settings";

/// The page it shows, an entry of `ui/app`'s build.
const PAGE: &str = "settings.html";

/// The window's title (the Mac hides it; Windows shows it in the taskbar).
const TITLE: &str = "Settings";

/// The id of the Mac app menu's "Settings…" item.
#[cfg(target_os = "macos")]
const MENU_ID: &str = "settings";

/// Brings the settings window forward, building it when there is none.
pub(crate) fn open_or_focus<R: Runtime, M: Manager<R>>(manager: &M) -> tauri::Result<()> {
    if let Some(window) = manager.get_webview_window(SETTINGS_WINDOW) {
        return raise(&window);
    }
    build(manager).or_else(|error| match manager.get_webview_window(SETTINGS_WINDOW) {
        // Two opens at once: the other built it.
        Some(window) => raise(&window),
        None => Err(error),
    })
}

/// Unminimises, shows and focuses `window`.
fn raise<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    window
        .unminimize()
        .and_then(|()| window.show())
        .and_then(|()| window.set_focus())
}

/// Builds the settings window (see the contract).
fn build<R: Runtime, M: Manager<R>>(manager: &M) -> tauri::Result<()> {
    // The e2e and marks builds open at the size whatever the work area, like `main`; so do the
    // unit tests, whose mock runtime has no monitor to ask.
    let (width, height) = if cfg!(any(feature = "e2e", test)) {
        window_size::SETTINGS_SIZE
    } else {
        window_size::fit(
            window_size::SETTINGS_SIZE,
            window_size::primary_work_area(manager.app_handle()),
        )
    };
    let builder = WebviewWindowBuilder::new(manager, SETTINGS_WINDOW, WebviewUrl::App(PAGE.into()))
        .title(TITLE)
        .use_https_scheme(true)
        .inner_size(width, height)
        .center();
    let builder = window_frame::with_frame(builder);
    let builder = csp::on_scripts(builder, manager);
    #[cfg(all(windows, feature = "e2e"))]
    let builder = crate::e2e::with_debugging_port(builder);
    #[cfg(feature = "e2e")]
    let builder = builder.initialization_script(crate::e2e::init_script());
    let built = number_build(manager, SETTINGS_WINDOW);
    let window = builder.build()?;
    window_size::paint_first_frame(&window);
    // The Windows maximise cover makes a Win32 child window and subclasses this one, which must
    // happen on the window's own thread; `open_settings` builds from an async command's thread.
    let framed = window.clone();
    window.run_on_main_thread(move || window_frame::after_build(&framed))?;
    on_destroyed(&window, built);
    Ok(())
}

/// Numbers a build of the window `label` in the bridge, before it is built (`None` when no
/// bridge is managed, in tests).
pub(crate) fn number_build<R: Runtime, M: Manager<R>>(manager: &M, label: &str) -> Option<u64> {
    manager
        .try_state::<Bridge>()
        .map(|bridge| bridge.window_built(label))
}

/// When `window` is destroyed, runs [`closed`] for it.
pub(crate) fn on_destroyed<R: Runtime>(window: &WebviewWindow<R>, built: Option<u64>) {
    let app = window.app_handle().clone();
    let label = window.label().to_owned();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            closed(&app, &label, built);
        }
    });
}

/// What follows the window `label`, built as number `built`, being destroyed: the bridge drops
/// its stream (`Bridge::close_window`), and when it is `main` the settings window is destroyed
/// too, so the app ends as it did with one window.
pub(crate) fn closed<R: Runtime, M: Manager<R>>(manager: &M, label: &str, built: Option<u64>) {
    if let (Some(built), Some(bridge)) = (built, manager.try_state::<Bridge>()) {
        bridge.close_window(label, built);
    }
    if label == MAIN_WINDOW
        && let Some(settings) = manager.get_webview_window(SETTINGS_WINDOW)
    {
        // Best effort: a window that cannot be destroyed stays, and closing it ends the app.
        let _destroyed = settings.destroy();
    }
}

/// The Mac app menu: Tauri's default, with "Settings…" on Cmd+, after About and its
/// separator, then a separator before Services.
#[cfg(target_os = "macos")]
pub(crate) fn app_menu<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> tauri::Result<tauri::menu::Menu<R>> {
    use tauri::menu::{Menu, MenuItem, MenuItemKind, PredefinedMenuItem};
    let menu = Menu::default(app)?;
    if let Some(MenuItemKind::Submenu(first)) = menu.items()?.into_iter().next() {
        let settings = MenuItem::with_id(app, MENU_ID, "Settings…", true, Some("CmdOrCtrl+,"))?;
        let separator = PredefinedMenuItem::separator(app)?;
        first.insert_items(&[&settings, &separator], 2)?;
    }
    Ok(menu)
}

/// Opens or focuses the settings window when the menu item `id` is "Settings…".
#[cfg(target_os = "macos")]
pub(crate) fn on_menu_event<R: Runtime>(app: &tauri::AppHandle<R>, id: &str) {
    if id == MENU_ID {
        // Best effort: a window that cannot open leaves the app as it was.
        let _opened = open_or_focus(app);
    }
}

#[cfg(test)]
mod tests;
