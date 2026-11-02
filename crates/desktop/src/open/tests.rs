//! The three commands' checks, against a fake host, confirm and OS: what each refuses, what a
//! declined confirm does, and that nothing opens unless every check passed.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use clauseo_protocol::{ErrorCode, FileRefId, HostError, Language, MatterId};
use uuid::Uuid;

use super::*;
use crate::test_support::Scratch;

/// What the fake OS was asked to do, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Opened {
    Url(String),
    Path(PathBuf),
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
    fn open_path(&self, path: &Path) -> Result<(), HostError> {
        self.0
            .lock()
            .unwrap()
            .push(Opened::Path(path.to_path_buf()));
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

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    futures::executor::block_on(future)
}

fn code(result: Result<impl std::fmt::Debug, HostError>) -> ErrorCode {
    result.unwrap_err().code()
}

#[test]
fn web_links_name_their_host_in_ascii() {
    let link = check_link("https://www.sci.gov.in/judgments?q=1#top").unwrap();
    assert_eq!(link.destination, Destination::Web("www.sci.gov.in".into()));
    assert_eq!(link.url, "https://www.sci.gov.in/judgments?q=1#top");

    let link = check_link("http://localhost:8080/x").unwrap();
    assert_eq!(link.destination, Destination::Web("localhost:8080".into()));

    // An international name shows in its xn-- form, so a look-alike cannot pass as another.
    let link = check_link("https://аpple.com/").unwrap();
    assert_eq!(
        link.destination,
        Destination::Web("xn--pple-43d.com".into())
    );
    assert_eq!(link.url, "https://xn--pple-43d.com/");

    // Upper case and spaces around are the url crate's to tidy; the host is what shows.
    let link = check_link("HTTPS://Example.COM").unwrap();
    assert_eq!(link.destination, Destination::Web("example.com".into()));
}

#[test]
fn a_mailto_link_names_its_addresses() {
    let link = check_link("mailto:clerk@court.example?subject=Filing&body=Hi").unwrap();
    assert_eq!(
        link.destination,
        Destination::Mail("clerk@court.example".into())
    );
    let link = check_link("mailto:a@x.example,b@y.example").unwrap();
    assert_eq!(
        link.destination,
        Destination::Mail("a@x.example,b@y.example".into())
    );
}

#[test]
fn other_links_are_refused() {
    let long = format!("https://example.com/{}", "a".repeat(MAX_LINK_BYTES));
    for bad in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "data:text/html,<script>1</script>",
        "ftp://example.com/x",
        "tauri://localhost/index.html",
        "smb://server/share",
        "x-apple.systempreferences:com.apple.preference.security",
        "ms-settings:privacy",
        "about:blank",
        "not a link",
        "",
        "/relative/path",
        "https://user:secret@example.com/",
        "https://example.com@evil.example/",
        "mailto:",
        "mailto:nobody",
        // A right-to-left override, and a space, in the address.
        "mailto:%E2%80%AEmoc.live@a",
        "mailto:a%20b@x.example",
        "mailto:%ZZ@x.example",
        long.as_str(),
    ] {
        let refused = check_link(bad);
        assert_eq!(code(refused), ErrorCode::CommandInvalid, "{bad}");
    }
}

#[test]
fn a_bad_scheme_is_refused_before_any_confirm() {
    let confirm = FakeConfirm::new(true);
    let opener = FakeOpener::default();
    let refused = block_on(open_external(
        &confirm,
        &opener,
        Language::En,
        "file:///Applications/Calculator.app",
    ));
    assert_eq!(code(refused), ErrorCode::CommandInvalid);
    assert!(confirm.shown().is_empty());
    assert!(opener.opened().is_empty());
}

#[test]
fn a_declined_confirm_opens_nothing_and_is_unauthorized() {
    let confirm = FakeConfirm::new(false);
    let opener = FakeOpener::default();
    let declined = block_on(open_external(
        &confirm,
        &opener,
        Language::En,
        "https://example.com/a",
    ));
    let error = declined.unwrap_err();
    assert_eq!(error.code(), ErrorCode::TransportUnauthorized);
    assert!(!error.retryable());
    assert_eq!(confirm.shown().len(), 1);
    assert!(opener.opened().is_empty());
}

#[test]
fn an_accepted_confirm_opens_the_link_it_named() {
    let confirm = FakeConfirm::new(true);
    let opener = FakeOpener::default();
    block_on(open_external(
        &confirm,
        &opener,
        Language::En,
        "https://example.com/a",
    ))
    .unwrap();
    assert_eq!(
        confirm.shown(),
        [LinkWords {
            title: "Open this link in your browser?".into(),
            message: "The link goes to example.com.".into(),
            ok: "Open link".into(),
            cancel: "Cancel".into(),
        }]
    );
    assert_eq!(
        opener.opened(),
        [Opened::Url("https://example.com/a".into())]
    );
}

#[test]
fn the_confirm_speaks_the_apps_language() {
    let confirm = FakeConfirm::new(true);
    let opener = FakeOpener::default();
    block_on(open_external(
        &confirm,
        &opener,
        Language::Hi,
        "mailto:clerk@court.example",
    ))
    .unwrap();
    let shown = confirm.shown();
    assert_eq!(shown[0].title, words::HI.mail_title);
    assert_eq!(
        shown[0].message,
        "आपका ईमेल ऐप clerk@court.example के लिए नया संदेश खोलेगा।"
    );
    assert_eq!(shown[0].ok, words::HI.mail_ok);
    assert_eq!(shown[0].cancel, words::HI.cancel);
}

/// A matter folder under `scratch` holding `Brief.docx`, and a file beside the folder.
fn matter_folder(scratch: &Scratch) -> (PathBuf, PathBuf, PathBuf) {
    let folder = scratch.path().join("Acme");
    std::fs::create_dir_all(folder.join("Drafts")).unwrap();
    let brief = folder.join("Drafts").join("Brief.docx");
    std::fs::write(&brief, b"PK").unwrap();
    let outside = scratch.path().join("Outside.docx");
    std::fs::write(&outside, b"PK").unwrap();
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
    block_on(open_file(&host, &opener, host.file)).unwrap();
    assert_eq!(
        opener.opened(),
        [Opened::Path(dunce::canonicalize(&brief).unwrap())]
    );
}

#[test]
fn an_unknown_ref_opens_nothing() {
    let scratch = Scratch::new("open-unknown", &[]);
    let (folder, brief, _) = matter_folder(&scratch);
    let host = host(&folder, &brief);
    let opener = FakeOpener::default();
    let refused = block_on(open_file(&host, &opener, file_ref()));
    assert_eq!(code(refused), ErrorCode::CommandInvalid);
    assert!(opener.opened().is_empty());
}

#[test]
fn a_file_outside_its_matter_is_refused() {
    let scratch = Scratch::new("open-outside", &[]);
    let (folder, _, outside) = matter_folder(&scratch);
    let opener = FakeOpener::default();
    for path in [outside.clone(), folder.join("..").join("Outside.docx")] {
        let host = host(&folder, &path);
        let refused = block_on(open_file(&host, &opener, host.file));
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
        let link = folder.join("Linked.docx");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let host = host(&folder, &link);
        let refused = block_on(open_file(&host, &opener, host.file));
        assert_eq!(code(refused), ErrorCode::CommandInvalid);
    }
    assert!(opener.opened().is_empty());
}

#[test]
fn only_document_files_open() {
    let scratch = Scratch::new("open-types", &[]);
    let (folder, _, _) = matter_folder(&scratch);
    let opener = FakeOpener::default();
    // A folder, and a folder named like an app bundle.
    let bundle = folder.join("Tool.app");
    std::fs::create_dir_all(bundle.join("Contents")).unwrap();
    let mut refused = vec![folder.join("Drafts"), bundle];
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
        let path = folder.join(name);
        std::fs::write(&path, b"x").unwrap();
        refused.push(path);
    }
    for path in refused {
        let host = host(&folder, &path);
        let answer = block_on(open_file(&host, &opener, host.file));
        assert_eq!(
            code(answer),
            ErrorCode::CommandInvalid,
            "{}",
            path.display()
        );
    }
    assert!(opener.opened().is_empty());

    // The type is matched without regard to case.
    let upper = folder.join("NOTE.PDF");
    std::fs::write(&upper, b"%PDF").unwrap();
    let host = host(&folder, &upper);
    block_on(open_file(&host, &opener, host.file)).unwrap();
    assert_eq!(opener.opened().len(), 1);
}

#[test]
fn a_missing_file_or_folder_opens_nothing() {
    let scratch = Scratch::new("open-missing", &[]);
    let (folder, brief, _) = matter_folder(&scratch);
    let opener = FakeOpener::default();
    let gone = host(&folder, &folder.join("Gone.docx"));
    let refused = block_on(open_file(&gone, &opener, gone.file));
    assert_eq!(code(refused), ErrorCode::CommandInvalid);

    let moved = host(&scratch.path().join("Moved"), &brief);
    let refused = block_on(open_file(&moved, &opener, moved.file));
    assert_eq!(code(refused), ErrorCode::MatterFolderMissing);
    assert!(opener.opened().is_empty());
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
        [Opened::Reveal(folder.clone()), Opened::Reveal(app_data)]
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
    assert!(opener.opened().is_empty());
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
