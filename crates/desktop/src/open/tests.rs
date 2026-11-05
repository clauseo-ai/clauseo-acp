//! The three commands' checks, against a fake host, confirm and OS: what each refuses, what a
//! declined confirm does, and that nothing opens unless every check passed. The link checks and
//! the confirm's words are in `links.rs`; the files and folders here.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use clauseo_protocol::{ErrorCode, FileRefId, HostError, Language, MatterId};
use uuid::Uuid;

use super::*;
use crate::test_support::Scratch;

mod links;

/// What the fake OS was asked to do, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Opened {
    Url(String),
    /// A file, and the type it was opened as.
    Path(PathBuf, String),
    Reveal(PathBuf),
}

#[derive(Default)]
struct FakeOpener(Mutex<Vec<Opened>>);

impl FakeOpener {
    fn opened(&self) -> Vec<Opened> {
        self.0.lock().unwrap().clone()
    }
}

impl Opener for FakeOpener {
    fn open_url(&self, url: &str) -> Result<(), HostError> {
        self.0.lock().unwrap().push(Opened::Url(url.to_owned()));
        Ok(())
    }
    fn open_path(&self, path: &Path, kind: &str) -> Result<(), HostError> {
        self.0
            .lock()
            .unwrap()
            .push(Opened::Path(path.to_path_buf(), kind.to_owned()));
        Ok(())
    }
    fn reveal(&self, path: &Path) -> Result<(), HostError> {
        self.0
            .lock()
            .unwrap()
            .push(Opened::Reveal(path.to_path_buf()));
        Ok(())
    }
}

/// Answers every confirm with `answer`, and keeps the words it was shown.
struct FakeConfirm {
    answer: bool,
    shown: Mutex<Vec<LinkWords>>,
}

impl FakeConfirm {
    fn new(answer: bool) -> Self {
        Self {
            answer,
            shown: Mutex::new(Vec::new()),
        }
    }
    fn shown(&self) -> Vec<LinkWords> {
        self.shown.lock().unwrap().clone()
    }
}

impl LinkConfirm for FakeConfirm {
    fn confirm_link(&self, words: LinkWords) -> BoxFuture<'_, bool> {
        self.shown.lock().unwrap().push(words);
        let answer = self.answer;
        Box::pin(async move { answer })
    }
}

/// A host that knows one file ref and one matter.
struct FakeHost {
    file: FileRefId,
    located: LocatedFile,
}

impl Locate for FakeHost {
    fn locate_file(&self, file: FileRefId) -> BoxFuture<'_, Result<LocatedFile, HostError>> {
        let answer = if file == self.file {
            Ok(self.located.clone())
        } else {
            Err(HostError::new(
                ErrorCode::CommandInvalid,
                format!("file ref {file} was not issued"),
            ))
        };
        Box::pin(async move { answer })
    }
    fn matter_folder(&self, matter: MatterId) -> BoxFuture<'_, Result<PathBuf, HostError>> {
        let answer = if matter == self.located.matter {
            Ok(self.located.folder.clone())
        } else {
            Err(HostError::new(
                ErrorCode::CommandUnknownMatter,
                format!("no matter {matter}"),
            ))
        };
        Box::pin(async move { answer })
    }
}

fn file_ref() -> FileRefId {
    FileRefId::new(Uuid::now_v7()).unwrap()
}

fn matter_id() -> MatterId {
    MatterId::new(Uuid::now_v7()).unwrap()
}

/// A host whose one ref names `path` in the matter whose folder is `folder`.
fn host(folder: &Path, path: &Path) -> FakeHost {
    FakeHost {
        file: file_ref(),
        located: LocatedFile {
            matter: matter_id(),
            folder: folder.to_path_buf(),
            path: path.to_path_buf(),
        },
    }
}

/// A clock the test moves by hand.
struct Clock(Mutex<Instant>);

impl Clock {
    fn new() -> Self {
        Self(Mutex::new(gate::now()))
    }
    fn now(&self) -> Instant {
        *self.0.lock().unwrap()
    }
    fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }
}

/// `open_external` as the command runs it: the link checked first, then the confirm, through a
/// fresh gate.
fn external(
    confirm: &FakeConfirm,
    opener: &FakeOpener,
    clock: &Clock,
    language: Language,
    url: &str,
) -> Result<(), HostError> {
    let link = check_link(url)?;
    let gate = gate::Gate::default();
    block_on(open_external(
        confirm,
        opener,
        &gate,
        &|| clock.now(),
        language,
        &link,
    ))
}

/// `open_file` through a fresh gate.
fn file(host: &FakeHost, opener: &FakeOpener, file: FileRefId) -> Result<(), HostError> {
    let clock = Clock::new();
    let gate = gate::Gate::default();
    block_on(open_file(host, opener, &gate, &|| clock.now(), file))
}

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    futures::executor::block_on(future)
}

fn code(result: Result<impl std::fmt::Debug, HostError>) -> ErrorCode {
    result.unwrap_err().code()
}

/// A matter folder under `scratch` holding `Brief.docx`, and a file beside the folder.
fn matter_folder(scratch: &Scratch) -> (PathBuf, PathBuf, PathBuf) {
    let folder = scratch.path().join("Acme");
    scratch.dir("Acme/Drafts");
    let brief = scratch.file("Acme/Drafts/Brief.docx", b"PK");
    let outside = scratch.file("Outside.docx", b"PK");
    (folder, brief, outside)
}

#[test]
fn a_document_in_its_matter_opens_by_its_resolved_path() {
    let scratch = Scratch::new("open-document", &[]);
    let (folder, brief, _) = matter_folder(&scratch);
    // Named through `..`: what opens is the resolved path.
    let host = host(
        &folder,
        &folder.join("Drafts").join("..").join("Drafts/Brief.docx"),
    );
    let opener = FakeOpener::default();
    file(&host, &opener, host.file).unwrap();
    assert_eq!(
        opener.opened(),
        [Opened::Path(
            dunce::canonicalize(&brief).unwrap(),
            "docx".to_owned()
        )]
    );
}

#[test]
fn an_unknown_ref_opens_nothing() {
    let scratch = Scratch::new("open-unknown", &[]);
    let (folder, brief, _) = matter_folder(&scratch);
    let host = host(&folder, &brief);
    let opener = FakeOpener::default();
    let refused = file(&host, &opener, file_ref());
    assert_eq!(code(refused), ErrorCode::CommandInvalid);
    assert_eq!(opener.opened(), []);
}

#[test]
fn a_file_outside_its_matter_is_refused() {
    let scratch = Scratch::new("open-outside", &[]);
    let (folder, _, outside) = matter_folder(&scratch);
    let opener = FakeOpener::default();
    for path in [outside.as_path(), &folder.join("..").join("Outside.docx")] {
        let host = host(&folder, path);
        let refused = file(&host, &opener, host.file);
        assert_eq!(
            code(refused),
            ErrorCode::CommandInvalid,
            "{}",
            path.display()
        );
    }
    // A link inside the folder to a file outside it.
    #[cfg(unix)]
    {
        let link = scratch.link(&outside, "Acme/Linked.docx");
        let host = host(&folder, &link);
        let refused = file(&host, &opener, host.file);
        assert_eq!(code(refused), ErrorCode::CommandInvalid);
    }
    assert_eq!(opener.opened(), []);
}

/// A document in a nested subfolder of its matter opens, and what it hands on has no `\\?\`
/// prefix at a normal length: `dunce` keeps the prefix only past `MAX_PATH`.
#[test]
fn a_document_in_a_nested_folder_of_its_matter_opens_plain() {
    let scratch = Scratch::new("open-nested", &[]);
    let folder = scratch.path().join("Acme");
    scratch.dir("Acme/Clients/2026/Drafts");
    let brief = scratch.file("Acme/Clients/2026/Drafts/Brief.docx", b"PK");
    let document = check_document(&brief, &folder).unwrap();
    assert!(
        !document.path.to_string_lossy().starts_with(r"\\?\"),
        "{}",
        document.path.display()
    );
    assert_eq!(document.kind, "docx");
    let host = host(&folder, &brief);
    let opener = FakeOpener::default();
    file(&host, &opener, host.file).unwrap();
    assert_eq!(
        opener.opened(),
        [Opened::Path(document.path, "docx".to_owned())]
    );
}

/// `starts_with` compares whole components, so a sibling whose name begins with the folder's
/// name (`Acme2` beside `Acme`) is not inside it.
#[test]
fn a_sibling_whose_name_starts_with_the_folders_is_refused() {
    let scratch = Scratch::new("open-prefix", &[]);
    let folder = scratch.dir("Acme");
    scratch.dir("Acme2");
    let sibling = scratch.file("Acme2/Brief.docx", b"PK");
    let opener = FakeOpener::default();
    let host = host(&folder, &sibling);
    let refused = file(&host, &opener, host.file);
    assert_eq!(code(refused), ErrorCode::CommandInvalid);
    assert_eq!(opener.opened(), []);
}

/// Eight nested folder names, about 370 characters with the separators: a document under them
/// passes `MAX_PATH` (260 UTF-16 units) on Windows, whatever the scratch folder's own length.
#[cfg(windows)]
fn deep_folders() -> Vec<String> {
    (0..8)
        .map(|n| format!("Subfolder {n} with a long name to pass MAX_PATH"))
        .collect()
}

/// On Windows a document past `MAX_PATH` in a short matter folder is accepted, and it is handed on
/// verbatim, as a long path must be. Both sides canonicalize verbatim, so `starts_with` finds
/// the folder. A file outside the matter, named through `..`, is still refused.
#[cfg(windows)]
#[test]
fn a_long_document_in_a_short_matter_folder_opens_verbatim() {
    use std::os::windows::ffi::OsStrExt as _;

    let scratch = Scratch::new("open-long-path", &[]);
    let folder = scratch.dir("Acme");
    let nested = deep_folders();
    let rel = nested.join("/");
    scratch.dir(&format!("Acme/{rel}"));
    let brief = scratch.file(&format!("Acme/{rel}/Brief.docx"), b"PK");
    let long = dunce::canonicalize(&brief).unwrap();
    assert!(
        long.as_os_str().encode_wide().count() > 260,
        "{}",
        long.display()
    );

    let document = check_document(&brief, &folder).unwrap();
    assert!(
        document.path.to_string_lossy().starts_with(r"\\?\"),
        "a long path stays verbatim: {}",
        document.path.display()
    );
    let host = host(&folder, &brief);
    let opener = FakeOpener::default();
    file(&host, &opener, host.file).unwrap();
    assert_eq!(
        opener.opened(),
        [Opened::Path(document.path, "docx".to_owned())]
    );

    // `Acme\<first folder>\..\..\Outside.docx` is `Outside.docx` beside the matter's folder.
    scratch.file("Outside.docx", b"PK");
    let through = folder
        .join(&nested[0])
        .join("..")
        .join("..")
        .join("Outside.docx");
    let refused = check_document(&through, &folder);
    assert_eq!(code(refused), ErrorCode::CommandInvalid);
    assert_eq!(opener.opened().len(), 1);
}

#[test]
fn only_document_files_open() {
    let scratch = Scratch::new("open-types", &[]);
    let (folder, _, _) = matter_folder(&scratch);
    let opener = FakeOpener::default();
    // A folder, and a folder named like an app bundle.
    scratch.dir("Acme/Tool.app/Contents");
    let mut refused = vec![folder.join("Drafts"), folder.join("Tool.app")];
    for name in [
        "run.command",
        "setup.exe",
        "a.sh",
        "a.bat",
        "a.lnk",
        "page.html",
        "a.svg",
        "macro.docm",
        "macro.xlsm",
        "noext",
        "a.webloc",
        "a.terminal",
    ] {
        refused.push(scratch.file(&format!("Acme/{name}"), b"x"));
    }
    for path in refused {
        let host = host(&folder, &path);
        let answer = file(&host, &opener, host.file);
        assert_eq!(
            code(answer),
            ErrorCode::CommandInvalid,
            "{}",
            path.display()
        );
    }
    assert_eq!(opener.opened(), []);

    // The type is matched without regard to case.
    let upper = scratch.file("Acme/NOTE.PDF", b"%PDF");
    let host = host(&folder, &upper);
    file(&host, &opener, host.file).unwrap();
    assert_eq!(opener.opened().len(), 1);
}

/// A document marked as a program (an execute bit) is refused on Linux, whatever its name says.
/// macOS opens with the checked type's app instead (`mac_app`).
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn a_file_marked_as_a_program_is_refused() {
    let scratch = Scratch::new("open-executable", &[]);
    let (folder, brief, _) = matter_folder(&scratch);
    scratch.executable("Acme/Drafts/Brief.docx");
    let host = host(&folder, &brief);
    let opener = FakeOpener::default();
    let refused = file(&host, &opener, host.file);
    assert_eq!(code(refused), ErrorCode::CommandInvalid);
    assert_eq!(opener.opened(), []);
}

/// One open at a time and at most one a second: a page cannot open documents in a loop.
#[test]
fn files_open_one_at_a_time_and_at_most_one_a_second() {
    let scratch = Scratch::new("open-gate", &[]);
    let (folder, brief, _) = matter_folder(&scratch);
    let host = host(&folder, &brief);
    let opener = FakeOpener::default();
    let clock = Clock::new();
    let gate = gate::Gate::default();
    let run = || block_on(open_file(&host, &opener, &gate, &|| clock.now(), host.file));
    let pass = gate.enter(clock.now()).unwrap();
    assert_eq!(code(run()), ErrorCode::CommandConflict);
    drop(pass);
    // A refused call does not hold the next back.
    let refused = block_on(open_file(
        &host,
        &opener,
        &gate,
        &|| clock.now(),
        file_ref(),
    ));
    assert_eq!(code(refused), ErrorCode::CommandInvalid);
    run().unwrap();
    clock.advance(BETWEEN_OPENS.checked_sub(Duration::from_millis(1)).unwrap());
    assert_eq!(code(run()), ErrorCode::CommandConflict);
    clock.advance(Duration::from_millis(1));
    run().unwrap();
    assert_eq!(opener.opened().len(), 2);
}

#[test]
fn a_missing_file_or_folder_opens_nothing() {
    let scratch = Scratch::new("open-missing", &[]);
    let (folder, brief, _) = matter_folder(&scratch);
    let opener = FakeOpener::default();
    let gone = host(&folder, &folder.join("Gone.docx"));
    let refused = file(&gone, &opener, gone.file);
    assert_eq!(code(refused), ErrorCode::CommandInvalid);

    let moved = host(&scratch.path().join("Moved"), &brief);
    let refused = file(&moved, &opener, moved.file);
    assert_eq!(code(refused), ErrorCode::MatterFolderMissing);
    assert_eq!(opener.opened(), []);
}

#[test]
fn reveal_shows_a_matters_folder_or_the_app_data_folder() {
    let scratch = Scratch::new("reveal", &["data"]);
    let (folder, brief, _) = matter_folder(&scratch);
    let host = host(&folder, &brief);
    let app_data = scratch.path().join("data");
    let opener = FakeOpener::default();
    block_on(reveal(
        &host,
        &opener,
        &app_data,
        RevealTarget::Matter(host.located.matter),
    ))
    .unwrap();
    block_on(reveal(&host, &opener, &app_data, RevealTarget::AppData)).unwrap();
    assert_eq!(
        opener.opened(),
        [Opened::Reveal(folder), Opened::Reveal(app_data)]
    );
}

#[test]
fn reveal_refuses_an_unknown_matter_and_a_missing_folder() {
    let scratch = Scratch::new("reveal-refused", &[]);
    let (folder, brief, _) = matter_folder(&scratch);
    let opener = FakeOpener::default();
    let host_ = host(&folder, &brief);
    let unknown = block_on(reveal(
        &host_,
        &opener,
        scratch.path(),
        RevealTarget::Matter(matter_id()),
    ));
    assert_eq!(code(unknown), ErrorCode::CommandUnknownMatter);

    let moved = host(&scratch.path().join("Moved"), &brief);
    let missing = block_on(reveal(
        &moved,
        &opener,
        scratch.path(),
        RevealTarget::Matter(moved.located.matter),
    ));
    assert_eq!(code(missing), ErrorCode::MatterFolderMissing);
    assert_eq!(opener.opened(), []);
}

#[test]
fn a_reveal_target_reads_a_matter_or_app_data_only() {
    let matter = matter_id();
    let read: RevealTarget =
        serde_json::from_value(serde_json::json!({ "matter": matter.to_string() })).unwrap();
    assert_eq!(read, RevealTarget::Matter(matter));
    let read: RevealTarget = serde_json::from_value(serde_json::json!("app_data")).unwrap();
    assert_eq!(read, RevealTarget::AppData);
    for bad in [
        serde_json::json!("home"),
        serde_json::json!({ "path": "/etc" }),
        serde_json::json!({ "matter": "not an id" }),
        serde_json::json!({ "app_data": null, "matter": matter.to_string() }),
    ] {
        assert!(
            serde_json::from_value::<RevealTarget>(bad.clone()).is_err(),
            "{bad}"
        );
    }
}
