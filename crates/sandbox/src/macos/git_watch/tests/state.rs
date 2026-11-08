//! The matter's state: an item a final scan left blocks launches while it is there, a dead
//! session's final scan runs at the next launch or the host's next start, and a session's end
//! removes its record unless a process of it may be left.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;

use super::super::aside::Stuck;
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
    }
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
    let kill = |name: &str| {
        killed.borrow_mut().push(name.to_owned());
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
    state::end(&store, "s", stuck, false);
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
    let kill = |_: &str| !left;
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

#[test]
fn a_dead_session_with_a_process_left_keeps_its_record_after_its_scan() {
    let fs = FakeFs::new();
    let store = launched(&["dead"]);
    fs.repo("/m/.git");
    let (scanned, _) = start_up(&store, &fs, &["dead"], true);
    assert_eq!(scanned, 1, "scanned anyway");
    assert!(!fs.exists("/m/.git"));
    assert_eq!(
        sessions(&store),
        ["dead"],
        "the next start or launch tries again"
    );
    // At a launch too.
    let killed = RefCell::new(Vec::new());
    let notices = Arc::new(Notices::default());
    let sink = Arc::clone(&notices) as Arc<dyn DenialSink>;
    let is_dead = |_: &SessionRecord| true;
    let kill = |name: &str| {
        killed.borrow_mut().push(name.to_owned());
        false
    };
    let host = Host {
        dead: &is_dead,
        kill: &kill,
    };
    state::begin(&store, &fs, &PathBuf::from("/m"), &sink, &host).unwrap();
    assert_eq!(killed.borrow().len(), 1);
    assert_eq!(sessions(&store), ["dead"]);
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
fn a_session_whose_kill_ran_out_keeps_its_record_at_its_end() {
    let store = launched(&["a", "b"]);
    state::end(&store, "a", Vec::new(), true);
    state::end(&store, "b", Vec::new(), false);
    assert_eq!(sessions(&store), ["a"]);
}
