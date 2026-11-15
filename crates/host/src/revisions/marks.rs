//! Whether a listed `.docx` holds tracked changes or comments (`PaperSummary::tracked`), so its
//! tile offers Tracked changes only when there are some. Read from the file itself, off the
//! actor and the matter's worker (`actor/papers/scan.rs` runs [`check`] on a thread of its own).
//!
//! A mark is any tracked change stetdoc 0.9.3 lists, accepts and rejects ([`CHANGE_MARKS`]:
//! text inserted, deleted or moved, and formatting, section, table, row, cell and numbering
//! changes), or a comment (`<w:comment `). It is looked for in every part stetdoc reads them
//! from: the main document and the headers, footers, footnotes, endnotes and comments its
//! relationships name ([`parts`]). Only bytes are matched, never parsed, in one pass over each
//! part (`memchr` finds each `<`): a mark is `<`, the prefix the part's root element binds to
//! Word's main XML namespace (`w` when it binds none), `:`, the name and white space. Marks
//! inside an XML comment or a CDATA section are text, not changes, and are passed over. The read
//! stops at the first mark, after
//! [`MAX_XML_BYTES`] of a part and after [`MAX_SCAN_BYTES`] in all, so a zip bomb or a huge
//! document costs a bounded read. A file that does not read as a zip, or has no such part, has
//! none. The file is opened without following a link at its last component.
//!
//! [`Kept`] holds each matter's answers by path, with the size and modified time the listing
//! gave, so a listing answers at once and only files that changed are read again. A read that
//! failed (a locked file, a sharing violation) is never kept: the next listing tries again.

use std::collections::HashMap;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use clauseo_matters::Paper;
use clauseo_protocol::{MatterPath, Timestamp};

use super::is_docx;

/// The most bytes of one XML part read, decompressed: a document over this is some thousands
/// of pages; a mark past it is missed and the tile offers no Tracked changes.
pub(crate) const MAX_XML_BYTES: u64 = 16 * 1024 * 1024;

/// The most bytes one file's scan reads, decompressed, over all its parts and relationships.
pub(crate) const MAX_SCAN_BYTES: u64 = 64 * 1024 * 1024;

/// The most bytes of a relationships part: a few hundred bytes a relationship, so Word's
/// hundred or so fit many times over. A larger part is taken as one with no relationships.
const MAX_RELS_BYTES: u64 = 64 * 1024;

/// Word's main XML namespace: a part's marks carry the prefix its root element binds to it.
const W_NAMESPACE: &[u8] = b"http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// The prefix of the marks of a part whose root element binds none to [`W_NAMESPACE`].
const DEFAULT_PREFIX: &[u8] = b"w";

/// The longest prefix taken from a root element: past it, a part's marks are taken to carry
/// [`DEFAULT_PREFIX`].
const MAX_PREFIX: usize = 64;

/// The most prefixes taken from a root element: past them, a part's marks are taken to carry
/// [`DEFAULT_PREFIX`].
const MAX_PREFIXES: usize = 8;

/// How much of a part is read at a time.
const BLOCK: usize = 64 * 1024;

/// An XML comment's opening after its `<`, and its end.
const COMMENT: (&[u8], &[u8]) = (b"!--", b"-->");

/// A CDATA section's opening after its `<`, and its end.
const CDATA: (&[u8], &[u8]) = (b"![CDATA[", b"]]>");

/// The element names after [`PREFIX`] of the tracked changes stetdoc 0.9.3 lists, accepts and
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

/// The element name of a comment.
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
/// part stetdoc reads them from (the main document first); `None` when a read failed.
fn zip_has_marks(file: impl Read + Seek) -> Option<bool> {
    let mut zip = match zip::ZipArchive::new(file) {
        Ok(zip) => zip,
        Err(zip::result::ZipError::Io(_)) => return None,
        Err(_) => return Some(false),
    };
    let mut left = MAX_SCAN_BYTES;
    let main = parts::main_part(&read_rels(&mut zip, "_rels/.rels", &mut left)?);
    let rels = read_rels(&mut zip, &parts::rels_of(&main), &mut left)?;
    let story = parts::story_parts(&main, &rels);
    for name in std::iter::once(&main).chain(&story) {
        if left == 0 {
            break;
        }
        let part = match zip.by_name(name) {
            Ok(part) => part,
            Err(zip::result::ZipError::Io(_)) => return None,
            Err(_) => continue,
        };
        let mut part = part.take(MAX_XML_BYTES.min(left));
        let limit = part.limit();
        let holds = holds_any(&mut part, &[CHANGE_MARKS, COMMENT_MARKS])?;
        left -= limit - part.limit();
        if holds {
            return Some(true);
        }
    }
    Some(false)
}

/// The relationships part `name` (at most `left`, which the read takes from), none when the file
/// has no such part or it is over [`MAX_RELS_BYTES`]; `None` when a read failed.
fn read_rels<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
    left: &mut u64,
) -> Option<Vec<u8>> {
    let part = match zip.by_name(name) {
        Ok(part) => part,
        Err(zip::result::ZipError::Io(_)) => return None,
        Err(_) => return Some(Vec::new()),
    };
    if part.size() > MAX_RELS_BYTES {
        return Some(Vec::new());
    }
    let limit = MAX_RELS_BYTES.min(*left);
    let mut bytes = Vec::new();
    // one byte past the limit, to tell a part over it: its stated size may be wrong
    part.take(limit + 1).read_to_end(&mut bytes).ok()?;
    let read = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    *left -= read.min(*left);
    if read > limit {
        return Some(Vec::new());
    }
    Some(bytes)
}

/// Whether `bytes` hold a mark with any of `marks`' names, outside comments and CDATA, read a
/// block at a time in one pass; `None` when a read failed before a mark was found.
fn holds_any(mut bytes: impl Read, marks: &[&[&[u8]]]) -> Option<bool> {
    let marks: Vec<&[u8]> = marks
        .iter()
        .flat_map(|names| names.iter().copied())
        .collect();
    let mut buffer = vec![0_u8; BLOCK];
    let (mut filled, mut ended) = fill(&mut bytes, &mut buffer, 0)?;
    let prefixes = bound_prefixes(buffer.get(..filled)?);
    // what a `<` needs after it to be told apart: `prefix:`, the name and its white space, or
    // a CDATA section's opening
    let longest = marks.iter().map(|mark| mark.len()).max().unwrap_or(0);
    let widest = prefixes.iter().map(Vec::len).max().unwrap_or(0);
    let ahead = (1 + widest + 1 + longest + 1).max(1 + CDATA.0.len());
    // the end of the comment or CDATA section the read is in
    let mut inside: Option<&[u8]> = None;
    loop {
        let block = buffer.get(..filled)?;
        // where a `<` has all it needs after it in the block; the rest waits for the next one
        let whole = if ended == Ended::No {
            filled.saturating_sub(ahead)
        } else {
            filled
        };
        let mut at = 0;
        while at < whole {
            if let Some(end) = inside {
                let Some(found) = memchr::memmem::find(block.get(at..)?, end) else {
                    // the block's last bytes may start the end
                    at = at.max(filled.saturating_sub(end.len() - 1));
                    break;
                };
                at += found + end.len();
                inside = None;
                continue;
            }
            let Some(open) = memchr::memchr(b'<', block.get(at..whole)?) else {
                at = whole;
                break;
            };
            at += open;
            let after = block.get(at + 1..).unwrap_or_default();
            if let Some((opening, end)) = [COMMENT, CDATA]
                .into_iter()
                .find(|(opening, _)| after.starts_with(opening))
            {
                at += 1 + opening.len();
                inside = Some(end);
            } else if is_mark(after, &prefixes, &marks) {
                return Some(true);
            } else {
                at += 1;
            }
        }
        match ended {
            Ended::No => {}
            Ended::Done => return Some(false),
            Ended::Failed => return None,
        }
        buffer.copy_within(at..filled, 0);
        (filled, ended) = fill(&mut bytes, &mut buffer, filled - at)?;
    }
}

/// Whether `after`, the bytes after a `<`, start one of `prefixes` (with its `:`, none for the
/// default namespace), one of `marks` and then white space.
fn is_mark(after: &[u8], prefixes: &[Vec<u8>], marks: &[&[u8]]) -> bool {
    prefixes.iter().any(|prefix| {
        let named = if prefix.is_empty() {
            Some(after)
        } else {
            after
                .strip_prefix(prefix.as_slice())
                .and_then(|rest| rest.strip_prefix(b":"))
        };
        named.is_some_and(|named| {
            marks.iter().any(|mark| {
                named
                    .strip_prefix(*mark)
                    .and_then(|rest| rest.first())
                    .is_some_and(u8::is_ascii_whitespace)
            })
        })
    })
}

/// The prefixes the root element in `head`, a part's first bytes, binds to [`W_NAMESPACE`] (an
/// empty one for the default namespace), or [`DEFAULT_PREFIX`] when it binds none, its tag is
/// not in `head`, or a prefix is past [`MAX_PREFIX`] or [`MAX_PREFIXES`].
fn bound_prefixes(head: &[u8]) -> Vec<Vec<u8>> {
    let fallback = || vec![DEFAULT_PREFIX.to_vec()];
    // the root's tag: the first `<` not opening a declaration, a comment or a doctype
    let mut rest = head;
    let tag = loop {
        let Some(open) = memchr::memchr(b'<', rest) else {
            return fallback();
        };
        rest = rest.get(open + 1..).unwrap_or_default();
        let past = if rest.starts_with(COMMENT.0) {
            memchr::memmem::find(rest, COMMENT.1).map(|end| end + COMMENT.1.len())
        } else if rest.starts_with(b"?") || rest.starts_with(b"!") {
            memchr::memchr(b'>', rest).map(|end| end + 1)
        } else {
            break memchr::memchr(b'>', rest).and_then(|end| rest.get(..end));
        };
        let Some(past) = past else {
            return fallback();
        };
        rest = rest.get(past..).unwrap_or_default();
    };
    let Some(tag) = tag else {
        return fallback();
    };
    // the attributes start after the element's name
    let name = tag
        .iter()
        .position(u8::is_ascii_whitespace)
        .unwrap_or(tag.len());
    let mut prefixes = Vec::new();
    for (name, value) in parts::attributes(tag.get(name..).unwrap_or_default()) {
        if value != W_NAMESPACE {
            continue;
        }
        let prefix = match name.strip_prefix(b"xmlns") {
            Some(b"") => b"".as_slice(),
            Some(declared) => match declared.strip_prefix(b":") {
                Some(prefix) if !prefix.is_empty() => prefix,
                _ => continue,
            },
            None => continue,
        };
        if prefix.len() > MAX_PREFIX || prefixes.len() == MAX_PREFIXES {
            return fallback();
        }
        prefixes.push(prefix.to_vec());
    }
    if prefixes.is_empty() {
        return fallback();
    }
    prefixes
}

/// How a [`fill`] stopped: the buffer full, the part at its end, or a read failed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ended {
    /// The buffer is full; the part may go on.
    No,
    /// The part ended.
    Done,
    /// A read failed after some bytes were read.
    Failed,
}

/// Reads into `buffer` after its first `filled` bytes until it is full, the part ends or a read
/// fails: how many bytes it then holds, and which stopped it. A read that fails with nothing
/// read since the last fill is `None`.
fn fill(bytes: &mut impl Read, buffer: &mut [u8], mut filled: usize) -> Option<(usize, Ended)> {
    let start = filled;
    while filled < buffer.len() {
        match bytes.read(buffer.get_mut(filled..)?) {
            Ok(0) => return Some((filled, Ended::Done)),
            Ok(read) => filled += read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) if filled > start => return Some((filled, Ended::Failed)),
            Err(_) => return None,
        }
    }
    Some((filled, Ended::No))
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
