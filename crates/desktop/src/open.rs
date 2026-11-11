//! What the three commands that leave the app do (`docs/decisions/0026-open-and-reveal.md`):
//! `open_external` opens an https, http or mailto link in the default browser or mail app, a
//! plain web link at once and a mailto or misleading one after a native confirm naming where it
//! goes (`ask.rs`); `open_file` opens a document of a matter with its
//! default app; `reveal` shows a matter's folder, or the app data folder, in Finder or File
//! Explorer. The webview names a file by the `FileRefId` the host issued and a folder by its
//! matter, never by a path. The host, the confirm and the OS sit behind traits, so tests run
//! without a window and never open anything.

mod ask;
mod bounded;
pub(crate) mod gate;
mod link;
#[cfg(target_os = "macos")]
mod mac_app;
mod native;
#[cfg(test)]
mod tests;
mod word_path;
mod words;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub(crate) use clauseo_host::LocatedFile;
use clauseo_protocol::{ErrorCode, FileRefId, HostError, Language, MatterId};
use serde::{Deserialize, Serialize};

use crate::dialogs::BoxFuture;

pub(crate) use ask::needs_confirm;
pub(crate) use bounded::FOLDER_WAIT;

pub(crate) use gate::{AFTER_DECLINE, BETWEEN_OPENS, Now, OpenGates, now};
#[cfg(test)]
pub(crate) use link::MAX_LINK_BYTES;
pub(crate) use link::{Destination, Link, check_link};
pub(crate) use native::{NativeLinkConfirm, NativeOpener};
pub(crate) use words::LinkWords;

/// The file types `open_file` opens, lower case, without the dot: documents, sheets, slides
/// and images. Nothing the OS runs itself: no programs, scripts, shortcuts or web pages, and no
/// Office files with macros by name (`docm`, `xlsm`, `pptm`, `dotm`). A document's own app
/// runs what its own policy allows (an old `doc` or `xls` can carry macros, an `rtf`
/// embedded objects).
pub(crate) const DOCUMENT_TYPES: &[&str] = &[
    "doc", "docx", "dotx", "rtf", "odt", "pdf", "txt", "md", "csv", "xls", "xlsx", "ods", "ppt",
    "pptx", "odp", "png", "jpg", "jpeg", "gif", "heic", "tif", "tiff",
];

/// Clauseo's app data folder, as `Locations::resolve` found it at start-up: what
/// `reveal(app_data)` shows.
pub(crate) struct AppData(pub(crate) PathBuf);

/// What `reveal` shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub(crate) enum RevealTarget {
    /// The matter's folder.
    Matter(MatterId),
    /// Clauseo's app data folder: the event store, the version store and the settings.
    AppData,
}

/// The host's side: what a file ref names, and where a matter's folder is.
pub(crate) trait Locate: Send + Sync {
    /// The file `file` names and its matter's folder. The host refuses a ref it did not issue
    /// and a file outside its matter's folder.
    fn locate_file(&self, file: FileRefId) -> BoxFuture<'_, Result<LocatedFile, HostError>>;
    /// The folder of `matter`, or `command.unknown_matter`.
    fn matter_folder(&self, matter: MatterId) -> BoxFuture<'_, Result<PathBuf, HostError>>;
    /// The marker check: whether a folder is still a matter's folder, by its
    /// `.clauseo/matter.json` (`clauseo_host::check_matter_folder`). It reads the folder, so it
    /// runs off the async runtime, bounded ([`bounded::bounded`]).
    fn marker_check(&self) -> MarkerCheck;
}

/// Whether `folder` is still `matter`'s folder: `matter.folder_missing` when it is not there,
/// `matter.folder_not_a_matter` when it is there without the matter's marker.
pub(crate) type MarkerCheck = Arc<dyn Fn(&Path, MatterId) -> Result<(), HostError> + Send + Sync>;

/// Asks the lawyer whether to open a link, in a native dialog drawn by Rust: `true` to go
/// ahead.
pub(crate) trait LinkConfirm: Send + Sync {
    /// Asks once, in `words`.
    fn confirm_link(&self, words: LinkWords) -> BoxFuture<'_, bool>;
}

/// The OS's side: the default browser, mail app and file apps, and the file manager.
pub(crate) trait Opener: Send + Sync {
    /// Opens `url` in the default browser or mail app.
    fn open_url(&self, url: &str) -> Result<(), HostError>;
    /// Opens the file `path` with the default app for files of `kind` (its extension, checked,
    /// lower case): on macOS that app by name, so what opens is a document in it whatever the
    /// path has become since the check.
    fn open_path(&self, path: &Path, kind: &str) -> Result<(), HostError>;
    /// Shows `path` selected in its parent folder, in Finder or File Explorer.
    fn reveal(&self, path: &Path) -> Result<(), HostError>;
    /// The 8.3 short form of the folder `folder`, for a Word document whose path is too long
    /// for Word (`word_path`): `GetShortPathNameW` on Windows. `None`, as for a volume without
    /// short names, unless an opener says otherwise.
    fn short_folder(&self, _folder: &Path) -> Option<PathBuf> {
        None
    }
}

/// What `open_file` tells the page about a document it opened.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct OpenedFile {
    /// Word opens it in Protected View: its path was too long for Word, so it opened through its
    /// folders' short names (`word_path`), and the lawyer chooses Enable Editing to change it.
    pub(crate) protected_view: bool,
}

/// A `command.invalid` failure with `message`.
fn invalid(message: impl Into<String>) -> HostError {
    HostError::new(ErrorCode::CommandInvalid, message)
}

/// A document `check_document` let through: its resolved path and its type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Document {
    /// The path with symlinks and `..` followed: what opens.
    pub(crate) path: PathBuf,
    /// Its extension, lower case: one of [`DOCUMENT_TYPES`].
    pub(crate) kind: String,
}

/// Checks `path` is a document `open_file` may open: it and `folder` resolve (symlinks and `..`
/// followed), it lies in `folder`, it is a file (not a folder, so not a macOS app bundle), it
/// is not marked as a program (no execute bit, on Linux; macOS opens with the checked type's app,
/// and its SMB and exFAT volumes mark every file executable), and its type is one of
/// [`DOCUMENT_TYPES`]. `matter.folder_missing` when the folder does not resolve;
/// `command.invalid` otherwise.
///
/// Both sides go through `std::fs::canonicalize`, which on Windows is always verbatim (`\\?\`).
/// `dunce::canonicalize` drops the prefix below `MAX_PATH` and keeps it above, so a long document
/// in a short folder would come back verbatim and the folder plain, and `starts_with` would
/// refuse it. Both verbatim compare. The path handed on is `dunce::simplified`: plain when it
/// fits, verbatim only when it must be.
pub(crate) fn check_document(path: &Path, folder: &Path) -> Result<Document, HostError> {
    #[expect(
        clippy::disallowed_methods,
        reason = "both sides verbatim, so a long document in a short folder compares; the path handed on is dunce::simplified"
    )]
    let folder = std::fs::canonicalize(folder).map_err(|error| {
        HostError::new(
            ErrorCode::MatterFolderMissing,
            format!("the matter's folder is not there: {error}"),
        )
    })?;
    #[expect(
        clippy::disallowed_methods,
        reason = "both sides verbatim, so a long document in a short folder compares; the path handed on is dunce::simplified"
    )]
    let path = std::fs::canonicalize(path)
        .map_err(|error| invalid(format!("the file is not there: {error}")))?;
    if !path.starts_with(&folder) {
        return Err(invalid("the file is not inside its matter's folder"));
    }
    let metadata = std::fs::metadata(&path)
        .map_err(|error| invalid(format!("the file does not read: {error}")))?;
    if !metadata.is_file() {
        return Err(invalid("only files open, not folders"));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o111 != 0 {
            return Err(invalid(
                "a file marked as a program does not open from Clauseo",
            ));
        }
    }
    let kind = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !DOCUMENT_TYPES.contains(&kind.as_str()) {
        return Err(invalid(format!(
            "files of the type \"{kind}\" do not open from Clauseo"
        )));
    }
    Ok(Document {
        path: dunce::simplified(&path).to_path_buf(),
        kind,
    })
}

/// Opens an https `url` in the default browser with no confirm, for Rust's own callers only
/// (stage D's sign-in page, R20): no command, capability or binding reaches it, and a test checks
/// none names it. The link check is `open_external`'s, narrowed to https; anything else is
/// `command.invalid`.
pub(crate) fn open_url_unconfirmed(opener: &dyn Opener, url: &str) -> Result<(), HostError> {
    let link = check_link(url)?;
    if !link.url.starts_with("https://") {
        return Err(invalid("only https links open without a confirm"));
    }
    opener.open_url(&link.url)
}

/// macOS's Files and Folders pane in System Settings' Privacy & Security.
pub(crate) const PRIVACY_FILES_AND_FOLDERS: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders";

/// `open_privacy_settings`: opens [`PRIVACY_FILES_AND_FOLDERS`] when `mac`, and nothing else, so
/// the page can open no other URL with it. Off macOS it opens nothing and is `command.invalid`;
/// the page shows its button on macOS only.
pub(crate) fn open_privacy_settings(opener: &dyn Opener, mac: bool) -> Result<(), HostError> {
    if !mac {
        return Err(invalid("the Privacy settings pane is macOS's"));
    }
    opener.open_url(PRIVACY_FILES_AND_FOLDERS)
}

/// `open_external` for a checked `link` whose visible text is `shown`: when [`needs_confirm`]
/// says so (a mailto link, a user name before the host, an international host, or text naming
/// another site) it asks first, as [`open_confirmed`] does; otherwise it opens at once. Both go
/// through `gate`: one call at a time (`command.conflict`), none for [`AFTER_DECLINE`] after a
/// decline, and none for [`BETWEEN_OPENS`] after a link opened with no confirm, so script in the
/// page cannot open a stream of tabs the lawyer was never asked about. An accepted confirm sets no
/// pause: the lawyer chose it.
pub(crate) async fn open_external(
    confirm: &dyn LinkConfirm,
    opener: &dyn Opener,
    gate: &gate::Gate,
    now: Now<'_>,
    language: Language,
    link: &Link,
    shown: Option<&str>,
) -> Result<(), HostError> {
    if needs_confirm(link, shown) {
        return open_confirmed(confirm, opener, gate, now, language, link).await;
    }
    let pass = gate.enter(now())?;
    pass.quiet_for(now(), BETWEEN_OPENS);
    opener.open_url(&link.url)
}

/// Opens a checked `link` after asking the lawyer in `language`, whatever the link: through
/// `gate` (one confirm at a time, none for [`AFTER_DECLINE`] after a decline:
/// `command.conflict`). Declined, nothing opens and the call fails with `transport.unauthorized`.
/// `open_external` uses it for the links that need a confirm; a connector's pay link always does.
pub(crate) async fn open_confirmed(
    confirm: &dyn LinkConfirm,
    opener: &dyn Opener,
    gate: &gate::Gate,
    now: Now<'_>,
    language: Language,
    link: &Link,
) -> Result<(), HostError> {
    let pass = gate.enter(now())?;
    if !confirm.confirm_link(LinkWords::new(link, language)).await {
        pass.quiet_for(now(), AFTER_DECLINE);
        return Err(HostError::new(
            ErrorCode::TransportUnauthorized,
            "the lawyer declined to open the link",
        ));
    }
    opener.open_url(&link.url)
}

/// `open_file`: through `gate` (one at a time, and at most one document opened per
/// [`BETWEEN_OPENS`]: `command.conflict`), asks the host what `file` names, checks it
/// (`check_document`) and opens it with the default app for its type. A Word document whose
/// path is too long for Word opens through its folders' short names, in Protected View, or is
/// refused as `matter.document_path_too_long` (`word_path`). A matter folder that is not there is
/// `matter.folder_missing`; one that is there without the matter's `.clauseo/matter.json` (none,
/// unreadable, or another matter's) is `matter.folder_not_a_matter` (`Locate::marker_check`).
///
/// The checks (the marker, `check_document`'s `canonicalize` and `metadata`, the short-name
/// lookup) and then the OS's open each run on a thread of their own, waited on for at most
/// `wait` (the command passes [`FOLDER_WAIT`], #617's 10 s): past it, `matter.folder_missing`
/// (retryable), with no path, and the caller's thread is never held. Checks that answer late
/// never open: the open waits apart, after them.
pub(crate) async fn open_file<O: Opener + 'static>(
    locate: &dyn Locate,
    opener: &Arc<O>,
    gate: &gate::Gate,
    now: Now<'_>,
    wait: Duration,
    file: FileRefId,
) -> Result<OpenedFile, HostError> {
    let pass = gate.enter(now())?;
    let located = locate.locate_file(file).await?;
    let (check, short) = (locate.marker_check(), Arc::clone(opener));
    let (document, path, protected_view) = bounded::bounded(
        "open_file check",
        wait,
        ErrorCode::MatterFolderMissing,
        move || {
            check(&located.folder, located.matter)?;
            let document = check_document(&located.path, &located.folder)?;
            let (path, protected_view) =
                word_path::word_open_path(&document.path, |folder| short.short_folder(folder))
                    .map_err(|word_path::TooLong| too_long_for_word())?;
            Ok((document, path, protected_view))
        },
    )
    .await?;
    // Only a call that gets this far holds the next one back: a page sending refs that are
    // refused cannot keep the lawyer's own clicks out.
    pass.quiet_for(now(), BETWEEN_OPENS);
    let opener = Arc::clone(opener);
    bounded::bounded(
        "open_file open",
        wait,
        ErrorCode::MatterFolderMissing,
        move || opener.open_path(&path, &document.kind),
    )
    .await?;
    Ok(OpenedFile { protected_view })
}

/// The refusal of a Word document whose path is too long for Word even through its folders'
/// short names.
fn too_long_for_word() -> HostError {
    HostError::new(
        ErrorCode::MatterDocumentPathTooLong,
        format!(
            "the document's path is over {} UTF-16 units, too long for Word, even with its folders' short names",
            word_path::WORD_PATH_LIMIT
        ),
    )
}

/// `reveal`: shows the matter's folder, or `app_data`, selected in its parent folder.
/// `command.unknown_matter` from the host; `matter.folder_missing` when the matter's folder is
/// not there, and `matter.folder_not_a_matter` when it is there without the matter's
/// `.clauseo/matter.json` (`Locate::marker_check`); `internal` when the app data folder is not
/// there. The marker check, the `is_dir` and the OS's reveal run on a thread of their own,
/// waited on for at most `wait` ([`FOLDER_WAIT`]): past it, `matter.folder_missing`
/// (retryable) for a matter's folder and `internal` for the app data folder, with no path.
pub(crate) async fn reveal<O: Opener + 'static>(
    locate: &dyn Locate,
    opener: &Arc<O>,
    app_data: &Path,
    wait: Duration,
    target: RevealTarget,
) -> Result<(), HostError> {
    let (folder, check, late) = match target {
        RevealTarget::Matter(matter) => (
            locate.matter_folder(matter).await?,
            Some((locate.marker_check(), matter)),
            ErrorCode::MatterFolderMissing,
        ),
        RevealTarget::AppData => (app_data.to_path_buf(), None, ErrorCode::Internal),
    };
    // The checks and the reveal wait apart, so checks that answer late never reveal.
    let folder = bounded::bounded("reveal check", wait, late, move || {
        if let Some((check, matter)) = check {
            check(&folder, matter)?;
        }
        if !folder.is_dir() {
            return Err(HostError::new(late, "the folder is not there"));
        }
        Ok(folder)
    })
    .await?;
    let opener = Arc::clone(opener);
    bounded::bounded("reveal", wait, late, move || opener.reveal(&folder)).await
}
