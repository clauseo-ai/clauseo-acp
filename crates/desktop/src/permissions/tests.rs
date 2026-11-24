//! The listing rules, the asks and the per-build store, over a fake [`Probe`]. Windows paths are
//! written with `/`, which both Windows and Unix split into components.
// unix-paths-ok: the macOS paths are only compared, both sides resolved alike

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
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
    /// What listing Documents answers.
    listed: bool,
    /// The accounts whose read is refused.
    refused: Vec<&'static str>,
    /// Every folder listed and keychain account read, in order.
    asked: Mutex<Vec<String>>,
    /// How many times Controlled folder access was read.
    cfa_reads: AtomicUsize,
    /// Holds the read open until the test sends on its sender (a second caller can arrive then).
    hold: Mutex<Option<Receiver<()>>>,
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
            listed: true,
            refused: Vec::new(),
            asked: Mutex::new(Vec::new()),
            cfa_reads: AtomicUsize::new(0),
            hold: Mutex::new(None),
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
        self.cfa_reads.fetch_add(1, Ordering::SeqCst);
        let hold = self.hold.lock().unwrap().take();
        if let Some(release) = hold {
            // Under way until the test lets it finish, after the second caller is started.
            let _released = release.recv();
        }
        self.cfa.clone()
    }
    fn protected_defaults(&self) -> Vec<PathBuf> {
        self.protected.clone()
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
fn controlled_folder_access_is_read_once_per_process_and_shared_while_under_way() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir, "1");
    let held = Fake::windows(true);
    let (release, hold) = channel();
    *held.hold.lock().unwrap() = Some(hold);
    let probe = ReadOnce::new(held);
    // Two at once (a reload while the first screen's read is under way): one read, both rows.
    let both = std::thread::scope(|scope| {
        let first = scope.spawn(|| rows(&probe, &store));
        while probe.probe.cfa_reads.load(Ordering::SeqCst) == 0 {
            std::thread::yield_now();
        }
        let second = scope.spawn(|| rows(&probe, &store));
        release.send(()).unwrap();
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert!(
        both.iter()
            .all(|listed| ids(listed) == [PermissionId::ControlledFolderAccess])
    );
    // Later calls and an ask reuse it.
    assert_eq!(
        ids(&rows(&probe, &store)),
        [PermissionId::ControlledFolderAccess]
    );
    ask(&probe, &store, PermissionId::ControlledFolderAccess).unwrap();
    assert_eq!(probe.probe.cfa_reads.load(Ordering::SeqCst), 1);
    // A read that could not tell is kept too: it is not tried again on every call.
    let mut unreadable = Fake::windows(true);
    unreadable.cfa = None;
    let probe = ReadOnce::new(unreadable);
    assert_eq!(rows(&probe, &store), NONE);
    assert_eq!(rows(&probe, &store), NONE);
    assert_eq!(probe.probe.cfa_reads.load(Ordering::SeqCst), 1);
}

#[test]
fn controlled_folder_access_follows_the_most_specific_registry_place() {
    use CfaLayer::{Set, Unreadable, Unset};
    let on = Some(Cfa {
        on: true,
        protected: Vec::new(),
    });
    let off = Some(Cfa::default());
    let from = |layers: &[CfaLayer]| cfa_from_layers(layers, Vec::new());
    // Group policy over MDM over Defender's own.
    assert_eq!(from(&[Set(1), Set(0), Set(0)]), on);
    assert_eq!(from(&[Unset, Set(0), Set(1)]), off);
    assert_eq!(from(&[Unset, Unset, Set(1)]), on);
    // A place that cannot be read falls through to the next.
    assert_eq!(from(&[Unreadable, Unset, Set(1)]), on);
    // Audit and disk-only modes do not block; another value cannot be told.
    assert_eq!(from(&[Set(2)]), off);
    assert_eq!(from(&[Set(4)]), off);
    assert_eq!(from(&[Set(7)]), None);
    // Nothing set anywhere: Defender's default, off. Nothing set and a place unread: cannot be
    // told, as with no place at all (off Windows), and no row is listed, as before.
    assert_eq!(from(&[Unset, Unset, Unset]), off);
    assert_eq!(from(&[Unset, Unreadable, Unset]), None);
    assert_eq!(from(&[]), None);
    // The places' added folders come along.
    assert_eq!(
        cfa_from_layers(&[Set(1)], vec![PathBuf::from("D:/Matters")]),
        Some(Cfa {
            on: true,
            protected: vec![PathBuf::from("D:/Matters")],
        })
    );
}

/// The environment the expansion tests see, looked up case-blind as Windows' is.
fn windows_env(name: &str) -> Option<String> {
    match name.to_ascii_uppercase().as_str() {
        "USERPROFILE" => Some("C:\\Users\\ada".to_owned()),
        "PUBLIC" => Some("C:\\Users\\Public".to_owned()),
        _ => None,
    }
}

#[test]
fn an_added_folder_expands_its_variables_and_keeps_what_it_cannot() {
    use super::expand::expand_with;
    assert_eq!(
        expand_with("%USERPROFILE%\\Matters", windows_env),
        "C:\\Users\\ada\\Matters"
    );
    assert_eq!(
        expand_with("%PUBLIC%\\Documents\\Firm", windows_env),
        "C:\\Users\\Public\\Documents\\Firm"
    );
    // Names are not case-sensitive, as Windows' are.
    assert_eq!(
        expand_with("%userprofile%\\Matters", windows_env),
        "C:\\Users\\ada\\Matters"
    );
    // Two variables, and an unknown one left as written.
    assert_eq!(
        expand_with("%PUBLIC%\\%USERPROFILE%", windows_env),
        "C:\\Users\\Public\\C:\\Users\\ada"
    );
    assert_eq!(
        expand_with("%NOPE%\\Matters", windows_env),
        "%NOPE%\\Matters"
    );
    assert_eq!(
        expand_with("%NOPE%%USERPROFILE%", windows_env),
        "%NOPE%C:\\Users\\ada"
    );
    // A lone % is left alone, and a value is not expanded again.
    assert_eq!(expand_with("D:\\100%", windows_env), "D:\\100%");
    assert_eq!(expand_with("D:\\a%b", windows_env), "D:\\a%b");
    assert_eq!(expand_with("D:\\Matters", windows_env), "D:\\Matters");
    assert_eq!(
        expand_with("%NOPE%", |_| Some("%USERPROFILE%".to_owned())),
        "%USERPROFILE%"
    );
}

#[test]
fn controlled_folder_access_counts_a_folder_added_with_a_variable() {
    use super::expand::expand_with;
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir, "1");
    let mut added = Fake::windows(true);
    // Windows' own defaults do not cover the matters folder: only the added folder does.
    added.protected = Vec::new();
    added.matters = Some(PathBuf::from("C:/Users/ada/Matters/Clauseo"));
    let blocking = PermissionRow {
        id: PermissionId::ControlledFolderAccess,
        state: OsPermissionState::Blocking,
        folder: None,
        settings_path: Some(CFA_SETTINGS.to_owned()),
        note: Some(CFA_NOTE.to_owned()),
    };
    let home = |name: &str| {
        name.eq_ignore_ascii_case("USERPROFILE")
            .then(|| "C:/Users/ada".to_owned())
    };
    if let Some(cfa) = added.cfa.as_mut() {
        cfa.protected = vec![PathBuf::from(expand_with("%USERPROFILE%/Matters", home))];
    }
    assert_eq!(rows(&added, &store), std::slice::from_ref(&blocking));

    // A variable that does not expand leaves the folder as written, which the matters folder is
    // not under.
    if let Some(cfa) = added.cfa.as_mut() {
        cfa.protected = vec![PathBuf::from(expand_with("%NOPE%/Matters", home))];
    }
    assert_eq!(rows(&added, &store), NONE);
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
