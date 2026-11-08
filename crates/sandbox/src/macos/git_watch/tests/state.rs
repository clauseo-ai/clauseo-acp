//! The matter's state: an item a final scan left blocks launches while it is there, a dead
//! session's final scan runs at the next launch or the host's next start once none of its
//! processes is left, and while one is left nothing moves and launches are refused; a
//! session's end removes its record, and one that outlived its kill marks it.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;

use super::super::aside::Stuck;
use super::super::host;
use super::super::run::Mode;
use super::super::state::{self, Baseline, Bytes, Host, Known, MatterState, SessionRecord};
use super::fake::{FakeFs, MemStore, Notices, SET_ASIDE};
use crate::{Denial, DenialSink, GitItem};

fn record(session: &str, baseline: Baseline) -> SessionRecord {
    SessionRecord {
        session: session.to_owned(),
        host_pid: 1,
        host_start: Some(1),
        probe: format!("chat.clauseo.session.{session}"),
        set_aside: Bytes::from(&PathBuf::from(SET_ASIDE)),
        baseline,
        launched: Some(1_000),
        outlived: false,
        finds: Vec::new(),
        mode: Mode::Live,
    }
}

/// A running session's record, launched with nothing known.
pub(super) fn record_of(session: &str) -> SessionRecord {
    record(session, Baseline::default())
}

fn begin(
    store: &MemStore,
    fs: &FakeFs,
    dead: &[&str],
    killed: &RefCell<Vec<String>>,
) -> (std::io::Result<()>, Arc<Notices>) {
    let notices = Arc::new(Notices::default());
    let sink = Arc::clone(&notices) as Arc<dyn DenialSink>;
    let is_dead = |r: &SessionRecord| dead.contains(&r.session.as_str());
    let kill = |r: &SessionRecord| {
        assert_eq!(
            r.launched,
            Some(1_000),
            "the kill skips what started before"
        );
        killed.borrow_mut().push(r.probe.clone());
        true
    };
    let host = Host {
        dead: &is_dead,
        kill: &kill,
    };
    let out = state::begin(store, fs, &PathBuf::from("/m"), &sink, &host);
    (out, notices)
}

#[test]
fn a_dead_sessions_final_scan_runs_at_the_next_launch() {
    let fs = FakeFs::new();
    fs.dir("/m/archive");
    fs.bare("/m/archive");
    let store = MemStore::default();
    // The dead session knew the archive; a live one is left alone.
    let id = |path: &str| {
        super::super::Fs::meta(&fs, &PathBuf::from(path))
            .unwrap()
            .id
    };
    let base = Baseline {
        repos: vec![Known::new(&PathBuf::from("/m/archive"), id("/m/archive"))],
        ..Baseline::default()
    };
    state::record(&store, record("dead", base)).unwrap();
    state::record(&store, record("live", Baseline::default())).unwrap();
    // What the dead session left: a .git and a new repository-shaped folder.
    fs.repo("/m/.git");
    fs.dir("/m/new");
    fs.bare("/m/new");
    let killed = RefCell::new(Vec::new());
    let (out, notices) = begin(&store, &fs, &["dead"], &killed);
    out.unwrap();
    assert_eq!(*killed.borrow(), ["chat.clauseo.session.dead"]);
    assert!(!fs.exists("/m/.git") && fs.exists("/m/archive/HEAD"));
    // Its folders were never kept, so a repository-shaped one loses only its HEAD.
    assert!(fs.exists("/m/new/objects") && !fs.exists("/m/new/HEAD"));
    let got: Vec<(PathBuf, GitItem, bool)> = notices
        .taken()
        .into_iter()
        .map(|d| {
            let Denial::GitSetAside {
                path,
                what,
                session_ended,
                ..
            } = d
            else {
                panic!("{d:?}");
            };
            (path, what, session_ended)
        })
        .collect();
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(got.iter().all(|(_, _, ended)| !ended));
    let sessions: Vec<String> = store
        .0
        .lock()
        .unwrap()
        .sessions
        .iter()
        .map(|r| r.session.clone())
        .collect();
    assert_eq!(sessions, ["live"]);
}

#[test]
fn an_item_left_in_place_refuses_the_matter_until_it_is_gone() {
    let fs = FakeFs::new();
    fs.repo("/m/.git");
    let store = MemStore::default();
    state::record(&store, record("s", Baseline::default())).unwrap();
    let stuck = vec![Stuck {
        path: PathBuf::from("/m/.git"),
        reason: "Operation not permitted".to_owned(),
    }];
    state::end(&store, "s", stuck);
    assert_eq!(store.0.lock().unwrap().sessions.len(), 0);
    let killed = RefCell::new(Vec::new());
    let (out, _) = begin(&store, &fs, &[], &killed);
    let error = out.unwrap_err().to_string();
    assert_eq!(
        error,
        "Clauseo won't start an AI tool in this matter until .git is moved out of it: an \
         earlier AI tool left it there, and Clauseo could neither move nor rename it \
         (Operation not permitted)."
    );
    super::super::Fs::rename_new(&fs, &PathBuf::from("/m/.git"), &PathBuf::from("/away")).unwrap();
    let (out, _) = begin(&store, &fs, &[], &killed);
    out.unwrap();
    assert_eq!(
        *store.0.lock().unwrap(),
        MatterState {
            matter: "/m".to_owned(),
            root: Some(Bytes::from(&PathBuf::from("/m"))),
            ..MatterState::default()
        }
    );
}

/// The host's start's sweep of `store`, with sessions `dead` dead and kills that leave a
/// process when `left`.
fn start_up(store: &MemStore, fs: &FakeFs, dead: &[&str], left: bool) -> (usize, Arc<Notices>) {
    let notices = Arc::new(Notices::default());
    let sink = Arc::clone(&notices) as Arc<dyn DenialSink>;
    let is_dead = |r: &SessionRecord| dead.contains(&r.session.as_str());
    let kill = |_: &SessionRecord| !left;
    let host = Host {
        dead: &is_dead,
        kill: &kill,
    };
    (state::sweep_dead(store, fs, &sink, &host).unwrap(), notices)
}

fn sessions(store: &MemStore) -> Vec<String> {
    let state = store.0.lock().unwrap();
    state.sessions.iter().map(|r| r.session.clone()).collect()
}

/// A store as a launch of `/m` left it, with sessions `names` recorded.
fn launched(names: &[&str]) -> MemStore {
    let store = MemStore::default();
    store.0.lock().unwrap().root = Some(Bytes::from(&PathBuf::from("/m")));
    for name in names {
        state::record(&store, record(name, Baseline::default())).unwrap();
    }
    store
}

#[test]
fn the_hosts_start_runs_a_dead_sessions_final_scan_without_a_launch() {
    let fs = FakeFs::new();
    let store = launched(&["dead", "live"]);
    fs.repo("/m/.git");
    let (scanned, notices) = start_up(&store, &fs, &["dead"], false);
    assert_eq!(scanned, 1);
    assert!(!fs.exists("/m/.git"));
    assert_eq!(notices.count(), 1);
    assert_eq!(sessions(&store), ["live"]);
}

/// A launch's `state::begin` of `/m` with every session dead, its kills leaving a process
/// when `left`.
fn launch(store: &MemStore, fs: &FakeFs, left: bool) -> (std::io::Result<()>, Arc<Notices>) {
    let notices = Arc::new(Notices::default());
    let sink = Arc::clone(&notices) as Arc<dyn DenialSink>;
    let is_dead = |_: &SessionRecord| true;
    let kill = |_: &SessionRecord| !left;
    let host = Host {
        dead: &is_dead,
        kill: &kill,
    };
    let out = state::begin(store, fs, &PathBuf::from("/m"), &sink, &host);
    (out, notices)
}

const STILL_RUNNING: &str = "Clauseo won't start an AI tool in this matter yet: a process of an \
                             earlier AI tool session is still running after Clauseo tried to \
                             stop it. Restart the Mac, then try again.";

#[test]
fn a_dead_session_with_a_process_left_moves_nothing_and_refuses_launches() {
    let fs = FakeFs::new();
    let store = launched(&["dead"]);
    fs.repo("/m/.git");
    // The host's start: killed, not scanned, kept.
    let (scanned, notices) = start_up(&store, &fs, &["dead"], true);
    assert_eq!(scanned, 0, "nothing scanned while a process is left");
    assert!(fs.exists("/m/.git"));
    assert_eq!(notices.count(), 0);
    assert_eq!(sessions(&store), ["dead"]);
    // A launch: refused, plainly, and still nothing moves.
    let (out, notices) = launch(&store, &fs, true);
    assert_eq!(out.unwrap_err().to_string(), STILL_RUNNING);
    assert!(fs.exists("/m/.git"));
    assert_eq!(notices.count(), 0);
    assert_eq!(sessions(&store), ["dead"]);
    // Once a kill finds none left, the scan that moves runs and the record goes.
    let (out, notices) = launch(&store, &fs, false);
    out.unwrap();
    assert!(!fs.exists("/m/.git"));
    assert_eq!(notices.count(), 1);
    assert_eq!(sessions(&store), Vec::<String>::new());
}

#[test]
fn a_session_that_outlived_its_kill_is_marked_and_scanned_once_none_is_left() {
    let fs = FakeFs::new();
    let store = launched(&["a", "b"]);
    fs.repo("/m/.git");
    state::outlived(&store, "a", &[PathBuf::from("/m/.git")]);
    state::end(&store, "b", Vec::new());
    assert_eq!(sessions(&store), ["a"]);
    let marked = store.0.lock().unwrap().sessions[0].clone();
    assert!(marked.outlived);
    assert_eq!(marked.finds, [Bytes::from(&PathBuf::from("/m/.git"))]);
    // Its watch may still be on the host's register: a marked record counts as dead anyway.
    assert!(host::dead(&marked));
    let (out, _) = launch(&store, &fs, true);
    let error = out.unwrap_err().to_string();
    assert_eq!(
        error,
        format!(
            "{STILL_RUNNING} That session left 1 item(s) in the matter for Clauseo to set aside \
             once it has stopped."
        )
    );
    assert!(fs.exists("/m/.git"));
    let (scanned, _) = start_up(&store, &fs, &["a"], false);
    assert_eq!(scanned, 1);
    assert!(!fs.exists("/m/.git"));
    assert_eq!(sessions(&store), Vec::<String>::new());
    let (out, _) = launch(&store, &fs, false);
    out.unwrap();
}

#[test]
fn a_dead_session_of_a_missing_matter_waits_for_it() {
    let fs = FakeFs::new();
    let store = launched(&["dead"]);
    fs.remove("/m");
    let (scanned, _) = start_up(&store, &fs, &["dead"], false);
    assert_eq!(scanned, 0);
    assert_eq!(sessions(&store), ["dead"]);
}

#[test]
fn a_session_record_keeps_how_its_folder_was_watched_and_reads_live_without_it() {
    let mut record = record_of("clauseo-mode");
    record.mode = Mode::AfterOnly;
    let text = serde_json::to_string(&record).unwrap();
    assert!(text.contains(r#""mode":"after_only""#), "{text}");
    let back: SessionRecord = serde_json::from_str(&text).unwrap();
    assert_eq!(back.mode, Mode::AfterOnly);
    let mut old: serde_json::Value = serde_json::from_str(&text).unwrap();
    old.as_object_mut().unwrap().remove("mode");
    let old: SessionRecord = serde_json::from_value(old).unwrap();
    assert_eq!(old.mode, Mode::Live, "a record from before reads as live");
}
