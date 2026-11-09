//! The ruling's pairing of a folder made just now with its file ID leaving another watched
//! place: a forged creation time doesn't hide a folder moved in from the agent's temp folder,
//! whichever watch reads its record first; a `mkdir` whose file ID left nowhere stays
//! uncounted, as does a folder's own leaving from where it was made.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::super::pairs::{PAIR_MAX, PAIR_WAIT, PAIR_WINDOW, Paired, Pairs, promote};
use super::super::{Changes, GONE_LIMIT, Guard, Next, Pulse, Session, feed, watch};
use super::{Log, script};
use crate::windows::denial_pipe::Record;
use crate::windows::set_aside::{Failed, Hooks, Live};

/// A session that shares `paired`, says its pauses, and on its one settle runs `meanwhile`
/// (the other watch reading its record).
struct Pairing<'a> {
    log: &'a Log,
    paired: Option<Paired>,
    meanwhile: RefCell<Option<Box<dyn FnOnce() + 'a>>>,
    pulse: Pulse,
}

impl Session for Pairing<'_> {
    fn end(&self) -> bool {
        self.log.push("end");
        true
    }
    fn say(&self, _line: &str) {}
    fn record(&self, record: &Record) {
        if let Record::Notice { text } = record {
            self.log.push(text.clone());
        }
    }
    fn pause(&self, _wait: Duration) -> bool {
        true
    }
    fn settle(&self, wait: Duration) {
        assert_eq!(wait, PAIR_WAIT);
        self.log.push("settle");
        if let Some(meanwhile) = self.meanwhile.borrow_mut().take() {
            meanwhile();
        }
    }
    fn now(&self) -> Duration {
        Duration::from_secs(100)
    }
    fn pulse(&self) -> &Pulse {
        &self.pulse
    }
    fn paired(&self) -> Option<&Paired> {
        self.paired.as_ref()
    }
}

const NOW: Duration = Duration::from_secs(100);

fn matter(pairs: &Arc<Pairs>) -> Paired {
    Paired {
        pairs: Arc::clone(pairs),
        root: PathBuf::from("matter"),
    }
}

fn session<'a>(log: &'a Log, pairs: &Arc<Pairs>) -> Pairing<'a> {
    Pairing {
        log,
        paired: Some(matter(pairs)),
        meanwhile: RefCell::new(None),
        pulse: Pulse::default(),
    }
}

/// `x`, made just now with file ID 7, gone before its scan.
fn gone_fresh() -> (Live, Vec<(PathBuf, u64)>) {
    let live = Live {
        unpaired: vec![PathBuf::from("x")],
        ..Live::default()
    };
    (live, vec![(PathBuf::from("x"), 7)])
}

/// A forged creation time: the folder looks made just now, but its file ID left the agent's
/// temp folder: it counts gone, with no wait.
#[test]
fn a_fresh_folder_whose_file_id_left_the_agents_temp_counts_gone() {
    let (log, pairs) = (Log::default(), Arc::new(Pairs::default()));
    pairs.note(NOW, Path::new("temp"), &[(PathBuf::from("x"), 7)]);
    let (mut live, fresh) = gone_fresh();
    promote(
        &mut live,
        &fresh,
        Some(&matter(&pairs)),
        &session(&log, &pairs),
    );
    assert_eq!(live.gone, [PathBuf::from("x")]);
    assert_eq!(live.unpaired, Vec::<PathBuf>::new());
    assert_eq!(log.lines(), Vec::<String>::new());
}

/// A `mkdir` whose file ID left nowhere: one wait for the other watches, then uncounted.
#[test]
fn a_fresh_folder_whose_file_id_left_nowhere_doesnt_count() {
    let (log, pairs) = (Log::default(), Arc::new(Pairs::default()));
    pairs.note(NOW, Path::new("temp"), &[(PathBuf::from("other"), 8)]);
    let (mut live, fresh) = gone_fresh();
    promote(
        &mut live,
        &fresh,
        Some(&matter(&pairs)),
        &session(&log, &pairs),
    );
    assert_eq!(live.gone, Vec::<PathBuf>::new());
    assert_eq!(live.unpaired, Vec::<PathBuf>::new());
    assert_eq!(log.lines(), ["settle"]);

    // With no pairing at all, none counts and nothing waits.
    let (mut live, fresh) = gone_fresh();
    promote(&mut live, &fresh, None, &session(&log, &pairs));
    assert_eq!(live.gone, Vec::<PathBuf>::new());
    assert_eq!(log.lines(), ["settle"]);
}

/// The temp folder's watch reads its record after the matter's check: the one wait pairs it.
#[test]
fn a_file_id_noted_as_leaving_after_the_check_still_pairs() {
    let (log, pairs) = (Log::default(), Arc::new(Pairs::default()));
    let late = session(&log, &pairs);
    let temp = Arc::clone(&pairs);
    *late.meanwhile.borrow_mut() = Some(Box::new(move || {
        temp.note(NOW, Path::new("temp"), &[(PathBuf::from("x"), 7)]);
    }));
    let (mut live, fresh) = gone_fresh();
    promote(&mut live, &fresh, Some(&matter(&pairs)), &late);
    assert_eq!(live.gone, [PathBuf::from("x")]);
    assert_eq!(log.lines(), ["settle"]);
}

/// A folder's own leaving from where it was made (`rm -r tmp`) never pairs; leaving another
/// folder of the matter does.
#[test]
fn a_folders_own_leaving_doesnt_pair_but_another_place_in_the_matter_does() {
    let pairs = Pairs::default();
    pairs.note(NOW, Path::new("matter"), &[(PathBuf::from("x"), 7)]);
    assert!(!pairs.paired(NOW, 7, &Path::new("matter").join("x")));
    pairs.note(
        NOW,
        Path::new("matter"),
        &[(PathBuf::from("a").join("x"), 7)],
    );
    assert!(pairs.paired(NOW, 7, &Path::new("matter").join("x")));
}

/// What left more than [`PAIR_WINDOW`] ago, or was pushed out past [`PAIR_MAX`], pairs no more.
#[test]
fn a_file_id_pairs_only_within_the_window_and_the_newest_pair_max() {
    let pairs = Pairs::default();
    let temp = Path::new("temp");
    pairs.note(NOW, temp, &[(PathBuf::from("x"), 7)]);
    let x = Path::new("matter").join("x");
    assert!(pairs.paired(NOW + PAIR_WINDOW, 7, &x));
    assert!(!pairs.paired(NOW + PAIR_WINDOW + Duration::from_millis(1), 7, &x));

    let flood: Vec<(PathBuf, u64)> = (0..PAIR_MAX as u64)
        .map(|id| (PathBuf::from(format!("f{id}")), 1000 + id))
        .collect();
    pairs.note(NOW, temp, &flood);
    assert!(!pairs.paired(NOW, 7, &x), "the oldest went first");
    assert!(pairs.paired(NOW, 1000, &x));
}

/// A guard whose fresh folders are all gone by their check.
struct AllGone;

impl Guard for AllGone {
    fn changed(&self, changes: &Changes, _hooks: &impl Hooks) -> Live {
        Live {
            unpaired: changes.fresh_paths(),
            gone: changes.moved_folders.clone(),
            ..Live::default()
        }
    }
    fn rescan(&self, _hooks: &impl Hooks) -> Live {
        Live::default()
    }
    fn retry(&self, _failed: &[Failed], _hooks: &impl Hooks) -> Live {
        Live::default()
    }
}

fn fresh(name: &str, id: u64) -> Next {
    Next::Changed(Changes {
        added: vec![PathBuf::from(name)],
        fresh_folders: vec![(PathBuf::from(name), id)],
        ..Changes::default()
    })
}

/// In the loop: forged folders moved in from temp, each gone, end the session at
/// [`GONE_LIMIT`]; as many `mkdir`s gone end nothing; one leaving another folder of the matter
/// in the same report pairs too; and one renamed in counts as it always did.
#[test]
fn forged_folders_moved_in_and_gone_end_the_session_and_mkdirs_dont() {
    let (log, pairs) = (Log::default(), Arc::new(Pairs::default()));
    let ids: Vec<(PathBuf, u64)> = (1..=3)
        .map(|id| (PathBuf::from(format!("t{id}")), id))
        .collect();
    pairs.note(NOW, Path::new("temp"), &ids);
    let reports = (1..=3).map(|id| fresh(&format!("m{id}"), id)).collect();
    watch(script(reports), &AllGone, &session(&log, &pairs));
    assert!(log.lines().contains(&"end".to_owned()), "{:?}", log.lines());

    let (log, pairs) = (Log::default(), Arc::new(Pairs::default()));
    let reports = (1..=u64::from(GONE_LIMIT) + 1)
        .map(|id| fresh(&format!("tmp{id}"), 50 + id))
        .collect();
    watch(script(reports), &AllGone, &session(&log, &pairs));
    assert!(
        !log.lines().contains(&"end".to_owned()),
        "{:?}",
        log.lines()
    );

    let (log, pairs) = (Log::default(), Arc::new(Pairs::default()));
    let within = |id: u64| {
        Next::Changed(Changes {
            added: vec![PathBuf::from("b/x")],
            fresh_folders: vec![(PathBuf::from("b/x"), id)],
            left: vec![(PathBuf::from("a/x"), id)],
            ..Changes::default()
        })
    };
    let renamed = Next::Changed(Changes {
        added: vec![PathBuf::from("r")],
        moved_folders: vec![PathBuf::from("r")],
        ..Changes::default()
    });
    watch(
        script(vec![within(1), within(2), renamed]),
        &AllGone,
        &session(&log, &pairs),
    );
    assert!(log.lines().contains(&"end".to_owned()), "{:?}", log.lines());
}

/// A pairing watch (state, temp) only notes what left: past an overflow, until it breaks.
#[test]
fn a_pairing_watch_notes_what_left_and_ends_quietly() {
    let pairs = Arc::new(Pairs::default());
    let temp = Paired {
        pairs: Arc::clone(&pairs),
        root: PathBuf::from("temp"),
    };
    let left = |name: &str, id: u64| {
        Next::Changed(Changes {
            left: vec![(PathBuf::from(name), id)],
            ..Changes::default()
        })
    };
    let reports = vec![
        left("a", 1),
        Next::Overflow,
        left("b", 2),
        Next::Broken(std::io::Error::from_raw_os_error(6)),
        left("c", 3),
    ];
    feed(script(reports), &temp, || NOW);
    let x = Path::new("matter").join("x");
    assert!(pairs.paired(NOW, 1, &x) && pairs.paired(NOW, 2, &x));
    assert!(!pairs.paired(NOW, 3, &x), "nothing after it broke");
}
