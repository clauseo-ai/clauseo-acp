//! The listing rules, the asks and the per-build store, over a fake [`Probe`]. Windows paths are
//! written with `/`, which both Windows and Unix split into components.
// unix-paths-ok: the macOS paths are only compared, both sides resolved alike

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clauseo_protocol::{ErrorCode, HostError};
use clauseo_settings::{Files as _, SystemFiles};
use serde_json::json;

use super::store::{Answer, Store, build_id, exe_identity};
use super::*;

/// What the fake OS answers, and what it was asked.
struct Fake {
    platform: Platform,
    matters: Option<PathBuf>,
    documents: Option<PathBuf>,
    keychain: KeychainCheck,
    cfa: Option<Cfa>,
    protected: Vec<PathBuf>,
    exe: Option<PathBuf>,
    /// What listing Documents answers.
    listed: bool,
    /// The accounts whose read is refused.
    refused: Vec<&'static str>,
    /// Every folder listed and keychain account read, in order.
    asked: Mutex<Vec<String>>,
}

impl Fake {
    fn new(platform: Platform) -> Self {
        Self {
            platform,
            matters: Some(PathBuf::from("/Users/ada/Documents/Clauseo")),
            documents: Some(PathBuf::from("/Users/ada/Documents")),
            keychain: KeychainCheck::default(),
            cfa: None,
            protected: Vec::new(),
            exe: Some(PathBuf::from("C:/Program Files/Clauseo/Clauseo.exe")),
            listed: true,
            refused: Vec::new(),
            asked: Mutex::new(Vec::new()),
        }
    }

    fn windows(on: bool) -> Self {
        Self {
            matters: Some(PathBuf::from("C:/Users/ada/Documents/Clauseo")),
            protected: vec![PathBuf::from("C:/Users/ada/Documents")],
            cfa: Some(Cfa {
                on,
                ..Cfa::default()
            }),
            ..Self::new(Platform::Windows)
        }
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl Probe for Fake {
    fn platform(&self) -> Platform {
        self.platform
    }
    fn matters_folder(&self) -> Option<PathBuf> {
        self.matters.clone()
    }
    fn documents(&self) -> Option<PathBuf> {
        self.documents.clone()
    }
    fn keychain(&self) -> KeychainCheck {
        self.keychain.clone()
    }
    fn controlled_folder_access(&self) -> Option<Cfa> {
        self.cfa.clone()
    }
    fn protected_defaults(&self) -> Vec<PathBuf> {
        self.protected.clone()
    }
    fn exe(&self) -> Option<PathBuf> {
        self.exe.clone()
    }
    fn list_folder(&self, folder: &Path) -> Result<bool, HostError> {
        self.asked
            .lock()
            .unwrap()
            .push(folder.display().to_string());
        Ok(self.listed)
    }
    fn read_keychain(&self, item: &KeychainItem) -> bool {
        self.asked.lock().unwrap().push(item.account.clone());
        !self.refused.contains(&item.account.as_str())
    }
}

fn item(service: &str, account: &str) -> KeychainItem {
    KeychainItem {
        service: service.to_owned(),
        account: account.to_owned(),
    }
}

/// A store in a scratch folder, for `build`.
fn store(dir: &tempfile::TempDir, build: &str) -> Store {
    Store::new(Arc::new(SystemFiles), dir.path(), build.to_owned())
}

fn rows(probe: &dyn Probe, store: &Store) -> Vec<PermissionRow> {
    list(probe, &store.asked()).rows
}

/// No row.
const NONE: [PermissionRow; 0] = [];

fn ids(rows: &[PermissionRow]) -> Vec<PermissionId> {
    rows.iter().map(|row| row.id).collect()
}

#[test]
fn linux_lists_no_permission() {
    let dir = tempfile::tempdir().unwrap();
    let mut probe = Fake::windows(true);
    probe.platform = Platform::Other;
    probe
        .keychain
        .would_ask
        .push(item("ai.clauseo.desktop", "claude-acp"));
    assert_eq!(rows(&probe, &store(&dir, "1")), NONE);
}

#[test]
fn documents_is_listed_only_for_a_matters_folder_inside_documents() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir, "1");
    let mut probe = Fake::new(Platform::Mac);
    assert_eq!(
        rows(&probe, &store),
        [PermissionRow {
            id: PermissionId::FilesDocuments,
            state: OsPermissionState::NotAsked,
            folder: Some(
                canonical(Path::new("/Users/ada/Documents"))
                    .display()
                    .to_string()
            ),
            settings_path: None,
            note: None,
        }]
    );
    for (matters, listed) in [
        ("/Users/ada/Documents", true),
        ("/Users/ada/documents/Firm/Matters", true),
        ("/Users/ada/Documents2/Clauseo", false),
        ("/Users/ada/Clauseo", false),
        ("/Volumes/Share/Clauseo", false),
    ] {
        probe.matters = Some(PathBuf::from(matters));
        assert_eq!(!rows(&probe, &store).is_empty(), listed, "{matters}");
    }
    probe.matters = None;
    assert_eq!(rows(&probe, &store), NONE);
}

#[test]
fn a_real_folder_compares_by_its_canonical_path() {
    let scratch = tempfile::tempdir().unwrap();
    let documents = scratch.path().join("Documents");
    SystemFiles
        .write_atomic(&documents.join("keep.txt"), b"")
        .unwrap();
    let store = store(&scratch, "1");
    let mut probe = Fake::new(Platform::Mac);
    probe.documents = Some(documents.clone());
    probe.matters = Some(documents.join("..").join("Documents").join("Clauseo"));
    assert_eq!(ids(&rows(&probe, &store)), [PermissionId::FilesDocuments]);
    probe.matters = Some(scratch.path().join("Clauseo"));
    assert_eq!(rows(&probe, &store), NONE);
}

#[test]
fn asking_for_documents_records_the_answer_for_this_build() {
    let dir = tempfile::tempdir().unwrap();
    let mut probe = Fake::new(Platform::Mac);
    probe.listed = false;
    let row = ask(&probe, &store(&dir, "1"), PermissionId::FilesDocuments).unwrap();
    assert_eq!(row.state, OsPermissionState::Denied);
    assert_eq!(row.settings_path.as_deref(), Some(FILES_SETTINGS));
    assert_eq!(probe.asked(), [row.folder.clone().unwrap()]);
    assert_eq!(rows(&probe, &store(&dir, "1")), [row]);

    probe.listed = true;
    let row = ask(&probe, &store(&dir, "1"), PermissionId::FilesDocuments).unwrap();
    assert_eq!(
        (row.state, row.settings_path),
        (OsPermissionState::Granted, None)
    );
    let shown = rows(&probe, &store(&dir, "1"));
    assert_eq!(shown[0].state, OsPermissionState::Granted);
    // Another build asked nothing.
    let shown = rows(&probe, &store(&dir, "2"));
    assert_eq!(shown[0].state, OsPermissionState::NotAsked);
}

#[test]
fn keychain_is_listed_only_when_an_item_would_ask() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir, "1");
    let mut probe = Fake::new(Platform::Mac);
    probe.matters = Some(PathBuf::from("/Users/ada/Clauseo"));
    assert_eq!(rows(&probe, &store), NONE);

    probe.keychain.would_ask = vec![
        item("ai.clauseo.desktop", "claude-acp"),
        item("ai.clauseo.desktop.mcp", "files"),
    ];
    assert_eq!(
        rows(&probe, &store),
        [PermissionRow {
            id: PermissionId::Keychain,
            state: OsPermissionState::WouldAsk,
            folder: None,
            settings_path: None,
            note: Some("ai.clauseo.desktop (claude-acp), ai.clauseo.desktop.mcp (files)".into()),
        }]
    );

    probe.keychain.would_ask.clear();
    probe.keychain.unknown = vec![item("ai.clauseo.desktop.connectors", "caselaw")];
    assert_eq!(rows(&probe, &store)[0].state, OsPermissionState::Unknown);
}

#[test]
fn asking_the_keychain_reads_each_item_and_names_the_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut probe = Fake::new(Platform::Mac);
    probe.matters = Some(PathBuf::from("/Users/ada/Clauseo"));
    probe.keychain.would_ask = vec![
        item("ai.clauseo.desktop", "claude-acp"),
        item("ai.clauseo.desktop", "codex-acp"),
    ];
    probe.keychain.unknown = vec![item("ai.clauseo.desktop.mcp", "files")];
    probe.refused = vec!["codex-acp"];
    let row = ask(&probe, &store(&dir, "1"), PermissionId::Keychain).unwrap();
    assert_eq!(probe.asked(), ["claude-acp", "codex-acp", "files"]);
    assert_eq!(row.state, OsPermissionState::Denied);
    assert_eq!(row.note.as_deref(), Some("ai.clauseo.desktop (codex-acp)"));
    assert_eq!(
        rows(&probe, &store(&dir, "1"))[0].state,
        OsPermissionState::Denied
    );

    probe.refused.clear();
    let row = ask(&probe, &store(&dir, "1"), PermissionId::Keychain).unwrap();
    assert_eq!((row.state, row.note), (OsPermissionState::Granted, None));
    // Granted on this build: still shown once nothing would ask, so it is not asked again.
    probe.keychain = KeychainCheck::default();
    assert_eq!(
        rows(&probe, &store(&dir, "1")),
        [PermissionRow {
            id: PermissionId::Keychain,
            state: OsPermissionState::Granted,
            folder: None,
            settings_path: None,
            note: None,
        }]
    );
    assert_eq!(rows(&probe, &store(&dir, "2")), NONE);
}

#[test]
fn controlled_folder_access_is_listed_only_when_it_blocks_the_matters_folder() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir, "1");
    let blocking = PermissionRow {
        id: PermissionId::ControlledFolderAccess,
        state: OsPermissionState::Blocking,
        folder: None,
        settings_path: Some(CFA_SETTINGS.to_owned()),
        note: Some(CFA_NOTE.to_owned()),
    };
    assert_eq!(
        rows(&Fake::windows(true), &store),
        std::slice::from_ref(&blocking)
    );
    assert_eq!(rows(&Fake::windows(false), &store), NONE);

    let mut unreadable = Fake::windows(true);
    unreadable.cfa = None;
    assert_eq!(rows(&unreadable, &store), NONE);

    let mut outside = Fake::windows(true);
    outside.matters = Some(PathBuf::from("D:/Matters"));
    assert_eq!(rows(&outside, &store), NONE);
    // A folder the firm added to the protected list.
    if let Some(cfa) = outside.cfa.as_mut() {
        cfa.protected.push(PathBuf::from("d:/MATTERS"));
    }
    assert_eq!(rows(&outside, &store), std::slice::from_ref(&blocking));

    let mut allowed = Fake::windows(true);
    if let Some(cfa) = allowed.cfa.as_mut() {
        cfa.allowed
            .push(PathBuf::from("c:/program files/clauseo/CLAUSEO.EXE"));
    }
    assert_eq!(rows(&allowed, &store), NONE);

    let row = ask(
        &Fake::windows(true),
        &store,
        PermissionId::ControlledFolderAccess,
    )
    .unwrap();
    assert_eq!(row, blocking);
    assert!(
        store.asked().is_empty(),
        "nothing is recorded: nothing was asked"
    );
}

#[test]
fn asking_for_what_is_not_listed_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir, "1");
    let mac = Fake::new(Platform::Mac);
    for (probe, id) in [
        (&mac, PermissionId::Keychain),
        (&mac, PermissionId::ControlledFolderAccess),
        (&Fake::windows(true), PermissionId::FilesDocuments),
        (&Fake::windows(false), PermissionId::ControlledFolderAccess),
    ] {
        let error = ask(probe, &store, id).unwrap_err();
        assert_eq!(error.code(), ErrorCode::CommandInvalid, "{id:?}");
    }
    assert_eq!(mac.asked(), Vec::<String>::new());
}

#[test]
fn the_store_keeps_this_build_s_answers_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(store::FILE);
    store(&dir, "0.0.0-dev.ca9ddcad3")
        .record(PermissionId::FilesDocuments, Answer::Granted)
        .unwrap();
    store(&dir, "0.0.0-dev.ca9ddcad3")
        .record(PermissionId::Keychain, Answer::Denied)
        .unwrap();
    let text: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        text,
        json!({
            "build": "0.0.0-dev.ca9ddcad3",
            "asked": {"files_documents": "granted", "keychain": "denied"}
        })
    );
    // A new build starts again, and drops the old build's answers when it records.
    assert!(store(&dir, "0.1.0").asked().is_empty());
    store(&dir, "0.1.0")
        .record(PermissionId::Keychain, Answer::Granted)
        .unwrap();
    let text: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        text,
        json!({"build": "0.1.0", "asked": {"keychain": "granted"}})
    );
    // A file that does not parse counts as nothing asked, and is replaced.
    SystemFiles.write_atomic(&path, b"{not json").unwrap();
    assert!(store(&dir, "0.1.0").asked().is_empty());
    store(&dir, "0.1.0")
        .record(PermissionId::FilesDocuments, Answer::Denied)
        .unwrap();
    assert_eq!(store(&dir, "0.1.0").asked().len(), 1);
}

#[test]
fn the_build_id_is_the_version_and_the_executable_s_identity() {
    let exe = Some((1_764_000_000, 48_213_504));
    assert_eq!(build_id("0.0.0", None, exe), "0.0.0+1764000000-48213504");
    assert_eq!(
        build_id("0.0.0", Some("dev.ca9ddcad3"), exe),
        "0.0.0-dev.ca9ddcad3+1764000000-48213504"
    );
    assert_eq!(
        build_id("0.4.1", Some(""), exe),
        "0.4.1+1764000000-48213504"
    );
    // Unreadable metadata: the version alone.
    assert_eq!(
        build_id("0.0.0", Some("dev.ca9ddcad3"), None),
        "0.0.0-dev.ca9ddcad3"
    );
    assert_eq!(build_id("0.4.1", None, None), "0.4.1");
    // Another build of the same version is another id, so nothing counts as asked.
    assert_ne!(
        build_id("0.0.0", None, exe),
        build_id("0.0.0", None, Some((1_764_000_001, 48_213_504)))
    );
}

#[test]
fn the_running_executable_has_an_identity() {
    let (mtime, len) = exe_identity().unwrap();
    assert!(mtime > 0 && len > 0);
}

#[test]
fn defender_s_preference_parses() {
    let text = r#"{
        "EnableControlledFolderAccess": 1,
        "ControlledFolderAccessProtectedFolders": ["D:\\Matters", "E:\\Firm"],
        "ControlledFolderAccessAllowedApplications": "C:\\Tools\\a.exe"
    }"#;
    assert_eq!(
        parse_preference(text),
        Some(Cfa {
            on: true,
            protected: vec![PathBuf::from(r"D:\Matters"), PathBuf::from(r"E:\Firm")],
            allowed: vec![PathBuf::from(r"C:\Tools\a.exe")],
        })
    );
    let off = r#"{"EnableControlledFolderAccess": 2,
        "ControlledFolderAccessProtectedFolders": null,
        "ControlledFolderAccessAllowedApplications": null}"#;
    assert_eq!(parse_preference(off), Some(Cfa::default()));
    assert_eq!(
        parse_preference(r#"{"EnableControlledFolderAccess": null}"#),
        None
    );
    assert_eq!(parse_preference(""), None);
    assert_eq!(parse_preference("Get-MpPreference : not recognised"), None);
}

#[test]
fn rows_go_on_the_wire_in_snake_case() {
    let row = PermissionRow {
        id: PermissionId::ControlledFolderAccess,
        state: OsPermissionState::WouldAsk,
        folder: None,
        settings_path: Some("x".into()),
        note: None,
    };
    assert_eq!(
        serde_json::to_value(&row).unwrap(),
        json!({
            "id": "controlled_folder_access",
            "state": "would_ask",
            "folder": null,
            "settings_path": "x",
            "note": null
        })
    );
    let id: PermissionId = serde_json::from_value(json!("files_documents")).unwrap();
    assert_eq!(id, PermissionId::FilesDocuments);
}

/// The real `ls` through the process crate, read to its end: a folder that lists, one that
/// does not.
#[cfg(unix)]
#[test]
fn listing_a_folder_runs_a_child_to_its_end() {
    let dir = tempfile::tempdir().unwrap();
    SystemFiles
        .write_atomic(&dir.path().join("brief.docx"), b"")
        .unwrap();
    let probe = super::system::SystemProbe;
    assert!(probe.list_folder(dir.path()).unwrap());
    assert!(!probe.list_folder(&dir.path().join("gone")).unwrap());
}
