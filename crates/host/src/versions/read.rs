//! `read_version {version, path, offset?}`: one version's file as text, a page at a time.
//!
//! On the matter's thread, in this order, each refused with its own sentence:
//! 1. the version is on this matter's timeline ([`Refusal::UnknownVersion`]: an id of another
//!    matter is unknown here, as a made-up one is, so nothing tells them apart);
//! 2. the path is in that version's file list, `Matter::files_at`, which leaves out excluded
//!    paths ([`Refusal::NotInVersion`]); only then is anything read, since `Matter::read_file`
//!    checks no exclusion: an old `.claude` file, or a path out of the folder, never is;
//! 3. unless deleted files are readable ([`super::DELETED_FILES_READABLE`]), the file is in the
//!    folder now (`clauseo_matters::holds_now`, every call), or `changes_between(version, last
//!    checkpoint)` records it `Renamed` (exact content) to a file in the folder now; else
//!    [`Refusal::Deleted`]. A rename with an edit is a delete and an add, so it stays refused;
//! 4. its format: a `.docx` (any case) is copied out for stetdoc, unless its text is kept from
//!    the page before; a `.pdf` is [`Refusal::Unsupported`]; any other file streams through
//!    a [`Window`], and is text only when all of it is UTF-8 with no NUL.
//!
//! Off the matter's thread: the stetdoc run (counted against the turn's runs), the page, and
//! the version's time from the matter's log.

use std::sync::Arc;

use clauseo_matters::{Matter, holds_now};
use clauseo_protocol::{ChangeKind, CheckpointId, ContentId, MatterPath};
use serde_json::{Map, Value, json};

use super::docx::Copied;
use super::mcp::Outcome;
use super::texts::Refusal;
use super::window::{Page, PastEnd, Window, page_of};
use super::{Backend, docx, lock, utc_time};
use crate::log_file::Cause;

/// The tool's name.
pub(super) const NAME: &str = "read_version";

/// What the matter's thread made of a read.
enum Read {
    /// A text file's page, and the file's size.
    Text { page: Page, total: u64 },
    /// A Word file whose text is kept from before.
    DocxKept(ContentId),
    /// A Word file copied out, with its content.
    DocxCopied(Copied, ContentId),
}

/// How a path is read, by its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Text,
    Docx,
    Pdf,
}

/// Runs `read_version` with `args`.
pub(super) fn run(backend: &Backend, args: &Map<String, Value>) -> Outcome {
    let version = match args.get("version") {
        Some(Value::String(text)) => text.parse::<CheckpointId>().ok(),
        Some(_) | None => return Err(Refusal::BadVersion),
    };
    let path = match args.get("path") {
        Some(Value::String(text)) => MatterPath::new(text).ok(),
        Some(_) | None => return Err(Refusal::BadPath),
    };
    let offset = match args.get("offset") {
        None | Some(Value::Null) => 0,
        Some(value) => value.as_u64().ok_or(Refusal::BadOffset)?,
    };
    let version = version.ok_or(Refusal::UnknownVersion)?;
    let config = &backend.config;
    let deleted_readable = backend.deleted_readable;
    let word = Docx {
        kept: backend.docx.content(),
        run_left: lock(&backend.caps).docx_run_left(),
        dir: docx::copy_dir(&config.data_dir, &*config.clock),
    };
    let read_path = path.clone();
    let read = backend.on_matter(move |matter| {
        check_and_read(matter, version, read_path, offset, deleted_readable, word)
    })?;
    let (format, page, total) = match read {
        Read::Text { page, total } => ("text", page, total),
        Read::DocxKept(content) => {
            let text = backend.docx.get(&content).ok_or(Refusal::WordFailed)?;
            docx_page(&text, offset)?
        }
        Read::DocxCopied(copied, content) => {
            if !lock(&backend.caps).take_docx_run() {
                return Err(Refusal::TooManyDocx);
            }
            let program = config.stetdoc.as_deref().ok_or(Refusal::WordFailed)?;
            let text: Arc<str> = docx::view(program, &config.clock, &copied)?.into();
            drop(copied);
            backend.docx.put(content, &text);
            docx_page(&text, offset)?
        }
    };
    let logged = config.source.logged(config.session.matter, &[version]);
    let mut answer = json!({
        "version": version.to_string(),
        "path": path.as_ref().map(MatterPath::as_str),
        "at": logged.at.get(&version).map(|at| utc_time(*at)),
        "format": format,
        "text": page.text,
        "offset": page.offset,
        "total_bytes": total,
    });
    if let (Some(next), Some(object)) = (page.next_offset, answer.as_object_mut()) {
        object.insert("next_offset".to_owned(), json!(next));
    }
    Ok(answer)
}

/// A page of a Word file's text: `docx_text`, the page and the text's length.
fn docx_page(text: &str, offset: u64) -> Result<(&'static str, Page, u64), Refusal> {
    let page = page_of(text, offset).ok_or(Refusal::PastEnd)?;
    Ok(("docx_text", page, text.len() as u64))
}

/// What the matter's thread needs to know for a Word file.
struct Docx {
    /// The Word file whose text is kept.
    kept: Option<ContentId>,
    /// Whether the turn may still run stetdoc: a copy that could not be shown is not made.
    run_left: bool,
    /// Where a Word file is copied when its text is not kept.
    dir: std::path::PathBuf,
}

/// The checks and the read, on the matter's thread (see the module's list).
fn check_and_read(
    matter: &Matter,
    version: CheckpointId,
    path: Option<MatterPath>,
    offset: u64,
    deleted_readable: bool,
    word: Docx,
) -> Result<Read, Refusal> {
    if !matter.timeline().iter().any(|entry| entry.id == version) {
        return Err(Refusal::UnknownVersion);
    }
    let path = path.ok_or(Refusal::NotInVersion)?;
    let file = matter
        .files_at(version)
        .map_err(|error| not_read(&error))?
        .into_iter()
        .find(|file| file.path == path)
        .ok_or(Refusal::NotInVersion)?;
    if !deleted_readable {
        still_here(matter, version, &path)?;
    }
    match format(&path) {
        Format::Pdf => Err(Refusal::Unsupported),
        Format::Docx if word.kept.as_ref() == Some(&file.content) => {
            Ok(Read::DocxKept(file.content))
        }
        Format::Docx if !word.run_left => Err(Refusal::TooManyDocx),
        Format::Docx => Ok(Read::DocxCopied(
            docx::copy_out(matter, version, &path, word.dir)?,
            file.content,
        )),
        Format::Text => {
            let mut window = Window::new(offset);
            matter
                .read_file(version, &path, &mut window)
                .map_err(|error| not_read(&error))?;
            match window.page() {
                Ok(Some(page)) => Ok(Read::Text {
                    page,
                    total: file.size,
                }),
                Ok(None) => Err(Refusal::Unsupported),
                Err(PastEnd) => Err(Refusal::PastEnd),
            }
        }
    }
}

/// The deleted-file limit: `path` is in the folder now, or the version's file was renamed,
/// exact content and nothing else, to a file that is.
fn still_here(matter: &Matter, version: CheckpointId, path: &MatterPath) -> Result<(), Refusal> {
    if here(matter, path)? {
        return Ok(());
    }
    let Some(last) = matter.last_checkpoint() else {
        return Err(Refusal::Deleted);
    };
    let changes = matter
        .changes_between(version, last)
        .map_err(|error| not_read(&error))?;
    let renamed_to = changes.iter().find_map(|change| {
        if let ChangeKind::Renamed { from } = change.change()
            && from == path
        {
            Some(change.path())
        } else {
            None
        }
    });
    match renamed_to {
        Some(now) if here(matter, now)? => Ok(()),
        Some(_) | None => Err(Refusal::Deleted),
    }
}

fn here(matter: &Matter, path: &MatterPath) -> Result<bool, Refusal> {
    holds_now(matter, path).map_err(|error| {
        tracing::info!(error = %Cause(&error), "a file's presence was not checked");
        Refusal::NotChecked
    })
}

fn not_read(error: &clauseo_matters::Error) -> Refusal {
    tracing::warn!(error = %Cause(error), "a version's file did not read");
    Refusal::NotRead
}

/// How `path` is read, by the end of its name in any case.
fn format(path: &MatterPath) -> Format {
    let name = path.as_str().rsplit('/').next().unwrap_or_default();
    let ends = |suffix: &str| {
        name.len() > suffix.len()
            && name
                .get(name.len() - suffix.len()..)
                .is_some_and(|end| end.eq_ignore_ascii_case(suffix))
    };
    if ends(".docx") {
        Format::Docx
    } else if ends(".pdf") {
        Format::Pdf
    } else {
        Format::Text
    }
}
