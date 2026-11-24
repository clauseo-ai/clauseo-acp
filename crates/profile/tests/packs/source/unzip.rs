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

/// Names Windows reads as a device or as another file's 8.3 short name are refused: `COM¹`,
/// `CON .txt` (Windows drops the spaces), `PROGRA~1`. A `~` not followed by digits is a name.
#[test]
fn windows_device_names_and_short_aliases_are_refused() {
    let dir = Scratch::new("zip-device");
    for name in [
        "COM¹.txt",
        "lpt³",
        "CON .txt",
        "skills/aux  .md",
        "PROGRA~1/x.md",
        "file~2.md",
        "a/b~12",
    ] {
        let bytes = zip_of(&[(name, b"x")]);
        assert_eq!(try_unzip(&dir, &bytes), Err(SourceError::Unsafe), "{name}");
    }
    let fine = zip_of(&[("a~b.md", b"x"), ("console.md", b"y"), ("com10.md", b"z")]);
    assert_eq!(try_unzip(&dir, &fine).unwrap().files, 3);
}

/// A file and a folder of the same name, in either order, are refused, leaving nothing.
#[test]
fn a_file_versus_folder_clash_is_refused() {
    let dir = Scratch::new("zip-clash");
    let file_first = zip_of(&[("a", b"file"), ("a/b.md", b"inside")]);
    assert_eq!(try_unzip(&dir, &file_first), Err(SourceError::Unsafe));
    let folder_first = zip_of(&[("a/b.md", b"inside"), ("a", b"file")]);
    assert_eq!(try_unzip(&dir, &folder_first), Err(SourceError::Unsafe));
}

/// An encrypted entry is refused (`ZipCrypto` here; AES the same).
#[test]
fn an_encrypted_entry_is_refused() {
    use zip::unstable::write::FileOptionsExt as _;
    let dir = Scratch::new("zip-encrypted");
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .with_deprecated_encryption(b"secret")
        .unwrap();
    zip.start_file("SKILL.md", options).unwrap();
    zip.write_all(b"---\nname: x\n---\n").unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    assert_eq!(try_unzip(&dir, &bytes), Err(SourceError::Unsafe));
}

/// A name of at most 400 bytes and 32 parts unpacks; one byte or one part more is refused.
#[test]
fn the_name_and_depth_caps_hold() {
    let dir = Scratch::new("zip-caps");
    let part = "x".repeat(99);
    let at_cap = format!("{part}/{part}/{part}/{part}y");
    assert_eq!(at_cap.len(), 400);
    let over = format!("{at_cap}z");
    let deep = vec!["d"; 32].join("/");
    let deeper = vec!["d"; 33].join("/");
    assert_eq!(
        try_unzip(&dir, &zip_of(&[(&at_cap, b"x")])).unwrap().files,
        1
    );
    let dir = Scratch::new("zip-caps-deep");
    assert_eq!(try_unzip(&dir, &zip_of(&[(&deep, b"x")])).unwrap().files, 1);
    for name in [&over, &deeper] {
        let dir = Scratch::new("zip-caps-over");
        assert_eq!(
            try_unzip(&dir, &zip_of(&[(name, b"x")])),
            Err(SourceError::Unsafe),
            "{} bytes",
            name.len()
        );
    }
}
