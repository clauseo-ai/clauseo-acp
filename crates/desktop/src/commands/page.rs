//! `page_marks`: the page's start-up marks, for host.log alone (`startup.rs`).

use clauseo_protocol::HostError;

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
