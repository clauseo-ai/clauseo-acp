//! The unclean-slot mark on a scratch folder (its name, write, listing and removal), and the
//! launch's order on a marked slot: no grant before a verify-only pass that passed, which clears
//! the mark; a pass that failed refuses and keeps it.

use super::*;
use crate::test_support::TempDir;

const POOL: &str = "S-1-5-21-1111111111-2222222222-3333333333-1001";
const OTHER: &str = "S-1-5-21-1111111111-2222222222-3333333333-1002";

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

#[test]
fn the_mark_is_named_for_the_pool_user_in_the_run_folder_and_listed() {
    let data = TempDir::new("unclean-mark");
    data.dir("sandbox/run");
    assert_eq!(
        unclean_path(data.path(), &sid(POOL)),
        data.path()
            .join("sandbox")
            .join("run")
            .join(format!("{POOL}.unclean"))
    );
    assert!(!is_unclean(data.path(), &sid(POOL)));
    assert_eq!(unclean_users(data.path()).unwrap(), BTreeSet::new());
    let path = mark_unclean(data.path(), &sid(POOL)).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), POOL);
    assert!(is_unclean(data.path(), &sid(POOL)));
    assert!(!is_unclean(data.path(), &sid(OTHER)));
    // A session marker and a name that gives no SID aren't marks.
    data.file("sandbox/run/0123456789abcdef.session", "{}");
    data.file("sandbox/run/not-a-sid.unclean", "");
    assert_eq!(
        unclean_users(data.path()).unwrap(),
        BTreeSet::from([sid(POOL)])
    );
    // The desktop user's alone, as `check_owned_file` and `own_file` make it on Windows.
    #[cfg(unix)]
    crate::test_support::set_mode(&path, 0o600);
    clear_unclean(data.path(), &sid(POOL)).unwrap();
    assert!(!is_unclean(data.path(), &sid(POOL)));
    // Gone already: fine.
    clear_unclean(data.path(), &sid(POOL)).unwrap();
    // No run folder: no marks.
    let empty = TempDir::new("unclean-none");
    assert_eq!(unclean_users(empty.path()).unwrap(), BTreeSet::new());
}

/// A slot whose every step is recorded in one list with the launch's grants.
struct Fake<'a> {
    events: &'a std::cell::RefCell<Vec<&'static str>>,
    unclean: bool,
    clean_sweep: bool,
    mark_fails: bool,
}

impl Slot for Fake<'_> {
    fn unclean(&mut self) -> bool {
        self.events.borrow_mut().push("look");
        self.unclean
    }
    fn verify(&mut self) -> Result<(), String> {
        self.events.borrow_mut().push("verify");
        if self.clean_sweep {
            Ok(())
        } else {
            Err("the verify pass exited 0xE0C1500F".to_owned())
        }
    }
    fn mark(&mut self) -> io::Result<()> {
        self.events.borrow_mut().push("mark");
        if self.mark_fails {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        self.unclean = true;
        Ok(())
    }
    fn clear(&mut self) -> io::Result<()> {
        self.events.borrow_mut().push("clear");
        self.unclean = false;
        Ok(())
    }
}

fn fake<'a>(events: &'a std::cell::RefCell<Vec<&'static str>>, unclean: bool) -> Fake<'a> {
    Fake {
        events,
        unclean,
        clean_sweep: true,
        mark_fails: false,
    }
}

/// `admit` with a launch that records its grant; the result, and whether the slot is marked
/// after it.
fn admitted(slot: &mut Fake<'_>) -> (Result<u32, Refused>, bool) {
    let events = slot.events;
    let got = admit(slot, &mut |_| {}, || {
        // The launch: its pipes, window station and desktop name the pool user.
        events.borrow_mut().push("grant");
        7
    });
    (got, slot.unclean)
}

#[test]
fn a_clean_slot_is_marked_before_anything_of_the_session_runs() {
    let events = std::cell::RefCell::new(Vec::new());
    let (got, marked) = admitted(&mut fake(&events, false));
    assert_eq!(got, Ok(7));
    assert_eq!(*events.borrow(), ["look", "mark", "grant"]);
    assert!(marked, "marked until a clean end");
}

#[test]
fn a_marked_slot_gets_no_grant_before_a_clean_sweep_and_stays_marked_for_the_session() {
    let events = std::cell::RefCell::new(Vec::new());
    let (got, marked) = admitted(&mut fake(&events, true));
    assert_eq!(got, Ok(7));
    assert_eq!(*events.borrow(), ["look", "verify", "mark", "grant"]);
    assert!(marked);
}

#[test]
fn a_marked_slot_whose_sweep_fails_is_refused_with_no_grant_and_stays_marked() {
    let events = std::cell::RefCell::new(Vec::new());
    let mut slot = Fake {
        clean_sweep: false,
        ..fake(&events, true)
    };
    let (got, marked) = admitted(&mut slot);
    assert_eq!(
        got,
        Err(Refused::Quarantined(
            "the verify pass exited 0xE0C1500F".to_owned()
        ))
    );
    assert_eq!(*events.borrow(), ["look", "verify"]);
    assert!(marked, "the mark stays until a fresh sweep verifies clean");
    assert!(RESTART.contains("restart the computer"));
}

#[test]
fn a_mark_that_cant_be_written_refuses_the_launch() {
    let events = std::cell::RefCell::new(Vec::new());
    let mut slot = Fake {
        mark_fails: true,
        ..fake(&events, false)
    };
    let (got, _) = admitted(&mut slot);
    assert!(matches!(got, Err(Refused::Unmarked(_))), "{got:?}");
    assert_eq!(*events.borrow(), ["look", "mark"], "no grant");
}

/// The slot on disk, its verify pass passing.
struct OnDisk<'a> {
    data: &'a Path,
}

impl Slot for OnDisk<'_> {
    fn unclean(&mut self) -> bool {
        is_unclean(self.data, &sid(POOL))
    }
    fn verify(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn mark(&mut self) -> io::Result<()> {
        let path = mark_unclean(self.data, &sid(POOL))?;
        // The desktop user's alone, as `check_owned_file` and `own_file` make it on Windows.
        #[cfg(unix)]
        crate::test_support::set_mode(&path, 0o600);
        #[cfg(not(unix))]
        let _ = path;
        Ok(())
    }
    fn clear(&mut self) -> io::Result<()> {
        clear_unclean(self.data, &sid(POOL))
    }
}

#[test]
fn a_killed_outer_leaves_the_mark_and_only_a_clean_end_clears_it() {
    let data = TempDir::new("unclean-ends");
    data.dir("sandbox/run");
    let mut slot = OnDisk { data: data.path() };
    // The session runs with the mark on disk: an outer killed in it leaves the mark.
    let seen = admit(&mut slot, &mut |_| {}, || {
        is_unclean(data.path(), &sid(POOL))
    });
    assert_eq!(seen, Ok(true));
    assert!(is_unclean(data.path(), &sid(POOL)), "killed: never ended");
    // An end that isn't clean (a sweep out of rounds or time, a survivor, a failure) leaves it.
    ended(&mut slot, false, &mut |_| {});
    assert!(is_unclean(data.path(), &sid(POOL)));
    // The next launch verifies, and its clean end clears the mark.
    let mut lines = Vec::new();
    admit(&mut slot, &mut |l| lines.push(l.to_owned()), || ()).unwrap();
    assert!(lines.iter().any(|l| l.contains("verify pass")), "{lines:?}");
    ended(&mut slot, true, &mut |_| {});
    assert!(!is_unclean(data.path(), &sid(POOL)));
}
