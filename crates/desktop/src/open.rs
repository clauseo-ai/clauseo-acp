//! What the three commands that leave the app do (`docs/decisions/0026-open-and-reveal.md`):
//! `open_external` opens an https, http or mailto link in the default browser or mail app after
//! a native confirm naming where it goes; `open_file` opens a document of a matter with its
//! default app; `reveal` shows a matter's folder, or the app data folder, in Finder or File
//! Explorer. The webview names a file by the `FileRefId` the host issued and a folder by its
//! matter, never by a path. The host, the confirm and the OS sit behind traits, so tests run
//! without a window and never open anything.

mod gate;
mod link;
#[cfg(target_os = "macos")]
mod mac_app;
mod native;
#[cfg(test)]
mod tests;
mod words;

use std::path::{Path, PathBuf};

pub(crate) use clauseo_host::LocatedFile;
use clauseo_protocol::{ErrorCode, FileRefId, HostError, Language, MatterId};
use serde::Deserialize;

use crate::dialogs::BoxFuture;

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
}

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

/// `open_external` for a checked `link`: asks the lawyer in `language`, through `gate` (one
/// confirm at a time, none for [`AFTER_DECLINE`] after a decline: `command.conflict`), and
/// opens it. Declined, nothing opens and the call fails with `transport.unauthorized`.
pub(crate) async fn open_external(
    confirm: &dyn LinkConfirm,
    opener: &dyn Opener,
    gate: &gate::Gate,
    now: Now<'_>,
    language: Language,
    link: &Link,
) -> Result<(), HostError> {
    let pass = gate.enter(now())?;
    if !confirm
        .confirm_link(LinkWords::new(&link.destination, language))
        .await
    {
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
/// (`check_document`) and opens it with the default app for its type.
pub(crate) async fn open_file(
    locate: &dyn Locate,
    opener: &dyn Opener,
    gate: &gate::Gate,
    now: Now<'_>,
    file: FileRefId,
) -> Result<(), HostError> {
    let pass = gate.enter(now())?;
    let located = locate.locate_file(file).await?;
    let document = check_document(&located.path, &located.folder)?;
    // Only a call that gets this far holds the next one back: a page sending refs that are
    // refused cannot keep the lawyer's own clicks out.
    pass.quiet_for(now(), BETWEEN_OPENS);
    opener.open_path(&document.path, &document.kind)
}

/// `reveal`: shows the matter's folder, or `app_data`, selected in its parent folder.
/// `command.unknown_matter` from the host, and `matter.folder_missing` when the matter's folder
/// is not there.
pub(crate) async fn reveal(
    locate: &dyn Locate,
    opener: &dyn Opener,
    app_data: &Path,
    target: RevealTarget,
) -> Result<(), HostError> {
    let folder = match target {
        RevealTarget::Matter(matter) => locate.matter_folder(matter).await?,
        RevealTarget::AppData => app_data.to_path_buf(),
    };
    if !folder.is_dir() {
        let code = match target {
            RevealTarget::Matter(_) => ErrorCode::MatterFolderMissing,
            RevealTarget::AppData => ErrorCode::Internal,
        };
        return Err(HostError::new(code, "the folder is not there"));
    }
    opener.reveal(&folder)
}
