//! The archive's refusals and caps.

use std::io::Write as _;

use super::{Scratch, zip_of};
use clauseo_profile::packs::source::unzip::unzip_capped;
use clauseo_profile::packs::source::{SourceError, unzip};

/// Unzips `bytes` into a fresh folder of `dir`; on an error the folder is gone.
fn try_unzip(
    dir: &Scratch,
    bytes: &[u8],
) -> Result<clauseo_profile::packs::source::Unpacked, SourceError> {
    let into = dir.path().join("out");
    let result = unzip(bytes, &into);
    if result.is_err() {
        assert!(!into.exists(), "a refused archive leaves nothing behind");
    }
    result
}

#[test]
fn a_plain_archive_unpacks() {
    let dir = Scratch::new("zip-plain");
    let bytes = zip_of(&[
        ("repo-main/", b""),
        ("repo-main/skills/pdf/SKILL.md", b"---\nname: pdf\n---\n"),
        ("repo-main/README.md", b"# hi"),
    ]);
    let done = try_unzip(&dir, &bytes).unwrap();
    assert_eq!(done.files, 2);
    let read = std::fs::read(dir.path().join("out/repo-main/README.md")).unwrap();
    assert_eq!(read, b"# hi");
}

#[test]
fn a_path_outside_the_folder_is_refused() {
    let dir = Scratch::new("zip-slip");
    for name in [
        "../evil.txt",
        "a/../../evil.txt",
        "/etc/evil",
        "C:/evil.txt",
        "c:evil.txt",
        "a\\..\\evil.txt",
        "a/./b.txt",
        "a//b.txt",
        "nul.txt",
        "bad\u{1}name",
    ] {
        let bytes = zip_of(&[(name, b"x")]);
        assert_eq!(try_unzip(&dir, &bytes), Err(SourceError::Unsafe), "{name}");
    }
    assert!(!dir.path().parent().unwrap().join("evil.txt").exists());
}

#[test]
fn a_symlink_entry_is_refused() {
    let dir = Scratch::new("zip-link");
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("ok.md", options).unwrap();
    zip.write_all(b"fine").unwrap();
    zip.add_symlink("link", "/etc/passwd", options).unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    assert_eq!(try_unzip(&dir, &bytes), Err(SourceError::Unsafe));
}

#[test]
fn names_equal_but_for_case_are_refused() {
    let dir = Scratch::new("zip-case");
    let bytes = zip_of(&[("Read.md", b"a"), ("read.md", b"b")]);
    assert_eq!(try_unzip(&dir, &bytes), Err(SourceError::Unsafe));
}

#[test]
fn a_bomb_by_count_is_too_big() {
    let dir = Scratch::new("zip-count");
    let names: Vec<String> = (0..1001).map(|n| format!("f{n}.md")).collect();
    let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"x"[..])).collect();
    assert_eq!(try_unzip(&dir, &zip_of(&entries)), Err(SourceError::TooBig));
}

#[test]
fn a_bomb_by_size_is_too_big() {
    let dir = Scratch::new("zip-size");
    // 26 MiB of zeros deflates to a few dozen KiB, past the 25 MiB unpacked cap.
    let zeros = vec![0_u8; 26 * 1024 * 1024];
    let bytes = zip_of(&[("zeros.txt", &zeros)]);
    assert!(bytes.len() < 1024 * 1024);
    assert_eq!(try_unzip(&dir, &bytes), Err(SourceError::TooBig));
    // The cap counts every entry together.
    let into = dir.path().join("small-cap");
    let bytes = zip_of(&[("a.txt", &[1_u8; 600]), ("b.txt", &[2_u8; 600])]);
    assert_eq!(
        unzip_capped(&bytes, &into, (10, 1000)),
        Err(SourceError::TooBig)
    );
    assert!(!into.exists());
}

#[test]
fn not_a_zip_is_refused() {
    let dir = Scratch::new("zip-not");
    assert_eq!(
        try_unzip(&dir, b"<html>not found</html>"),
        Err(SourceError::Unsafe)
    );
}
