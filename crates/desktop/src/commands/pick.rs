//! The folder commands: the native folder dialog, and Clauseo's own chooser where Windows'
//! would stall (`chooser.rs`), with the page's timing of it. Moved out of `commands.rs` whole.

use std::path::{Path, PathBuf};

use clauseo_protocol::{ErrorCode, HostError};
use clauseo_settings::{KnownFolder, PlatformDirs as _, SystemDirs};
use tauri::{AppHandle, Runtime, State};

use crate::bridge::Bridge;
use crate::chooser::{Chooser, Go, Listing, PickAnswer, Route, StartPoint};
use crate::dialogs::{LastPick, StartFolders, StartHint};
use crate::open::AppData;
use crate::picked::PickedFolder;

/// Opens the native folder dialog and registers the folder picked: its ref, and its display
/// relative to the home folder (`picked.rs`). `None` when cancelled. The dialog opens where
/// `start` says: `"last"` (absent is the same), the last pick's parent, else on Windows Documents
/// once the share check passed, else home (macOS: the OS's own remembered folder); `"documents"`,
/// `"matters"` (where new matters go) or `{"path": ...}` (an absolute folder) when that folder is
/// usable, except on Windows a folder inside a known folder while the share check has not passed
/// (`dialogs::start_for`). Nothing here stats the folder.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn pick_folder<R: Runtime>(
    app: AppHandle<R>,
    bridge: State<'_, Bridge>,
    last: State<'_, LastPick>,
    chooser: State<'_, Chooser>,
    start: Option<StartHint>,
) -> Result<Option<PickedFolder>, HostError> {
    crate::ipc::timed("pick_folder", async {
        let folder = native_start(&bridge, &last, chooser.inner(), start.as_ref()).await;
        native_pick(app, &bridge, &last, folder).await
    })
    .await
}

/// Opens the native folder dialog for "Add a skill" (Settings › Skills) and answers the folder's
/// path, which the page then sends as `AddSkill { path }`; `None` when cancelled. Folders only
/// (`dialogs::SkillPicker`). Only a path returned here is admitted in an `AddSkill`.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn pick_skill<R: Runtime>(
    app: AppHandle<R>,
    bridge: State<'_, Bridge>,
) -> Result<Option<String>, HostError> {
    crate::ipc::timed("pick_skill", async {
        let picker = crate::dialogs::SkillPicker::new(app);
        Ok(bridge.pick_skill(&picker).await)
    })
    .await
}

/// Where the native dialog opens for `hint` (`None` is `"last"`), as `dialogs::start_for` gives
/// it. Nothing is read from the disk: the host's default matters folder (for `"matters"`) comes
/// from its own state, and the Documents and known folders from the OS's known-folder API and
/// the share check's cached answer, all read off the async thread (`spawn_blocking`), since the
/// known-folder API may wait on a share that is down.
async fn native_start(
    bridge: &Bridge,
    last: &LastPick,
    chooser: &Chooser,
    hint: Option<&StartHint>,
) -> Option<PathBuf> {
    let home = SystemDirs.home();
    let matters = match hint {
        Some(StartHint::Matters) => bridge
            .default_location()
            .await
            .and_then(|display| crate::dialogs::matters_from_display(&display, home.as_deref())),
        _ => None,
    };
    let last = last.get();
    let hint = hint.cloned();
    let chooser = chooser.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let known = chooser.known_paths();
        let documents = documents_folder(home.as_deref());
        let folders = StartFolders {
            last_pick: last.as_deref(),
            home: home.as_deref(),
            documents: documents.as_deref(),
            matters: matters.as_deref(),
            known: &known,
            stalls: chooser.cached_stall(),
        };
        crate::dialogs::start_for(hint.as_ref(), cfg!(windows), &folders)
    })
    .await
    .ok()
    .flatten()
}

/// The user's Documents folder, without the disk: Windows' known folder, else `home/Documents`
/// (macOS gives no known folders).
fn documents_folder(home: Option<&Path>) -> Option<PathBuf> {
    if cfg!(windows) {
        SystemDirs.known_folder(KnownFolder::Documents)
    } else {
        home.map(|home| home.join("Documents"))
    }
}

/// [`pick_folder`]'s native dialog, opening on `start` when there is one.
async fn native_pick<R: Runtime>(
    app: AppHandle<R>,
    bridge: &Bridge,
    last: &LastPick,
    start: Option<PathBuf>,
) -> Result<Option<PickedFolder>, HostError> {
    let home = SystemDirs.home();
    let picker = crate::dialogs::NativePicker::new(app, start, last);
    bridge.pick_folder(&picker, home.as_deref()).await
}

/// [`pick_folder`], unless Windows' dialog would stall on a known folder whose share is down
/// (`chooser/stall.rs`, found out as the app starts, so at once by now, at most 1.5 s after the
/// start; `CLAUSEO_FOLDER_PICKER=app|native` decides instead): then it opens an in-app chooser
/// session (closing any other) and answers `choose` at once, without waiting on any read. At
/// most 20 sessions a run and 3 a minute; past that the native dialog opens. Never `choose` off
/// Windows. `start` (see [`pick_folder`]) applies on the native dialog only: the chooser keeps
/// its own start.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn pick_folder_or_choose<R: Runtime>(
    app: AppHandle<R>,
    bridge: State<'_, Bridge>,
    last: State<'_, LastPick>,
    chooser: State<'_, Chooser>,
    app_data: State<'_, AppData>,
    start: Option<StartHint>,
) -> Result<PickAnswer, HostError> {
    crate::ipc::timed("pick_folder_or_choose", async {
        let clicked = crate::open::now();
        crate::chooser::trace::step("click received", clicked, "");
        // The e2e build's folder comes first, as `NativePicker::pick` gives it: no check, no
        // session.
        #[cfg(feature = "e2e")]
        let fixed = crate::e2e::matter_dir();
        #[cfg(not(feature = "e2e"))]
        let fixed: Option<std::path::PathBuf> = None;
        let setting = std::env::var(crate::chooser::PICKER_VAR).ok();
        let stalls = chooser.inner().clone();
        let has_fixed = fixed.is_some();
        let route = tauri::async_runtime::spawn_blocking(move || {
            // Past the session caps the lawyer sees the native dialog; no check runs then.
            let may_open = cfg!(windows) && stalls.may_open(crate::open::now());
            crate::chooser::route(has_fixed, may_open, setting.as_deref(), || {
                stalls.would_stall()
            })
        })
        .await
        .unwrap_or(Route::Native);
        if let (Route::Fixed, Some(folder)) = (route, fixed) {
            return crate::chooser::pick_fixed(&bridge, folder, SystemDirs.home().as_deref()).await;
        }
        crate::chooser::trace::step("route decided", clicked, &format!("{route:?}"));
        if route == Route::Chooser {
            let data = app_data.0.clone();
            // Home and the refused folders ask Windows for known folders, which may wait on a
            // share: read on the session's own thread, so the answer goes back at once.
            let asked = move || {
                let mut refused = clauseo_settings::refused_folders(&SystemDirs);
                refused.push(data);
                (SystemDirs.home(), refused)
            };
            let now = crate::open::now();
            if let Some(start) = chooser.open_asking(asked, last.get().as_deref(), now) {
                crate::chooser::trace::step("session opened", clicked, "");
                return Ok(PickAnswer::Choose { start });
            }
        }
        let hint = if route == Route::Native { start } else { None };
        let folder = native_start(&bridge, &last, chooser.inner(), hint.as_ref()).await;
        Ok(match native_pick(app, &bridge, &last, folder).await? {
            Some(folder) => PickAnswer::Picked { folder },
            None => PickAnswer::Cancelled,
        })
    })
    .await
}

/// Where the chooser session `chooser` may start: its drives, home, Desktop and last place,
/// once its one read of the drives landed (at most 1.5 s after it opened). `command.invalid`
/// for an unknown or closed session.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn chooser_starts(
    state: State<'_, Chooser>,
    chooser: u32,
) -> Result<Vec<StartPoint>, HostError> {
    crate::ipc::timed("chooser_starts", async {
        let state = state.inner().clone();
        let asked = crate::open::now();
        let starts =
            tauri::async_runtime::spawn_blocking(move || state.starts(chooser, crate::open::now()))
                .await
                .map_err(|error| {
                    HostError::new(ErrorCode::Internal, format!("no starts: {error}"))
                })?;
        crate::chooser::trace::step("starts answered", asked, "");
        starts
    })
    .await
}

/// The page's timing of a chooser it showed, for the log alone (`chooser/trace.rs`): the
/// milliseconds from the click to the desktop's answer, to the chooser's first render and to its
/// first paint, as the page measured them. Always answers ok; logs no path, and only the first
/// [`CHOOSER_SHOWN_CAP`] calls of a run.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn chooser_shown(
    answer_ms: u32,
    render_ms: u32,
    paint_ms: u32,
) -> Result<(), HostError> {
    crate::ipc::timed("chooser_shown", async {
        if CHOOSER_SHOWN_LOGGED.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            < CHOOSER_SHOWN_CAP
        {
            tracing::info!(
                target: crate::chooser::trace::TARGET,
                at_ms = crate::chooser::trace::at_ms(),
                answer_ms,
                render_ms,
                paint_ms,
                "folder chooser: the page showed it"
            );
        }
        Ok(())
    })
    .await
}

/// How many `chooser_shown` lines one run logs, so script in the page can't flood host.log.
const CHOOSER_SHOWN_CAP: u32 = 20;

/// The `chooser_shown` calls so far this run, logged or not.
static CHOOSER_SHOWN_LOGGED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Goes where `go` says in the chooser session `chooser` and lists the folder there: by a
/// start point's key, an index into the last listing, up (to a drive or typed share root), a
/// typed path, or again. At most 2 s: `no_answer` past it. `command.invalid` for an unknown or
/// closed session, an index or key it did not give, and a typed path it refuses;
/// `transport.unauthorized` past 300 listings.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn chooser_list(
    state: State<'_, Chooser>,
    chooser: u32,
    go: Go,
) -> Result<Listing, HostError> {
    crate::ipc::timed("chooser_list", async {
        let state = state.inner().clone();
        tauri::async_runtime::spawn_blocking(move || state.list(chooser, go, crate::open::now()))
            .await
            .map_err(|error| HostError::new(ErrorCode::Internal, format!("no listing: {error}")))?
    })
    .await
}

/// Chooses the folder the chooser session shows, after a native confirm with the full path and
/// Cancel focused; confirmed, it is registered as `pick_folder` registers a pick and the session
/// closes. `None` when the lawyer declines or the folder has gone; the session stays open.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn chooser_choose<R: Runtime>(
    app: AppHandle<R>,
    bridge: State<'_, Bridge>,
    last: State<'_, LastPick>,
    state: State<'_, Chooser>,
    chooser: u32,
) -> Result<Option<PickedFolder>, HostError> {
    crate::ipc::timed("chooser_choose", async {
        let confirm = crate::chooser::NativeFolderConfirm::new(app, &bridge);
        let home = SystemDirs.home();
        crate::chooser::choose_folder(&state, &bridge, &confirm, &last, home.as_deref(), chooser)
            .await
    })
    .await
}

/// Closes the chooser session; `command.invalid` when it is not open.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn chooser_cancel(
    state: State<'_, Chooser>,
    chooser: u32,
) -> Result<(), HostError> {
    crate::ipc::timed("chooser_cancel", async { state.close(chooser) }).await
}
