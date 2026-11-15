//! Whether a listed `.docx` holds tracked changes or comments (`PaperSummary::tracked`), so its
//! tile offers Tracked changes only when there are some. Read from the file itself, off the
//! actor and the matter's worker (`actor/papers/scan.rs` runs [`check`] on a thread of its own).
//!
//! A mark is any tracked change stetdoc 0.9.3 lists, accepts and rejects ([`CHANGE_MARKS`]:
//! text inserted, deleted or moved, and formatting, section, table, row, cell and numbering
//! changes), or a comment (`<w:comment `). It is looked for in every part stetdoc reads them
//! from: the main document and the headers, footers, footnotes, endnotes and comments its
//! relationships name ([`parts`]), or by Word's usual part names when those relationships are
//! too large or not UTF-8. Each part is read as XML, as it streams (quick-xml's `NsReader`): a
//! mark is an element in Word's main namespace, whatever prefix the part binds to it, with one
//! of the marks' names, so comments, CDATA, processing instructions and nested declarations are
//! read as XML reads them. The scan never answers "no changes" for a part it does not model: a
//! part that is not well-formed, that uses a prefix it never declares, that holds a doctype (whose
//! entities could hold markup), or that is not UTF-8 (a UTF-16 byte order mark, a NUL in its first
//! bytes, or another declared encoding) answers "may have changes", which offers Tracked changes
//! as a change does. The read stops at the first mark, after [`MAX_XML_BYTES`] of a part and after
//! [`MAX_SCAN_BYTES`] in all, so a zip bomb or a huge
//! document costs a bounded read; a part or a file the scan could not read whole answers "may
//! have changes" too. A file that does not read as a zip, or has no such part, has
//! none. The file is opened without following a link at its last component.
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

/// The most bytes of a relationships part: about 300 bytes a relationship, so a brief with
/// thousands of links and images fits. Over it, Word's usual part names are scanned instead.
const MAX_RELS_BYTES: u64 = 1024 * 1024;

/// Word's main XML namespace: a mark is an element in it.
const W_NAMESPACE: &[u8] = b"http://schemas.openxmlformats.org/wordprocessingml/2006/main";

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

/// Whether the zip `file` reads as a Word file that holds a tracked change or a comment in any
/// part stetdoc reads them from (the main document first); `None` when a read failed. A part
/// the scan does not model, or could not read whole, answers `Some(true)`: it may have changes,
/// so the tile offers Tracked changes, as for a change, and stetdoc decides when they are listed.
fn zip_has_marks(file: impl Read + Seek) -> Option<bool> {
    zip_marks(file, MAX_XML_BYTES, MAX_SCAN_BYTES)
}

/// [`zip_has_marks`] with these caps: at most `part_cap` bytes of a part and `total_cap` in all.
fn zip_marks(file: impl Read + Seek, part_cap: u64, total_cap: u64) -> Option<bool> {
    let mut zip = match zip::ZipArchive::new(file) {
        Ok(zip) => zip,
        Err(zip::result::ZipError::Io(_)) => return None,
        Err(_) => return Some(false),
    };
    let mut left = total_cap;
    let main = match read_rels(&mut zip, "_rels/.rels", &mut left)? {
        Rels::Read(rels) => parts::main_part(&rels),
        Rels::Unread => parts::main_part(b""),
    };
    let story = match read_rels(&mut zip, &parts::rels_of(&main), &mut left)? {
        Rels::Read(rels) => parts::story_parts(&main, &rels),
        Rels::Unread => {
            let names: Vec<String> = zip
                .file_names()
                .filter_map(Result::ok)
                .map(std::borrow::Cow::into_owned)
                .collect();
            parts::usual_story_parts(names.iter().map(String::as_str))
        }
    };
    // more story parts than the scan reads: it cannot answer that they hold none
    if story.len() > parts::MAX_PARTS {
        return Some(true);
    }
    for name in std::iter::once(&main).chain(&story) {
        if left == 0 {
            // past the total cap with parts left unread: they may have changes
            return Some(true);
        }
        let part = match zip.by_name(name) {
            Ok(part) => part,
            Err(zip::result::ZipError::Io(_)) => return None,
            Err(_) => continue,
        };
        let mut part = part.take(part_cap.min(left));
        let limit = part.limit();
        let found = holds_any(&mut part, &[CHANGE_MARKS, COMMENT_MARKS])?;
        left -= limit - part.limit();
        // cut by a cap: the scan read to the cap and the part goes on past it, so what is past
        // the cap may have changes; a read that fails there is a failed read
        let cut = part.limit() == 0 && part.get_mut().read(&mut [0_u8]).ok()? == 1;
        if cut || found != Found::Clean {
            return Some(true);
        }
    }
    Some(false)
}

/// A relationships part as the scan takes it.
enum Rels {
    /// Its bytes, empty when the file has no such part.
    Read(Vec<u8>),
    /// Over [`MAX_RELS_BYTES`] or not UTF-8: not read for its relationships.
    Unread,
}

/// The relationships part `name` (at most `left`, which the read takes from); `None` when a read
/// failed.
fn read_rels<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
    left: &mut u64,
) -> Option<Rels> {
    let part = match zip.by_name(name) {
        Ok(part) => part,
        Err(zip::result::ZipError::Io(_)) => return None,
        Err(_) => return Some(Rels::Read(Vec::new())),
    };
    if part.size() > MAX_RELS_BYTES {
        return Some(Rels::Unread);
    }
    let limit = MAX_RELS_BYTES.min(*left);
    let mut bytes = Vec::new();
    // one byte past the limit, to tell a part over it: its stated size may be wrong
    part.take(limit + 1).read_to_end(&mut bytes).ok()?;
    let read = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    *left -= read.min(*left);
    if read > limit || !utf8(&bytes) {
        return Some(Rels::Unread);
    }
    Some(Rels::Read(bytes))
}

/// Whether a part's first bytes read as UTF-8 to the scan: no UTF-16 byte order mark and no NUL
/// in its first four bytes (UTF-16 or UTF-32 without a mark).
fn utf8(head: &[u8]) -> bool {
    !head.starts_with(&[0xFE, 0xFF])
        && !head.starts_with(&[0xFF, 0xFE])
        && !head.iter().take(4).any(|&byte| byte == 0)
}

/// What the scan of a part found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Found {
    /// An element in Word's main namespace with one of the marks' names.
    Mark,
    /// None, in a part read to its end as the scan models it.
    Clean,
    /// Something the scan does not model, or a part cut short: it may have changes.
    Unmodelled,
}

/// What `bytes`, one XML part, hold: an element in [`W_NAMESPACE`] with any of `marks`' local
/// names, read as it streams; `None` when a read failed before a mark was found.
fn holds_any(bytes: impl Read, marks: &[&[&[u8]]]) -> Option<Found> {
    let marks: Vec<&[u8]> = marks
        .iter()
        .flat_map(|names| names.iter().copied())
        .collect();
    let mut bytes = BufReader::with_capacity(64 * 1024, bytes);
    if !utf8(bytes.fill_buf().ok()?) {
        return Some(Found::Unmodelled);
    }
    let mut reader = quick_xml::NsReader::from_reader(bytes);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        let (resolved, event) = match reader.read_resolved_event_into(&mut buffer) {
            Ok(read) => read,
            Err(quick_xml::Error::Io(_)) => return None,
            Err(_) => return Some(Found::Unmodelled),
        };
        match event {
            Event::Start(element) | Event::Empty(element) => match resolved {
                ResolveResult::Bound(Namespace(namespace))
                    if namespace.as_bytes() == W_NAMESPACE
                        && marks.contains(&element.local_name().as_ref().as_bytes()) =>
                {
                    return Some(Found::Mark);
                }
                ResolveResult::Unknown(_) => return Some(Found::Unmodelled),
                ResolveResult::Bound(_) | ResolveResult::Unbound => {}
            },
            Event::Decl(declaration) => {
                let other = declaration.encoding().is_some_and(|encoding| {
                    !encoding.is_ok_and(|encoding| {
                        encoding.eq_ignore_ascii_case("utf-8")
                            || encoding.eq_ignore_ascii_case("utf8")
                    })
                });
                if other {
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

mod parts;
#[cfg(any(windows, test))]
mod reparse;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod xml_tests;
