//! Whether a listed `.docx` holds tracked changes or comments (`PaperSummary::tracked`), so its
//! tile offers Tracked changes only when there are some. Read on the matter's worker as
//! `ListPapers` lists the folder, from the file itself: its `word/document.xml` holds an
//! insertion, a deletion or a move (`<w:ins `, `<w:del `, `<w:moveFrom `, `<w:moveTo `), or its
//! `word/comments.xml` a comment (`<w:comment `). Only bytes are matched, never parsed, and the
//! read stops at the first mark and after [`MAX_XML_BYTES`] of a part, so a zip bomb costs a
//! bounded read. A file that does not read as a zip, or has no such part, has none.
//!
//! Each answer is kept for the worker's thread by the file's path, size and modified time, so a
//! listing reads again only the Word files that changed since the last; the kept answers are the
//! last listing's, no more.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::{Path, PathBuf};

use clauseo_matters::PaperList;
use clauseo_protocol::{MatterPath, Timestamp};

use super::is_docx;

/// The most bytes of one XML part read, decompressed.
pub(crate) const MAX_XML_BYTES: u64 = 64 * 1024 * 1024;

/// What marks a tracked change in `word/document.xml`.
const CHANGE_MARKS: &[&[u8]] = &[b"<w:ins ", b"<w:del ", b"<w:moveFrom ", b"<w:moveTo "];

/// What marks a comment in `word/comments.xml`.
const COMMENT_MARKS: &[&[u8]] = &[b"<w:comment "];

/// A listing with the `.docx` files among its papers that hold tracked changes or comments.
#[derive(Debug)]
pub(crate) struct Listed {
    /// The folder's listing.
    pub(crate) list: PaperList,
    /// The papers whose tile offers Tracked changes.
    pub(crate) tracked: HashSet<MatterPath>,
}

/// A kept answer: the file's size and modified time when it was read, and what it held.
type Kept = HashMap<PathBuf, (u64, Timestamp, bool)>;

thread_local! {
    /// This worker's answers from its last listing.
    static KEPT: RefCell<Kept> = RefCell::new(HashMap::new());
}

/// `list` of `folder`, with its `.docx` papers that hold tracked changes or comments.
pub(crate) fn listed(folder: &Path, list: PaperList) -> Listed {
    KEPT.with_borrow_mut(|kept| {
        let mut now = Kept::new();
        let mut tracked = HashSet::new();
        for paper in list.papers.iter().filter(|paper| is_docx(&paper.path)) {
            let mut path = folder.to_path_buf();
            path.extend(paper.path.as_str().split('/'));
            let has = match kept.get(&path) {
                Some(&(size, modified, has))
                    if size == paper.size && modified == paper.modified =>
                {
                    has
                }
                _ => has_marks(&path),
            };
            if has {
                tracked.insert(paper.path.clone());
            }
            now.insert(path, (paper.size, paper.modified, has));
        }
        *kept = now;
        Listed { list, tracked }
    })
}

/// Whether the Word file at `path` holds a tracked change or a comment.
fn has_marks(path: &Path) -> bool {
    File::open(path).is_ok_and(|file| zip_has_marks(BufReader::new(file)))
}

/// Whether the zip `file` reads as a Word file that holds a tracked change or a comment.
fn zip_has_marks(file: impl Read + Seek) -> bool {
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        return false;
    };
    let mut holds = |name: &str, marks: &[&[u8]]| {
        zip.by_name(name)
            .is_ok_and(|part| holds_any(part.take(MAX_XML_BYTES), marks))
    };
    holds("word/document.xml", CHANGE_MARKS) || holds("word/comments.xml", COMMENT_MARKS)
}

/// Whether `bytes` hold any of `marks`, read a block at a time; a read that fails ends it.
fn holds_any(mut bytes: impl Read, marks: &[&[u8]]) -> bool {
    let keep = marks.iter().map(|mark| mark.len()).max().unwrap_or(1) - 1;
    let mut buffer = vec![0_u8; 64 * 1024];
    // the block's first `carried` bytes are the last block's end: a mark may span the two
    let mut carried = 0;
    loop {
        let Some(free) = buffer.get_mut(carried..) else {
            return false;
        };
        let read = match bytes.read(free) {
            Ok(0) | Err(_) => return false,
            Ok(read) => read,
        };
        let filled = carried + read;
        let Some(block) = buffer.get(..filled) else {
            return false;
        };
        if marks
            .iter()
            .any(|mark| block.windows(mark.len()).any(|at| at == *mark))
        {
            return true;
        }
        carried = keep.min(filled);
        buffer.copy_within(filled - carried..filled, 0);
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write as _};

    use super::{CHANGE_MARKS, COMMENT_MARKS, holds_any, zip_has_marks};

    /// A reader that hands out one byte at a time, so every mark spans two reads.
    struct Trickle<'a>(&'a [u8]);

    impl std::io::Read for Trickle<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let Some((first, rest)) = self.0.split_first() else {
                return Ok(0);
            };
            if let Some(slot) = buf.first_mut() {
                *slot = *first;
                self.0 = rest;
                return Ok(1);
            }
            Ok(0)
        }
    }

    #[test]
    fn a_mark_is_found_across_reads_and_a_near_miss_is_not() {
        let xml = b"<w:p><w:ins w:id=\"1\" w:author=\"A\"><w:r/></w:ins></w:p>";
        assert!(holds_any(Trickle(xml), CHANGE_MARKS));
        assert!(holds_any(&xml[..], CHANGE_MARKS));
        let plain =
            b"<w:p><w:r><w:t>w:ins and w:del are words here</w:t></w:r><w:instrText/></w:p>";
        assert!(!holds_any(&plain[..], CHANGE_MARKS));
        assert!(holds_any(
            &b"<w:comments><w:comment w:id=\"0\">"[..],
            COMMENT_MARKS
        ));
        assert!(!holds_any(&b"<w:comments></w:comments>"[..], COMMENT_MARKS));
    }

    /// A `.docx` with these parts, in memory.
    fn docx(parts: &[(&str, &str)]) -> Cursor<Vec<u8>> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, text) in parts {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        let mut file = zip.finish().unwrap();
        file.set_position(0);
        file
    }

    #[test]
    fn a_word_file_has_marks_by_its_changes_or_its_comments_only() {
        let body = "<w:body><w:p><w:r><w:t>Clause 4</w:t></w:r></w:p></w:body>";
        assert!(!zip_has_marks(docx(&[("word/document.xml", body)])));
        let changed = "<w:body><w:p><w:del w:id=\"2\" w:author=\"A\"/></w:p></w:body>";
        assert!(zip_has_marks(docx(&[("word/document.xml", changed)])));
        let comments = "<w:comments><w:comment w:id=\"0\" w:author=\"A\"/></w:comments>";
        let parts = [("word/document.xml", body), ("word/comments.xml", comments)];
        assert!(zip_has_marks(docx(&parts)));
        assert!(!zip_has_marks(Cursor::new(b"not a zip".to_vec())));
    }
}
