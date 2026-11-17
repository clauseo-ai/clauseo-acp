//! `read_version {version, path, offset?}`: one version's file as text, a page at a time.
//!
//! The checks, in this order, each refused with its own sentence:
//! 1. the version is on this matter's timeline ([`Refusal::UnknownVersion`]: an id of another
//!    matter is unknown here, as a made-up one is, so nothing tells them apart);
//! 2. the path is in that version's tree as `Matter::files_at` lists it, excluded paths left out
//!    (`clauseo_matters::version_file`, one tree lookup; [`Refusal::NotInVersion`]); only then is
//!    anything read, since `Matter::read_file` checks no exclusion: an old `.claude` file, or a
//!    path out of the folder, never is;
//! 3. unless deleted files are readable ([`super::DELETED_FILES_READABLE`]), the matter held the
//!    file from that version on (`continuity.rs`) and the folder holds it now under the name it
//!    ends with (`clauseo_matters::holds_now`), checked in the job that reads it; else
//!    [`Refusal::Deleted`];
//! 4. its format: a `.pdf` is [`Refusal::Unsupported`]; a `.docx` (any case) must be at most
//!    [`MAX_DOCX_BYTES`] by its size in history and needs stetdoc and a stetdoc run left this
//!    turn, both settled (the run reserved) before anything is copied, unless its text is kept
//!    from an earlier page; any other file must be at most
//!    [`MAX_TEXT_BYTES`] by its size in history, then streams through a [`Window`] and is text
//!    only when all of it is UTF-8 with no NUL.
//!
//! The format is decided after step 2, but its refusal is given only once step 3 passes, so the
//! order holds. Off the matter's thread: the stetdoc run (one per session at a time), the page,
//! and the version's time from the matter's log.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_matters::{Matter, VersionFile, holds_now, version_file};
use clauseo_protocol::{CheckpointId, MatterPath};
use serde_json::{Map, Value};

use super::caps::Reserved;
use super::continuity::{Step, Walk};
use super::docx::{self, Copied};
use super::mcp::Outcome;
use super::texts::Refusal;
use super::window::{Page, PastEnd, Window, page_of};
use super::{Backend, LINE_SEPARATORS, MAX_DOCX_BYTES, MAX_TEXT_BYTES, Opened, lock, utc_time};
use crate::log_file::Cause;

/// The tool's name.
pub(super) const NAME: &str = "read_version";

/// How a path is read, by its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Text,
    Docx,
    Pdf,
}

/// What the read will do once the deleted-file limit passes, decided off the matter's thread.
enum Plan<'a> {
    /// Stream the text file.
    Text,
    /// Page the Word text kept from before.
    Kept(Arc<str>),
    /// Copy the Word file out for stetdoc, with the run reserved for it.
    Copy(Run<'a>),
    /// Refuse, once the limit has passed.
    Refuse(Refusal),
}

/// What the matter's thread does once the limit passes: [`Plan`] without the reservation.
#[derive(Debug, Clone)]
enum Finish {
    Text { offset: u64 },
    Kept,
    Copy { dir: PathBuf },
    Refuse(Refusal),
}

/// What the matter's thread made of the read.
enum Done {
    Text(Page),
    Kept,
    Copied(Copied),
}

/// A job's outcome: more of the walk, or the read done.
enum Next {
    More(Walk),
    Done(Done),
}

/// A stetdoc run reserved from the turn's: given back when dropped unspent.
struct Run<'a> {
    backend: &'a Backend,
    reserved: Option<Reserved>,
}

impl Run<'_> {
    /// The run goes ahead: it stays counted.
    fn spend(&mut self) {
        self.reserved = None;
    }
}

impl Drop for Run<'_> {
    fn drop(&mut self) {
        if let Some(reserved) = self.reserved.take() {
            lock(&self.backend.caps).give_back(&reserved);
        }
    }
}

/// Runs `read_version` with `args`.
pub(super) fn run(backend: &Backend, args: &Map<String, Value>) -> Outcome {
    let version = match args.get("version") {
        Some(Value::String(text)) => text.parse::<CheckpointId>().ok(),
        Some(_) | None => return Err(Refusal::BadVersion),
    };
    let path = match args.get("path") {
        Some(Value::String(text)) if text.contains(LINE_SEPARATORS) => {
            return Err(Refusal::BadPath);
        }
        Some(Value::String(text)) => MatterPath::new(text).ok(),
        Some(_) | None => return Err(Refusal::BadPath),
    };
    let offset = match args.get("offset") {
        None | Some(Value::Null) => 0,
        Some(value) => value.as_u64().ok_or(Refusal::BadOffset)?,
    };
    let version = version.ok_or(Refusal::UnknownVersion)?;
    let deleted_readable = backend.deleted_readable;
    let (file, mut walk) = backend.on_matter(move |opened| {
        let matter = opened.matter;
        if !matter.timeline().iter().any(|entry| entry.id == version) {
            return Err(Refusal::UnknownVersion);
        }
        let path = path.ok_or(Refusal::NotInVersion)?;
        let file = version_file(matter, version, &path)
            .map_err(|error| not_read(&error))?
            .ok_or(Refusal::NotInVersion)?;
        let walk = if deleted_readable {
            None
        } else {
            Some(Walk::from(matter, version, path).ok_or(Refusal::UnknownVersion)?)
        };
        Ok((file, walk))
    })?;
    let mut plan = plan(backend, &file);
    let finish = match &plan {
        Plan::Text => Finish::Text { offset },
        Plan::Kept(_) => Finish::Kept,
        Plan::Copy(_) => Finish::Copy {
            dir: docx::copy_dir(&backend.config.data_dir, &*backend.config.clock),
        },
        Plan::Refuse(refusal) => Finish::Refuse(*refusal),
    };
    let done = loop {
        let (finish, path) = (finish.clone(), file.path.clone());
        let next = backend.on_matter(move |opened| {
            if let Some(walk) = walk {
                match walk.step(opened.matter)? {
                    Step::More(walk) => return Ok(Next::More(walk)),
                    Step::Held(now) => {
                        if !here(opened.matter, &now)? {
                            return Err(Refusal::Deleted);
                        }
                    }
                }
            }
            finish_read(&opened, version, &path, finish).map(Next::Done)
        })?;
        match next {
            Next::More(more) => walk = Some(more),
            Next::Done(done) => break done,
        }
    };
    let (format, page, total) = match (done, &mut plan) {
        (Done::Text(page), _) => ("text", page, file.size),
        (Done::Kept, Plan::Kept(text)) => docx_page(text, offset)?,
        (Done::Copied(copied), Plan::Copy(run)) => {
            let text = show(backend, run, &file, &copied)?;
            drop(copied);
            docx_page(&text, offset)?
        }
        (Done::Kept | Done::Copied(_), _) => return Err(Refusal::WordFailed),
    };
    drop(plan);
    let config = &backend.config;
    let logged = config.source.logged(config.session.matter, &[version]);
    let at = logged
        .at
        .get(&version)
        .map_or_else(|| "unknown".to_owned(), |at| utc_time(*at));
    let next = page
        .next_offset
        .map_or_else(|| "end".to_owned(), |next| next.to_string());
    // The path last: it may hold `; ` but never a line break (`MatterPath`).
    Ok(format!(
        "version: {version}; at: {at}; format: {format}; offset: {}; next_offset: {next}; \
         total_bytes: {total}; path: {}\n\n{}",
        page.offset,
        file.path.as_str(),
        page.text
    ))
}

/// What the read of `file` will do, by its name and size, and for a Word file whether its text
/// is kept, stetdoc is there and a run is left (reserved now).
fn plan<'a>(backend: &'a Backend, file: &VersionFile) -> Plan<'a> {
    match format(&file.path) {
        Format::Pdf => Plan::Refuse(Refusal::Unsupported),
        Format::Text if file.size > MAX_TEXT_BYTES => Plan::Refuse(Refusal::TooLarge),
        Format::Text => Plan::Text,
        Format::Docx if file.size > MAX_DOCX_BYTES => Plan::Refuse(Refusal::WordTooLarge),
        Format::Docx => {
            if let Some(text) = backend.docx.get(&file.content) {
                Plan::Kept(text)
            } else if backend.config.stetdoc.is_none() {
                Plan::Refuse(Refusal::NoWordTool)
            } else {
                match lock(&backend.caps).reserve_docx_run() {
                    Some(reserved) => {
                        #[cfg(test)]
                        backend
                            .copies_planned
                            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Plan::Copy(Run {
                            backend,
                            reserved: Some(reserved),
                        })
                    }
                    None => Plan::Refuse(Refusal::TooManyDocx),
                }
            }
        }
    }
}

/// On the matter's thread, once the deleted-file limit passed: the read `finish` names.
fn finish_read(
    opened: &Opened<'_>,
    version: CheckpointId,
    path: &MatterPath,
    finish: Finish,
) -> Result<Done, Refusal> {
    match finish {
        Finish::Refuse(refusal) => Err(refusal),
        Finish::Kept => Ok(Done::Kept),
        Finish::Copy { dir } => Ok(Done::Copied(docx::copy_out(
            opened.matter,
            version,
            path,
            dir,
            opened.cap.min(MAX_DOCX_BYTES),
        )?)),
        Finish::Text { offset } => {
            let mut window = Window::new(offset);
            opened
                .matter
                .read_file(version, path, &mut window)
                .map_err(|error| not_read(&error))?;
            match window.page() {
                Ok(Some(page)) => Ok(Done::Text(page)),
                Ok(None) => Err(Refusal::Unsupported),
                Err(PastEnd) => Err(Refusal::PastEnd),
            }
        }
    }
}

/// stetdoc's text of the copy, one run per session at a time. A Word file whose text another
/// read kept while this one waited is not shown again: its run is given back.
fn show(
    backend: &Backend,
    run: &mut Run<'_>,
    file: &VersionFile,
    copied: &Copied,
) -> Result<Arc<str>, Refusal> {
    let _one_at_a_time = lock(&backend.stetdoc_run);
    if let Some(text) = backend.docx.get(&file.content) {
        return Ok(text);
    }
    let program = backend
        .config
        .stetdoc
        .as_deref()
        .ok_or(Refusal::NoWordTool)?;
    run.spend();
    let text: Arc<str> = docx::view(program, &backend.config.clock, copied)?.into();
    backend.docx.put(file.content.clone(), &text);
    Ok(text)
}

/// A page of a Word file's text: `docx_text`, the page and the text's length.
fn docx_page(text: &str, offset: u64) -> Result<(&'static str, Page, u64), Refusal> {
    let page = page_of(text, offset).ok_or(Refusal::PastEnd)?;
    Ok(("docx_text", page, text.len() as u64))
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
