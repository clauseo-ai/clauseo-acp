//! Whether a listed `.docx` holds tracked changes or comments (`PaperSummary::tracked`), so its
//! tile offers Tracked changes only when there are some. Read from the file itself, off the
//! actor and the matter's worker (`actor/papers/scan.rs` runs [`check`] on a thread of its own):
//! its `word/document.xml` holds an insertion, a deletion or a move (`<w:ins `, `<w:del `,
//! `<w:moveFrom `, `<w:moveTo `), or its `word/comments.xml` a comment (`<w:comment `). Only
//! bytes are matched, never parsed, in one pass over each part (`memchr::memmem` finds each
//! `<w:`), and the read stops at the first mark and after [`MAX_XML_BYTES`] of a part, so a zip
//! bomb or a huge document costs a bounded read. A file that does not read as a zip, or has no
//! such part, has none. The file is opened without following a link at its last component.
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

/// What every mark starts with: what `memmem` looks for.
const PREFIX: &[u8] = b"<w:";

/// What follows [`PREFIX`] for a tracked change in `word/document.xml`.
const CHANGE_MARKS: &[&[u8]] = &[b"ins ", b"del ", b"moveFrom ", b"moveTo "];

/// What follows [`PREFIX`] for a comment in `word/comments.xml`.
const COMMENT_MARKS: &[&[u8]] = &[b"comment "];

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
/// be read (so the answer is not kept). Never follows a link at `path`.
pub(crate) fn check(path: &Path) -> Option<bool> {
    #[cfg(test)]
    if let Some(hook) = hooks::find(path) {
        return hook(path);
    }
    let file = crate::write::open_read_nofollow(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return Some(false);
    }
    zip_has_marks(BufReader::new(file))
}

/// Whether the zip `file` reads as a Word file that holds a tracked change or a comment; `None`
/// when a read failed.
fn zip_has_marks(file: impl Read + Seek) -> Option<bool> {
    let mut zip = match zip::ZipArchive::new(file) {
        Ok(zip) => zip,
        Err(zip::result::ZipError::Io(_)) => return None,
        Err(_) => return Some(false),
    };
    let mut holds = |name: &str, marks: &[&[u8]]| match zip.by_name(name) {
        Ok(part) => holds_any(part.take(MAX_XML_BYTES), marks),
        Err(zip::result::ZipError::Io(_)) => None,
        Err(_) => Some(false),
    };
    if holds("word/document.xml", CHANGE_MARKS)? {
        return Some(true);
    }
    holds("word/comments.xml", COMMENT_MARKS)
}

/// Whether `bytes` hold [`PREFIX`] followed by any of `marks`, read a block at a time in one
/// pass; `None` when a read failed.
fn holds_any(mut bytes: impl Read, marks: &[&[u8]]) -> Option<bool> {
    let finder = memchr::memmem::Finder::new(PREFIX);
    let longest = marks.iter().map(|mark| mark.len()).max().unwrap_or(0) + PREFIX.len();
    let mut buffer = vec![0_u8; 64 * 1024];
    // the block's first `carried` bytes are the last block's end: a mark may span the two
    let mut carried = 0;
    loop {
        let free = buffer.get_mut(carried..)?;
        let read = bytes.read(free).ok()?;
        let filled = carried + read;
        let block = buffer.get(..filled)?;
        for at in finder.find_iter(block) {
            let after = block.get(at + PREFIX.len()..).unwrap_or_default();
            if marks.iter().any(|mark| after.starts_with(mark)) {
                return Some(true);
            }
        }
        if read == 0 {
            return Some(false);
        }
        carried = (longest - 1).min(filled);
        buffer.copy_within(filled - carried..filled, 0);
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

#[cfg(test)]
mod tests;
