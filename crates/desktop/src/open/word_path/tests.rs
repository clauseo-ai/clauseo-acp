//! `word_open_path` with a fake folder resolver, on every OS, and with the real one on Windows.

use std::path::{Path, PathBuf};

use super::*;

/// A path of about 330 units: a folder of 300 `a`s under `/`, then `name`.
fn long(name: &str) -> PathBuf {
    Path::new("/").join("a".repeat(300)).join(name)
}

/// A resolver that shortens every folder to `/SHORT~1`.
fn short() -> impl Fn(&Path) -> Option<PathBuf> {
    |_| Some(PathBuf::from("/SHORT~1"))
}

/// A resolver for a volume without 8.3 names: the long form back.
fn unchanged() -> impl Fn(&Path) -> Option<PathBuf> {
    |folder| Some(folder.to_path_buf())
}

#[test]
fn a_long_word_document_opens_through_its_short_folder_with_its_real_name() {
    for name in [
        "Brief.docx",
        "Old.DOC",
        "Macro.Docm",
        "Plain.dotx",
        "T.dotm",
        "Note.rtf",
    ] {
        assert_eq!(
            word_open_path(&long(name), short()),
            Ok((Path::new("/SHORT~1").join(name), true)),
            "{name}"
        );
    }
}

#[test]
fn the_file_name_is_never_shortened() {
    let name = format!("{}.docx", "Long name ".repeat(20));
    let (path, protected) = word_open_path(&long(&name), short()).unwrap();
    assert!(protected);
    assert_eq!(path.file_name().unwrap().to_str(), Some(name.as_str()));
}

#[test]
fn a_long_word_document_without_short_names_is_refused() {
    assert_eq!(
        word_open_path(&long("Brief.docx"), unchanged()),
        Err(TooLong)
    );
    assert_eq!(word_open_path(&long("Brief.docx"), |_| None), Err(TooLong));
}

#[test]
fn a_long_word_document_still_too_long_when_shortened_is_refused() {
    // The short folder plus the name is 260 units.
    let name = format!(
        "{}.docx",
        "n".repeat(260 - "/SHORT~1/".len() - ".docx".len())
    );
    assert_eq!(word_open_path(&long(&name), short()), Err(TooLong));
    // One unit less fits.
    let name = &name[1..];
    let (path, protected) = word_open_path(&long(name), short()).unwrap();
    assert!(protected);
    assert_eq!(path.as_os_str().len(), WORD_PATH_LIMIT);
}

#[test]
fn other_files_and_paths_within_the_limit_open_as_they_are() {
    let never = |_: &Path| -> Option<PathBuf> { panic!("not asked") };
    for name in ["Brief.pdf", "Sheet.xlsx", "Notes.txt", "Brief"] {
        assert_eq!(
            word_open_path(&long(name), never),
            Ok((long(name), false)),
            "{name}"
        );
    }
    // Exactly at the limit, and under it.
    let at = Path::new("/").join("a".repeat(WORD_PATH_LIMIT - "/x/Brief.docx".len() - 1));
    let at = at.join("x").join("Brief.docx");
    assert_eq!(at.as_os_str().len(), WORD_PATH_LIMIT);
    assert_eq!(word_open_path(&at, never), Ok((at.clone(), false)));
    let under = Path::new("/Matters/Brief.docx");
    assert_eq!(
        word_open_path(under, never),
        Ok((under.to_path_buf(), false))
    );
}

#[test]
fn length_counts_utf16_units() {
    // `ह` is 3 bytes in UTF-8 and one UTF-16 unit: 240 of them fit where their bytes would not.
    let path = Path::new("/").join("ह".repeat(240)).join("Brief.docx");
    assert!(path.as_os_str().len() > WORD_PATH_LIMIT);
    assert_eq!(units(&path), 252);
    assert_eq!(
        word_open_path(&path, |_| -> Option<PathBuf> { panic!("not asked") }),
        Ok((path.clone(), false))
    );
}

#[test]
fn a_verbatim_prefix_is_dropped() {
    assert_eq!(
        plain_str(r"\\?\C:\Matters\a"),
        Some(r"C:\Matters\a".to_owned())
    );
    assert_eq!(
        plain_str(r"\\?\UNC\server\share\a"),
        Some(r"\\server\share\a".to_owned())
    );
    assert_eq!(plain_str(r"C:\Matters\a"), None);
}

/// With real folders over the limit: where the scratch volume keeps 8.3 names, the document
/// resolves to its short folders and its real name, which name the same file; where it keeps
/// none (CI's volume may have 8.3 creation off), it is refused. Either branch passes, and the
/// test prints which ran.
#[cfg(windows)]
#[test]
fn a_long_word_document_on_this_volume_opens_short_or_is_refused() {
    use crate::test_support::Scratch;

    let scratch = Scratch::new("open-word-long-path", &[]);
    let rel = (0..8)
        .map(|n| format!("Subfolder {n} with a long name to pass MAX_PATH"))
        .collect::<Vec<_>>()
        .join("/");
    scratch.dir(&rel);
    let brief = scratch.file(&format!("{rel}/Brief of the claimant.docx"), b"PK");
    // Resolved as open_file hands it on: verbatim, since it is this long.
    let brief = crate::open::check_document(&brief, scratch.path())
        .unwrap()
        .path;
    assert!(
        units(&plain(&brief)) > WORD_PATH_LIMIT,
        "{}",
        brief.display()
    );

    let parent = brief.parent().unwrap();
    let folder = short_folder(parent);
    let has_short_names = folder.as_ref().is_some_and(|f| plain(f) != plain(parent));
    let expected = folder
        .map(|f| plain(&f.join("Brief of the claimant.docx")))
        .filter(|p| units(p) <= WORD_PATH_LIMIT);
    if let (true, Some(expected)) = (has_short_names, expected) {
        println!("branch: 8.3 names on this volume; opens through short folders");
        let (path, protected) = word_open_path(&brief, short_folder).unwrap();
        assert!(protected);
        assert_eq!(path, expected);
        assert!(!path.to_string_lossy().starts_with(r"\\?\"));
        assert_eq!(
            path.file_name().unwrap().to_str(),
            Some("Brief of the claimant.docx")
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"PK");
    } else {
        println!("branch: no 8.3 names on this volume (or still too long); refused");
        assert_eq!(word_open_path(&brief, short_folder), Err(TooLong));
    }
}
