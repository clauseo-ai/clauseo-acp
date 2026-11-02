//! `locate_file` and `matter_folder`, the desktop's `open_file` and `reveal` (0026): a known ref
//! answers its matter, folder and file; an unknown ref, a file outside its matter's folder and a
//! moved folder are refused at call time; an unknown matter is refused; and a principal other
//! than the local desktop gets neither.

use std::path::PathBuf;

use clauseo_host::LocatedFile;
use clauseo_protocol::{
    ErrorCode, FileRefId, HostError, HostQuery, MatterId, PaperSummary, Principal, QueryResult,
};

use crate::support::{Dir, Running, foreign_uuid, wait};
use crate::write;

fn papers(host: &Running, matter: MatterId) -> Vec<PaperSummary> {
    match host.query(HostQuery::ListPapers { matter }).unwrap() {
        QueryResult::Papers { papers, .. } => papers,
        other => panic!("ListPapers answered {other:?}"),
    }
}

fn locate(host: &Running, file: FileRefId) -> Result<LocatedFile, HostError> {
    wait(
        "locate_file",
        host.handle.locate_file(Principal::LocalDesktop, file),
    )
}

fn folder_of(host: &Running, matter: MatterId) -> Result<PathBuf, HostError> {
    wait(
        "matter_folder",
        host.handle.matter_folder(Principal::LocalDesktop, matter),
    )
}

fn register(host: &Running, path: PathBuf) -> FileRefId {
    wait("register_file", host.handle.register_file(path)).unwrap()
}

#[test]
fn a_known_ref_answers_its_matter_folder_and_file() {
    let dir = Dir::new("locate_known", &[]);
    let host = dir.start(0);
    let folder = dir.folder("acme", &[("pleadings/claim.docx", "claim")]);
    let other = dir.folder("other", &[("x.txt", "x")]);
    let matter = host.create_matter("acme", &folder);
    host.create_matter("other", &other);
    let listed = papers(&host, matter);
    let claim = listed
        .iter()
        .find(|paper| paper.path.as_str() == "pleadings/claim.docx")
        .unwrap();
    let located = locate(&host, claim.file).unwrap();
    assert_eq!(
        located,
        LocatedFile {
            matter,
            folder: folder.clone(),
            path: folder.join("pleadings").join("claim.docx"),
        }
    );
    assert_eq!(folder_of(&host, matter).unwrap(), folder);
    host.shutdown();
}

#[test]
fn an_unknown_ref_is_refused() {
    let dir = Dir::new("locate_unknown", &[]);
    let host = dir.start(0);
    let unknown = FileRefId::new(foreign_uuid(1)).unwrap();
    let error = locate(&host, unknown).unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandInvalid, "{error}");
    host.shutdown();
}

#[test]
fn a_file_outside_its_matters_folder_is_refused() {
    let dir = Dir::new("locate_outside", &[]);
    let host = dir.start(0);
    let folder = dir.folder("acme", &[("brief.txt", "b")]);
    let elsewhere = dir.folder("elsewhere", &[("x.docx", "x")]);
    host.create_matter("acme", &folder);
    // In no matter's folder.
    let loose = register(&host, elsewhere.join("x.docx"));
    let error = locate(&host, loose).unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandInvalid, "{error}");
    // Written under the matter's folder, resolving outside it.
    let climbing = register(&host, folder.join("..").join("elsewhere").join("x.docx"));
    let error = locate(&host, climbing).unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandInvalid, "{error}");
    host.shutdown();
}

/// A link inside the matter's folder to a file outside it: the host's own check refuses it,
/// before the desktop's.
#[cfg(unix)]
#[test]
fn a_link_out_of_the_matters_folder_is_refused() {
    let dir = Dir::new("locate_link_out", &[]);
    let host = dir.start(0);
    let folder = dir.folder("acme", &[("brief.txt", "b")]);
    let elsewhere = dir.folder("elsewhere", &[("x.docx", "x")]);
    host.create_matter("acme", &folder);
    let link = folder.join("linked.docx");
    write::symlink(&elsewhere.join("x.docx"), &link);
    let linked = register(&host, link);
    let error = locate(&host, linked).unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandInvalid, "{error}");
    host.shutdown();
}

#[test]
fn a_ref_is_checked_again_when_asked() {
    let dir = Dir::new("locate_moved", &[]);
    let host = dir.start(0);
    let folder = dir.folder("acme", &[("brief.txt", "b")]);
    let matter = host.create_matter("acme", &folder);
    let brief = papers(&host, matter)[0].file;
    locate(&host, brief).unwrap();
    // The file gone, then the whole folder moved.
    write::rename(&folder.join("brief.txt"), &folder.join("renamed.txt"));
    let error = locate(&host, brief).unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandInvalid, "{error}");
    write::rename(&folder, &folder.with_file_name("moved"));
    let error = locate(&host, brief).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterFolderMissing, "{error}");
    host.shutdown();
}

#[test]
fn matter_folder_refuses_an_unknown_matter() {
    let dir = Dir::new("locate_unknown_matter", &[]);
    let host = dir.start(0);
    let unknown = MatterId::new(foreign_uuid(2)).unwrap();
    let error = folder_of(&host, unknown).unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandUnknownMatter, "{error}");
    host.shutdown();
}

#[test]
fn only_the_local_desktop_may_ask() {
    let dir = Dir::new("locate_principal", &[]);
    let host = dir.start(0);
    let folder = dir.folder("acme", &[("brief.txt", "b")]);
    let matter = host.create_matter("acme", &folder);
    let brief = papers(&host, matter)[0].file;
    // The webview never names a principal; any principal but the desktop's own is refused.
    let page: Principal = serde_json::from_str("\"webview\"").unwrap();
    assert!(matches!(page, Principal::Unknown { .. }));
    let error = wait("locate_file", host.handle.locate_file(page.clone(), brief)).unwrap_err();
    assert_eq!(error.code(), ErrorCode::TransportUnauthorized, "{error}");
    let error = wait("matter_folder", host.handle.matter_folder(page, matter)).unwrap_err();
    assert_eq!(error.code(), ErrorCode::TransportUnauthorized, "{error}");
    host.shutdown();
}
