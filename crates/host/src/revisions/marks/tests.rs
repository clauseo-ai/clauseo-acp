use std::io::{Cursor, Write as _};

use super::{CHANGE_MARKS, COMMENT_MARKS, check, holds_any, zip_has_marks};
use crate::pipeline::tests::support::TempDir;

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

/// A reader whose read fails after `ok` bytes, as a locked or vanished file's does.
struct Fails<'a>(&'a [u8]);

impl std::io::Read for Fails<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.0.is_empty() {
            return Err(std::io::Error::other("a sharing violation"));
        }
        let n = self.0.len().min(buf.len());
        buf[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}

#[test]
fn a_mark_is_found_across_reads_and_a_near_miss_is_not() {
    let xml = b"<w:p><w:ins w:id=\"1\" w:author=\"A\"><w:r/></w:ins></w:p>";
    assert_eq!(holds_any(Trickle(xml), CHANGE_MARKS), Some(true));
    assert_eq!(holds_any(&xml[..], CHANGE_MARKS), Some(true));
    let moved = b"<w:p><w:moveFrom w:id=\"3\"/></w:p>";
    assert_eq!(holds_any(Trickle(moved), CHANGE_MARKS), Some(true));
    let plain = b"<w:p><w:r><w:t>w:ins and w:del are words here</w:t></w:r><w:instrText/></w:p>";
    assert_eq!(holds_any(&plain[..], CHANGE_MARKS), Some(false));
    assert_eq!(holds_any(Trickle(plain), CHANGE_MARKS), Some(false));
    let comment = b"<w:comments><w:comment w:id=\"0\">";
    assert_eq!(holds_any(&comment[..], COMMENT_MARKS), Some(true));
    let none = b"<w:comments></w:comments>";
    assert_eq!(holds_any(&none[..], COMMENT_MARKS), Some(false));
}

#[test]
fn a_read_that_fails_is_unknown_not_unmarked() {
    let clean = b"<w:body><w:p><w:r><w:t>Clause 4</w:t></w:r></w:p>";
    assert_eq!(holds_any(Fails(clean), CHANGE_MARKS), None);
    // A mark before the failure still counts.
    let changed = b"<w:body><w:del w:id=\"2\"/>";
    assert_eq!(holds_any(Fails(changed), CHANGE_MARKS), Some(true));
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

const BODY: &str = "<w:body><w:p><w:r><w:t>Clause 4</w:t></w:r></w:p></w:body>";

#[test]
fn a_word_file_has_marks_by_its_changes_or_its_comments_only() {
    assert_eq!(
        zip_has_marks(docx(&[("word/document.xml", BODY)])),
        Some(false)
    );
    let changed = "<w:body><w:p><w:del w:id=\"2\" w:author=\"A\"/></w:p></w:body>";
    assert_eq!(
        zip_has_marks(docx(&[("word/document.xml", changed)])),
        Some(true)
    );
    let comments = "<w:comments><w:comment w:id=\"0\" w:author=\"A\"/></w:comments>";
    let parts = [("word/document.xml", BODY), ("word/comments.xml", comments)];
    assert_eq!(zip_has_marks(docx(&parts)), Some(true));
    assert_eq!(
        zip_has_marks(Cursor::new(b"not a zip".to_vec())),
        Some(false)
    );
}

#[test]
fn a_file_that_will_not_open_is_unknown() {
    let dir = TempDir::new("marks_unopened");
    assert_eq!(check(&dir.path().join("gone.docx")), None);
}

#[cfg(unix)]
#[test]
fn a_link_is_never_followed() {
    let dir = TempDir::new("marks_link");
    let changed = "<w:body><w:p><w:ins w:id=\"1\"/></w:p></w:body>";
    let real = dir.path().join("real.docx");
    crate::write::write_file(&real, docx(&[("word/document.xml", changed)]).get_ref()).unwrap();
    assert_eq!(check(&real), Some(true));
    let link = dir.path().join("link.docx");
    crate::write::symlink(&real, &link).unwrap();
    // O_NOFOLLOW refuses the link (ELOOP): unknown, never the target's answer.
    assert_eq!(check(&link), None);
}
