//! Whether a listed `.docx` holds tracked changes or comments (`PaperSummary::tracked`), so its
//! tile offers Tracked changes only when there are some. Read from the file itself, off the
//! actor and the matter's worker (`actor/papers/scan.rs` runs [`check`] on a thread of its own).
//!
//! A mark is any tracked change stetdoc 0.9.3 lists, accepts and rejects ([`CHANGE_MARKS`]:
//! text inserted, deleted or moved, and formatting, section, table, row, cell and numbering
//! changes), or a comment (`<w:comment `). The package's relationships decide nothing about
//! what is read: every XML part of the zip is ([`parts`]), so a change in any part Word reads is
//! found whatever names it. Each part is read as XML, as it streams (quick-xml's `NsReader`): a
//! mark is an element in Word's main namespace, Transitional or Strict, whatever prefix the part
//! binds to it, with one of the marks' names.
//!
//! The scan answers "no changes" (`Some(false)`) only for a file it read whole as it models it:
//! the zip opened, its content types and every relationships part read as XML, no part they name
//! missing, and every XML part read to its end, within [`MAX_XML_BYTES`] a part and
//! [`MAX_SCAN_BYTES`] in all, with no mark. Anything else may have changes and answers
//! `Some(true)`, as a change does: a zip that does not open, a part that is not well-formed,
//! uses a prefix it never declares, binds a namespace with a reference in its value, holds a
//! doctype or is not UTF-8, a part cut by a cap or left unread, a missing part. Offering Tracked
//! changes on such a file costs an empty list at worst; stetdoc decides then. `None` is a read
//! of the file that failed and is tried again. The file is opened without following a link at
//! its last component.
//!
//! [`Kept`] holds each matter's answers by path, with the size and modified time the listing
//! gave, so a listing answers at once and only files that changed are read again. A read that
//! failed (a locked file, a sharing violation) is never kept: the next listing tries again.

use std::collections::HashMap;
use std::io::{BufRead as _, BufReader, Read, Seek};
use std::path::Path;

use clauseo_matters::Paper;
use clauseo_protocol::{MatterPath, Timestamp};
use quick_xml::events::Event;
use quick_xml::name::{Namespace, ResolveResult};

use super::is_docx;

/// The most bytes of one XML part read, decompressed: a document over this is some thousands
/// of pages. A part past it may have changes: the tile offers Tracked changes.
pub(crate) const MAX_XML_BYTES: u64 = 16 * 1024 * 1024;

/// The most bytes one file's scan reads, decompressed, over all its parts and relationships. A
/// file with parts left unread past it may have changes, as a part past [`MAX_XML_BYTES`] may.
pub(crate) const MAX_SCAN_BYTES: u64 = 64 * 1024 * 1024;

/// Word's main XML namespaces, Transitional and Strict: a mark is an element in either.
const W_NAMESPACES: &[&str] = &[
    "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    "http://purl.oclc.org/ooxml/wordprocessingml/main",
];

/// The local names of the tracked changes stetdoc 0.9.3 lists, accepts and
/// rejects in every part it reads (each checked with the release binary): text and paragraph
/// marks, rows and custom XML inserted, deleted or moved, cells inserted, deleted or merged,
/// and run, paragraph, section, table, table exception, grid, row, cell and numbering changes.
/// A move's range markers (`w:moveFromRangeStart`) alone are not a change it lists.
const CHANGE_MARKS: &[&[u8]] = &[
    b"ins",
    b"del",
    b"moveFrom",
    b"moveTo",
    b"rPrChange",
    b"pPrChange",
    b"sectPrChange",
    b"tblPrChange",
    b"tblPrExChange",
    b"tblGridChange",
    b"trPrChange",
    b"tcPrChange",
    b"numberingChange",
    b"cellIns",
    b"cellDel",
    b"cellMerge",
    b"customXmlInsRangeStart",
    b"customXmlDelRangeStart",
];

/// The local name of a comment.
const COMMENT_MARKS: &[&[u8]] = &[b"comment"];

/// One matter's answers: by path, the size and modified time listed when it was read, and
/// whether it holds marks.
#[derive(Debug, Default)]
pub(crate) struct Kept(HashMap<MatterPath, (u64, Timestamp, bool)>);

impl Kept {
    /// Whether `paper` holds marks, if it was read at its listed size and time.
    pub(crate) fn get(&self, paper: &Paper) -> Option<bool> {
        match self.0.get(&paper.path) {
            Some(&(size, modified, has)) if size == paper.size && modified == paper.modified => {
                Some(has)
            }
            _ => None,
        }
    }

    /// Keeps what a read of `paper` found.
    pub(crate) fn put(&mut self, paper: &Paper, has: bool) {
        self.0
            .insert(paper.path.clone(), (paper.size, paper.modified, has));
    }

    /// Drops the answers for paths `papers` no longer lists.
    pub(crate) fn keep_only(&mut self, papers: &[Paper]) {
        let listed: std::collections::HashSet<&MatterPath> =
            papers.iter().map(|paper| &paper.path).collect();
        self.0.retain(|path, _| listed.contains(path));
    }

    /// `papers`' `.docx` files with no answer kept for their listed size and time.
    pub(crate) fn unread<'a>(&self, papers: &'a [Paper]) -> impl Iterator<Item = &'a Paper> {
        papers
            .iter()
            .filter(|paper| is_docx(&paper.path) && self.get(paper).is_none())
    }
}

/// Whether the Word file at `path` holds a tracked change or a comment; `None` when it could not
/// be read (so the answer is not kept). Never follows a link at `path`, and on Windows never
/// reads a cloud placeholder or an offline file, which would download it ([`reparse`]).
pub(crate) fn check(path: &Path) -> Option<bool> {
    #[cfg(test)]
    if let Some(hook) = hooks::find(path) {
        return hook(path);
    }
    let file = crate::write::open_read_nofollow(path).ok()?;
    #[cfg(windows)]
    let file = windows_file(path, file)?;
    if !file.metadata().ok()?.is_file() {
        return Some(false);
    }
    zip_has_marks(BufReader::new(file))
}

/// `file`, opened at `path` as a reparse point itself, as the file to read: the same handle, or
/// for a reparse point that is not a link (deduplicated, a cloud file kept on this computer…)
/// the file opened again normally. `None` for a link or a mount point (never followed), a file
/// in the cloud or offline (never read), and a reopened file that is not the same file.
#[cfg(windows)]
fn windows_file(path: &Path, file: std::fs::File) -> Option<std::fs::File> {
    use crate::network_paths::{attribute_tag, file_identity};
    let (attributes, tag) = attribute_tag(&file).ok()?;
    match reparse::reparse(attributes, tag) {
        reparse::Reparse::Read => Some(file),
        reparse::Reparse::Skip | reparse::Reparse::Unknown => None,
        reparse::Reparse::Reopen => {
            // `file` stays open while the path is opened again, so the same volume and file ID
            // on both proves the second open followed nothing swapped in between. A FAT or exFAT
            // volume, which has no file IDs, holds no reparse point, so never gets here; an
            // identity that can't be read, or no file ID, proves nothing and skips the file
            let again = std::fs::File::open(path).ok()?;
            let first = file_identity(&file).ok()?;
            if file_identity(&again).ok()? != first {
                return None;
            }
            drop(file);
            // turned into a placeholder or an offline file since: never read
            let (attributes, tag) = attribute_tag(&again).ok()?;
            match reparse::reparse(attributes, tag) {
                reparse::Reparse::Read | reparse::Reparse::Reopen => Some(again),
                reparse::Reparse::Skip | reparse::Reparse::Unknown => None,
            }
        }
    }
}

/// Whether the zip `file` may hold a tracked change or a comment: `Some(false)` only when it was
/// read whole as the scan models it and holds none; `None` when a read failed and is worth
/// trying again. `pub(super)` for `fuzz/fuzz_targets/zip_marks.rs`, where this file is a module.
pub(super) fn zip_has_marks(file: impl Read + Seek) -> Option<bool> {
    zip_marks(file, MAX_XML_BYTES, MAX_SCAN_BYTES)
}

/// [`zip_has_marks`] with these caps: at most `part_cap` bytes of a part and `total_cap` in all.
fn zip_marks(file: impl Read + Seek, part_cap: u64, total_cap: u64) -> Option<bool> {
    let mut zip = match zip::ZipArchive::new(file) {
        Ok(zip) => zip,
        Err(zip::result::ZipError::Io(error)) => return failed(&error),
        Err(_) => return Some(true),
    };
    let Some(entries) = parts::entries(&zip) else {
        return Some(true);
    };
    let names = &entries.names;
    let mut left = total_cap;
    // the content types: the parts that are XML under another name, and the parts they name
    let Some(types_name) = names.iter().find(|name| parts::is_content_types(name)) else {
        return Some(true);
    };
    let types = match read_whole(&mut zip, types_name, part_cap, &mut left)? {
        Whole::Read(bytes) => parts::content_types(&bytes),
        Whole::Not => None,
    };
    let Some(types) = types else {
        return Some(true);
    };
    if !types.all_present(&entries) {
        return Some(true);
    }
    // the relationships: read only so that a part they name and the zip lacks is noticed
    let rels: Vec<&String> = names.iter().filter(|name| types.is_rels(name)).collect();
    if rels.len() > parts::MAX_PARTS {
        return Some(true);
    }
    for name in rels {
        let Whole::Read(bytes) = read_whole(&mut zip, name, part_cap, &mut left)? else {
            return Some(true);
        };
        let targets = parts::targets(name, &bytes);
        if !targets.is_some_and(|targets| parts::present(&targets, &entries)) {
            return Some(true);
        }
        // read for marks too: a part named `.rels` may be typed and used as a story part; a real
        // relationships part holds none
        if holds_any(bytes.as_slice(), &[CHANGE_MARKS, COMMENT_MARKS])? != Found::Clean {
            return Some(true);
        }
    }
    let xml: Vec<&String> = names
        .iter()
        .filter(|name| !parts::is_content_types(name) && !types.is_rels(name))
        .filter(|name| types.is_xml(name))
        .collect();
    if xml.len() > parts::MAX_PARTS {
        return Some(true);
    }
    for name in xml {
        if left == 0 {
            // past the total cap with parts left unread: they may have changes
            return Some(true);
        }
        let part = match zip.by_name(name) {
            Ok(part) => part,
            Err(zip::result::ZipError::Io(error)) => return failed(&error),
            Err(_) => return Some(true),
        };
        let mut part = part.take(part_cap.min(left));
        let limit = part.limit();
        let found = holds_any(&mut part, &[CHANGE_MARKS, COMMENT_MARKS])?;
        left -= limit - part.limit();
        // cut by a cap: the scan read to the cap and the part goes on past it
        let cut = part.limit() == 0
            && match part.get_mut().read(&mut [0_u8]) {
                Ok(more) => more == 1,
                Err(error) => return failed(&error),
            };
        if cut || found != Found::Clean {
            return Some(true);
        }
    }
    Some(false)
}

/// What a read error answers: a damaged file (its data cut short or not what it should be) may
/// have changes; any other failure (a locked file, a sharing violation) is tried again.
fn failed(error: &std::io::Error) -> Option<bool> {
    damaged(error).then_some(true)
}

/// Whether a read failed on the file's data rather than on reaching it.
fn damaged(error: &std::io::Error) -> bool {
    use std::io::ErrorKind;
    matches!(
        error.kind(),
        ErrorKind::UnexpectedEof | ErrorKind::InvalidData | ErrorKind::InvalidInput
    )
}

/// A part read for what it says, not for marks.
enum Whole {
    /// All its bytes.
    Read(Vec<u8>),
    /// Missing, damaged or past a cap: not read whole.
    Not,
}

/// The part `name` whole (at most `part_cap` bytes and `left`, which the read takes from);
/// `None` when a read failed and is worth trying again.
fn read_whole<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
    part_cap: u64,
    left: &mut u64,
) -> Option<Whole> {
    let part = match zip.by_name(name) {
        Ok(part) => part,
        Err(zip::result::ZipError::Io(error)) => return failed(&error).map(|_| Whole::Not),
        Err(_) => return Some(Whole::Not),
    };
    let limit = part_cap.min(*left);
    let mut bytes = Vec::new();
    // one byte past the limit, to tell a part over it: its stated size may be wrong
    if let Err(error) = part.take(limit + 1).read_to_end(&mut bytes) {
        return failed(&error).map(|_| Whole::Not);
    }
    let read = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    *left -= read.min(*left);
    if read > limit {
        return Some(Whole::Not);
    }
    Some(Whole::Read(bytes))
}

/// Whether a part's first bytes read as UTF-8 to the scan: no UTF-16 byte order mark and no NUL
/// in its first four bytes (UTF-16 or UTF-32 without a mark).
fn utf8(head: &[u8]) -> bool {
    !head.starts_with(&[0xFE, 0xFF])
        && !head.starts_with(&[0xFF, 0xFE])
        && !head.iter().take(4).any(|&byte| byte == 0)
}

/// Whether an XML declaration leaves the part UTF-8: it names no encoding, or UTF-8.
fn declared_utf8(declaration: &quick_xml::events::BytesDecl<'_>) -> bool {
    declaration.encoding().is_none_or(|encoding| {
        encoding.is_ok_and(|encoding| {
            encoding.eq_ignore_ascii_case("utf-8") || encoding.eq_ignore_ascii_case("utf8")
        })
    })
}

/// What the scan of a part found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Found {
    /// An element in Word's main namespace with one of the marks' names.
    Mark,
    /// None, in a part read to its end as the scan models it.
    Clean,
    /// Something the scan does not model, or a damaged part: it may have changes.
    Unmodelled,
}

/// What `bytes`, one XML part, hold: an element in one of [`W_NAMESPACES`] with any of `marks`'
/// local names, read as it streams; `None` when a read failed and is worth trying again.
fn holds_any(bytes: impl Read, marks: &[&[&[u8]]]) -> Option<Found> {
    let marks: Vec<&[u8]> = marks
        .iter()
        .flat_map(|names| names.iter().copied())
        .collect();
    let mut bytes = BufReader::with_capacity(64 * 1024, bytes);
    let head = match bytes.fill_buf() {
        Ok(head) => head,
        Err(error) => return damaged(&error).then_some(Found::Unmodelled),
    };
    if !utf8(head) {
        return Some(Found::Unmodelled);
    }
    let mut reader = quick_xml::NsReader::from_reader(bytes);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        let (resolved, event) = match reader.read_resolved_event_into(&mut buffer) {
            Ok(read) => read,
            Err(quick_xml::Error::Io(error)) => {
                return damaged(&error).then_some(Found::Unmodelled);
            }
            Err(_) => return Some(Found::Unmodelled),
        };
        match event {
            Event::Start(element) | Event::Empty(element) => {
                if !plain_bindings(&element) {
                    return Some(Found::Unmodelled);
                }
                match resolved {
                    ResolveResult::Bound(Namespace(namespace))
                        if W_NAMESPACES.contains(&namespace)
                            && marks.contains(&element.local_name().as_ref().as_bytes()) =>
                    {
                        return Some(Found::Mark);
                    }
                    ResolveResult::Unknown(_) => return Some(Found::Unmodelled),
                    ResolveResult::Bound(_) | ResolveResult::Unbound => {}
                }
            }
            Event::Decl(declaration) => {
                if !declared_utf8(&declaration) {
                    return Some(Found::Unmodelled);
                }
            }
            Event::DocType(_) => return Some(Found::Unmodelled),
            Event::Eof => return Some(Found::Clean),
            Event::End(_)
            | Event::Text(_)
            | Event::CData(_)
            | Event::Comment(_)
            | Event::PI(_)
            | Event::GeneralRef(_) => {}
        }
    }
}

/// Whether `element`'s attributes read, and its namespace declarations hold no reference (`&`):
/// quick-xml binds a prefix to the raw value, where Word reads the value decoded.
fn plain_bindings(element: &quick_xml::events::BytesStart<'_>) -> bool {
    element.attributes().all(|attribute| {
        attribute.is_ok_and(|attribute| {
            let key = attribute.key.as_ref();
            !(key == "xmlns" || key.starts_with("xmlns:")) || !attribute.value.contains('&')
        })
    })
}

/// Tests' stand-ins for [`check`], by folder: a test registers one for its own folder, so tests
/// running at once never see each other's.
#[cfg(test)]
pub(crate) mod hooks {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex, PoisonError};

    /// A stand-in for [`super::check`].
    pub(crate) type Hook = Arc<dyn Fn(&Path) -> Option<bool> + Send + Sync>;

    static HOOKS: Mutex<Vec<(PathBuf, Hook)>> = Mutex::new(Vec::new());

    /// Answers [`super::check`] with `hook` for every file under `folder`.
    pub(crate) fn set(folder: &Path, hook: Hook) {
        let mut hooks = HOOKS.lock().unwrap_or_else(PoisonError::into_inner);
        hooks.push((folder.to_path_buf(), hook));
    }

    /// The stand-in for `path`, if a test set one.
    pub(super) fn find(path: &Path) -> Option<Hook> {
        let hooks = HOOKS.lock().unwrap_or_else(PoisonError::into_inner);
        hooks
            .iter()
            .find(|(folder, _)| path.starts_with(folder))
            .map(|(_, hook)| Arc::clone(hook))
    }
}

// Each module names its file: fuzz/fuzz_targets/zip_marks.rs compiles this file as a module of
// its own (#[path]), where a bare `mod parts;` would be looked for beside it, not in marks/.
#[cfg(test)]
#[path = "marks/package_tests.rs"]
mod package_tests;
#[path = "marks/parts.rs"]
mod parts;
#[cfg(any(windows, test))]
#[path = "marks/reparse.rs"]
mod reparse;
#[cfg(test)]
#[path = "marks/tests.rs"]
mod tests;
#[cfg(test)]
#[path = "marks/xml_tests.rs"]
mod xml_tests;
