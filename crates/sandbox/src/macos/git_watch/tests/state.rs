//! The matter's state: an item a final scan left blocks launches while it is there, a dead
//! session's final scan runs at the next launch, and a session's end removes its record.

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
    let kill = |name: &str| killed.borrow_mut().push(name.to_owned());
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
            ..MatterState::default()
        }
    );
}
