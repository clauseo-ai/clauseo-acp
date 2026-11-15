//! `page_marks`: the page's start-up marks, for host.log alone (`startup.rs`). `set_layout_menu`:
//! the room's panels, for the words of the Mac menu's toggles (`menu.rs`).

use clauseo_protocol::HostError;
use tauri::{AppHandle, Runtime};

use crate::startup::PageMarks;

/// The page's start-up marks (`ui/app/src/start-marks.ts`), sent once after the welcome's
/// primary button mounts, logged on the desktop's clock (`startup::log_page`): the first call of
/// a run only. Always answers ok; carries only milliseconds.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn page_marks(marks: PageMarks) -> Result<(), HostError> {
    crate::ipc::timed("page_marks", async {
        crate::startup::log_page(marks);
        Ok(())
    })
    .await
}

/// Which of the room's panels are shown, sent by the page on start and on each change, so the
/// Mac menu's View toggles say what a press does ("Hide the sidebar" or "Show the sidebar", and
/// the same for the papers; `menu.rs`). Granted to `main` only. Always answers ok: a menu that
/// can't take the words keeps the ones it shows, and off macOS, which has no menu, it does
/// nothing.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn set_layout_menu<R: Runtime>(
    app: AppHandle<R>,
    sidebar: bool,
    papers: bool,
) -> Result<(), HostError> {
    crate::ipc::timed("set_layout_menu", async {
        #[cfg(target_os = "macos")]
        {
            let layout = crate::menu::LayoutShown { sidebar, papers };
            let _retitled = crate::menu::set_layout(&app, layout);
        }
        #[cfg(not(target_os = "macos"))]
        let _unused = (app, sidebar, papers);
        Ok(())
    })
    .await
}
