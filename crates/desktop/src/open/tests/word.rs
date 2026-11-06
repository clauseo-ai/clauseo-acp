//! `open_file` for a Word document whose path is too long for Word (`word_path`): it opens
//! through the short folder the OS gives, with its real name and `protected_view`, or is refused
//! as `matter.document_path_too_long`, and a refusal holds no later open back. The paths are real
//! (`check_document` resolves them); the short folders are the fake OS's.

use super::*;

/// A matter folder under `scratch` holding `name` five folders down, its path over the limit.
fn long_document(scratch: &Scratch, name: &str) -> (PathBuf, PathBuf) {
    let folder = scratch.dir("Acme");
    let rel = (0..5)
        .map(|n| format!("Folder {n} with a long name, to pass Word's limit"))
        .collect::<Vec<_>>()
        .join("/");
    scratch.dir(&format!("Acme/{rel}"));
    let path = scratch.file(&format!("Acme/{rel}/{name}"), b"PK");
    let resolved = check_document(&path, &folder).unwrap().path;
    assert!(
        resolved.as_os_str().len() > word_path::WORD_PATH_LIMIT,
        "{}",
        resolved.display()
    );
    (folder, path)
}

#[test]
fn a_long_word_document_opens_through_its_short_folder_in_protected_view() {
    let scratch = Scratch::new("open-word-short", &[]);
    let (folder, brief) = long_document(&scratch, "Brief of the claimant.docx");
    let host = host(&folder, &brief);
    let short = PathBuf::from("/SHORT~1");
    let opener = FakeOpener(Mutex::default(), Some(short.clone()));
    let opened = file(&host, &opener, host.file).unwrap();
    assert_eq!(
        opened,
        OpenedFile {
            protected_view: true
        }
    );
    assert_eq!(
        opener.opened(),
        [Opened::Path(
            short.join("Brief of the claimant.docx"),
            "docx".to_owned()
        )]
    );
}

#[test]
fn a_long_word_document_without_short_names_is_refused_and_opens_nothing() {
    let scratch = Scratch::new("open-word-refused", &[]);
    let (folder, brief) = long_document(&scratch, "Brief.DOCX");
    let host = host(&folder, &brief);
    let opener = FakeOpener::default();
    let clock = Clock::new();
    let gate = gate::Gate::default();
    let run = || block_on(open_file(&host, &opener, &gate, &|| clock.now(), host.file));
    let refused = run().unwrap_err();
    assert_eq!(refused.code(), ErrorCode::MatterDocumentPathTooLong);
    assert!(!refused.code().is_retryable());
    assert_eq!(opener.opened(), []);
    // The refusal does not hold the next open back: a short document opens at once.
    let (short_folder, short_brief, _) = matter_folder(&scratch);
    let short_host = super::host(&short_folder, &short_brief);
    let opened = block_on(open_file(
        &short_host,
        &opener,
        &gate,
        &|| clock.now(),
        short_host.file,
    ))
    .unwrap();
    assert_eq!(opened, OpenedFile::default());
}

#[test]
fn a_long_document_of_another_type_opens_as_it_is() {
    let scratch = Scratch::new("open-word-other", &[]);
    let (folder, report) = long_document(&scratch, "Report.pdf");
    let host = host(&folder, &report);
    let opener = FakeOpener(Mutex::default(), Some(PathBuf::from("/SHORT~1")));
    let opened = file(&host, &opener, host.file).unwrap();
    assert_eq!(opened, OpenedFile::default());
    assert_eq!(
        opener.opened(),
        [Opened::Path(
            check_document(&report, &folder).unwrap().path,
            "pdf".to_owned()
        )]
    );
}

#[test]
fn a_short_word_document_opens_as_it_is() {
    let scratch = Scratch::new("open-word-plain", &[]);
    let (folder, brief, _) = matter_folder(&scratch);
    let host = host(&folder, &brief);
    let opener = FakeOpener(Mutex::default(), Some(PathBuf::from("/SHORT~1")));
    assert_eq!(
        file(&host, &opener, host.file).unwrap(),
        OpenedFile::default()
    );
    assert_eq!(
        opener.opened(),
        [Opened::Path(
            check_document(&brief, &folder).unwrap().path,
            "docx".to_owned()
        )]
    );
}
