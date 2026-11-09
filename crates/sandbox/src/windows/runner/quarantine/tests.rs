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
    clear_fails: bool,
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
    fn clear(&mut self) -> io::Result<()> {
        self.events.borrow_mut().push("clear");
        if self.clear_fails {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        self.unclean = false;
        Ok(())
    }
}

fn admitted(
    unclean: bool,
    clean_sweep: bool,
    clear_fails: bool,
) -> (Result<u32, Quarantined>, Vec<&'static str>, bool) {
    let events = std::cell::RefCell::new(Vec::new());
    let mut slot = Fake {
        events: &events,
        unclean,
        clean_sweep,
        clear_fails,
    };
    let got = admit(&mut slot, &mut |_| {}, || {
        // The launch: its pipes, window station and desktop name the pool user.
        events.borrow_mut().push("grant");
        7
    });
    let left = slot.unclean;
    (got, events.into_inner(), left)
}

#[test]
fn a_clean_slot_launches_with_no_verify_pass() {
    let (got, events, _) = admitted(false, true, false);
    assert_eq!(got, Ok(7));
    assert_eq!(events, ["look", "grant"]);
}

#[test]
fn a_marked_slot_gets_no_grant_before_a_clean_sweep_which_clears_the_mark() {
    let (got, events, left) = admitted(true, true, false);
    assert_eq!(got, Ok(7));
    assert_eq!(events, ["look", "verify", "clear", "grant"]);
    assert!(!left, "the mark is cleared");
    // A mark that can't be removed: the slot was verified, so the launch goes on, and the next
    // launch verifies again.
    let (got, events, left) = admitted(true, true, true);
    assert_eq!(got, Ok(7));
    assert_eq!(events, ["look", "verify", "clear", "grant"]);
    assert!(left);
}

#[test]
fn a_marked_slot_whose_sweep_fails_is_refused_with_no_grant_and_stays_marked() {
    let (got, events, left) = admitted(true, false, false);
    assert_eq!(
        got,
        Err(Quarantined {
            why: "the verify pass exited 0xE0C1500F".to_owned()
        })
    );
    assert_eq!(events, ["look", "verify"]);
    assert!(left, "the mark stays until a fresh sweep verifies clean");
    assert!(RESTART.contains("restart the computer"));
}
