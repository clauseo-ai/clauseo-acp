//! What the three commands that leave the app do (`docs/decisions/0026-open-and-reveal.md`):
//! `open_external` opens an https, http or mailto link in the default browser or mail app after
//! a native confirm naming where it goes; `open_file` opens a document of a matter with its
//! default app; `reveal` shows a matter's folder, or the app data folder, in Finder or File
//! Explorer. The webview names a file by the `FileRefId` the host issued and a folder by its
//! matter, never by a path. The host, the confirm and the OS sit behind traits, so tests run
//! without a window and never open anything.

mod native;
#[cfg(test)]
mod tests;
mod words;

use std::path::{Path, PathBuf};

use clauseo_protocol::{ErrorCode, FileRefId, HostError, Language, MatterId};
use serde::Deserialize;

use crate::dialogs::BoxFuture;

pub(crate) use native::{NativeLinkConfirm, NativeOpener};
pub(crate) use words::LinkWords;

/// The longest link `open_external` takes, in bytes.
pub(crate) const MAX_LINK_BYTES: usize = 2048;

/// The longest mail address list a mailto link may show in the confirm, in bytes.
const MAX_MAIL_SHOWN: usize = 254;

/// The file types `open_file` opens, lower case, without the dot: documents, sheets, slides
/// and images. Nothing that runs code when opened: no programs, scripts, shortcuts, web pages,
/// or Office files with macros (`docm`, `xlsm`, `pptm`, `dotm`).
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

/// Where a link goes, for the confirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Destination {
    /// An https or http link to this host (with its port when the link names one).
    Web(String),
    /// A mailto link to these addresses, as the link wrote them.
    Mail(String),
}

/// A link `open_external` may open: the URL to hand the OS, and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Link {
    /// The URL as the `url` crate writes it back.
    pub(crate) url: String,
    /// Where it goes, as the confirm names it.
    pub(crate) destination: Destination,
}

/// A file the host issued a ref for, with the folder of the matter that owns the ref, as the
/// host found them when asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocatedFile {
    /// The matter that owns the ref.
    pub(crate) matter: MatterId,
    /// That matter's folder.
    pub(crate) folder: PathBuf,
    /// The file.
    pub(crate) path: PathBuf,
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
    /// Opens the file `path` with its default app.
    fn open_path(&self, path: &Path) -> Result<(), HostError>;
    /// Shows `path` selected in its parent folder, in Finder or File Explorer.
    fn reveal(&self, path: &Path) -> Result<(), HostError>;
}

/// A `command.invalid` failure with `message`.
fn invalid(message: impl Into<String>) -> HostError {
    HostError::new(ErrorCode::CommandInvalid, message)
}

/// Checks `text` is a link `open_external` may open: at most [`MAX_LINK_BYTES`], https or http
/// with a host and no user name or password, or mailto with one or more plain addresses.
/// Anything else is `command.invalid`.
pub(crate) fn check_link(text: &str) -> Result<Link, HostError> {
    if text.len() > MAX_LINK_BYTES {
        return Err(invalid(format!(
            "a link of {} bytes is over the {MAX_LINK_BYTES} byte limit",
            text.len()
        )));
    }
    let url = url::Url::parse(text).map_err(|error| invalid(format!("not a link: {error}")))?;
    let destination = match url.scheme() {
        "https" | "http" => Destination::Web(web_host(&url)?),
        "mailto" => Destination::Mail(mail_addresses(&url)?),
        other => {
            return Err(invalid(format!(
                "links of the scheme {other} do not open; only https, http and mailto do"
            )));
        }
    };
    Ok(Link {
        url: url.as_str().to_owned(),
        destination,
    })
}

/// The host an https or http link goes to, as ASCII (an international name in its `xn--`
/// form, so the confirm cannot show a look-alike), with the port when the link names one.
fn web_host(url: &url::Url) -> Result<String, HostError> {
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid("a link with a user name or password does not open"));
    }
    let host = url
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or_else(|| invalid("a web link without a host does not open"))?;
    Ok(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    })
}

/// The addresses a mailto link writes to, decoded. Only printable ASCII without spaces, at most
/// [`MAX_MAIL_SHOWN`] bytes, and at least one `@`: the confirm shows them, so nothing that
/// could hide or reorder text (controls, bidirectional marks, look-alike letters) gets there.
fn mail_addresses(url: &url::Url) -> Result<String, HostError> {
    let decoded = percent_decode(url.path())
        .ok_or_else(|| invalid("a mailto link whose address does not decode does not open"))?;
    let plain = decoded.bytes().all(|byte| byte.is_ascii_graphic());
    if decoded.is_empty() || decoded.len() > MAX_MAIL_SHOWN || !plain || !decoded.contains('@') {
        return Err(invalid(
            "a mailto link opens only to plain addresses of at most 254 characters",
        ));
    }
    Ok(decoded)
}

/// `text` with each `%XX` decoded, when the result is UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        if byte == b'%' {
            let hex = bytes.get(at + 1..at + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            out.push(byte);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Checks `path` is a document `open_file` may open: it and `folder` resolve (symlinks and `..`
/// followed), it lies in `folder`, it is a file (not a folder, so not a macOS app bundle), and
/// its type is one of [`DOCUMENT_TYPES`]. Answers the resolved path, which is what opens.
/// `matter.folder_missing` when the folder does not resolve; `command.invalid` otherwise.
pub(crate) fn check_document(path: &Path, folder: &Path) -> Result<PathBuf, HostError> {
    let folder = dunce::canonicalize(folder).map_err(|error| {
        HostError::new(
            ErrorCode::MatterFolderMissing,
            format!("the matter's folder is not there: {error}"),
        )
    })?;
    let path = dunce::canonicalize(path)
        .map_err(|error| invalid(format!("the file is not there: {error}")))?;
    if !path.starts_with(&folder) {
        return Err(invalid("the file is not inside its matter's folder"));
    }
    let is_file = std::fs::metadata(&path)
        .map_err(|error| invalid(format!("the file does not read: {error}")))?
        .is_file();
    if !is_file {
        return Err(invalid("only files open, not folders"));
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
    Ok(path)
}

/// Opens an https `url` in the default browser with no confirm, for Rust's own callers only
/// (stage D's sign-in page, R20): no command, capability or binding reaches it, and a test checks
/// none names it. The link check is `open_external`'s, narrowed to https; anything else is
/// `command.invalid`.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "stage D's R20 calls it; until then only its tests do"
    )
)]
pub(crate) fn open_url_unconfirmed(opener: &dyn Opener, url: &str) -> Result<(), HostError> {
    let link = check_link(url)?;
    if !link.url.starts_with("https://") {
        return Err(invalid("only https links open without a confirm"));
    }
    opener.open_url(&link.url)
}

/// `open_external`: checks `url` (`check_link`), asks the lawyer in `language`, and opens it.
/// Declined, nothing opens and the call fails with `transport.unauthorized`.
pub(crate) async fn open_external(
    confirm: &dyn LinkConfirm,
    opener: &dyn Opener,
    language: Language,
    url: &str,
) -> Result<(), HostError> {
    let link = check_link(url)?;
    if !confirm
        .confirm_link(LinkWords::new(&link.destination, language))
        .await
    {
        return Err(HostError::new(
            ErrorCode::TransportUnauthorized,
            "the lawyer declined to open the link",
        ));
    }
    opener.open_url(&link.url)
}

/// `open_file`: asks the host what `file` names, checks it (`check_document`) and opens it.
pub(crate) async fn open_file(
    locate: &dyn Locate,
    opener: &dyn Opener,
    file: FileRefId,
) -> Result<(), HostError> {
    let located = locate.locate_file(file).await?;
    let path = check_document(&located.path, &located.folder)?;
    opener.open_path(&path)
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
