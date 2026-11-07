//! The chooser against a fake machine: where the page can go (indices, up, typed paths), the
//! limits (time, threads, entries, listings, idle), and that choosing issues a ref only through
//! a confirm and the bridge's own `pick_folder`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use clauseo_protocol::ErrorCode;

use super::listing::{Drives, Entry, LISTING_MAX, OUTSTANDING_MAX, bounded};
use super::place::{Place, TYPED_MAX};
use super::*;

mod choose;
mod fast;
mod typed;

const HOME: &str = r"C:\Users\rmehra";

/// A machine of folders named in a map: drives C and D, Z a network drive, the server `SRV` in
/// use, a Desktop under home, the link `C:\Users\All Users` and the hidden `AppData` under home,
/// and `C:\Slow`, whose read waits on `gate` (up to 30 s) until the test lets it go; with
/// `slow_drives`, so does reading the drives.
struct Fake {
    tree: HashMap<String, Result<Vec<Entry>, std::io::ErrorKind>>,
    desktop: String,
    servers: Vec<String>,
    slow_drives: bool,
    gate: Mutex<mpsc::Receiver<()>>,
}

impl Fake {
    fn wait(&self) {
        let _released = self
            .gate
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(30));
    }
}

impl Machine for Fake {
    fn drives(&self) -> Drives {
        if self.slow_drives {
            self.wait();
        }
        Drives {
            local: vec!['C', 'D'],
            remote: vec!['Z'],
            servers: self.servers.clone(),
            desktop: Some(self.desktop.clone()),
        }
    }
    fn read(&self, folder: &str) -> std::io::Result<Vec<Entry>> {
        if folder == r"C:\Slow" {
            self.wait();
        }
        self.tree
            .get(&folder.to_lowercase())
            .cloned()
            .unwrap_or(Err(std::io::ErrorKind::NotFound))
            .map_err(std::io::Error::from)
    }
    fn entry(&self, path: &str) -> Option<Entry> {
        let path = path.to_lowercase();
        let link = path == r"c:\users\all users";
        let hidden = path == r"c:\users\rmehra\appdata";
        let known = matches!(self.tree.get(&path), Some(Ok(_)));
        (link || hidden || known).then(|| Entry {
            hidden,
            link,
            ..dir("")
        })
    }
}

fn dir(name: &str) -> Entry {
    Entry {
        name: name.to_owned(),
        dir: true,
        hidden: false,
        link: false,
    }
}

/// A chooser on the fake machine, and the sender that lets `C:\Slow`'s reads go.
fn fake_chooser(desktop: &str) -> (Chooser, mpsc::Sender<()>) {
    fake_with(desktop, &["SRV"], false)
}

/// [`fake_chooser`] with the servers in use, and whether reading the drives hangs.
fn fake_with(desktop: &str, servers: &[&str], slow_drives: bool) -> (Chooser, mpsc::Sender<()>) {
    let (release, gate) = mpsc::channel();
    let mut tree = HashMap::new();
    let mut put = |path: &str, names: &[&str]| {
        let entries = names.iter().map(|name| dir(name)).collect();
        tree.insert(path.to_lowercase(), Ok(entries));
    };
    put(r"C:\", &["Cases", "Users", "Windows"]);
    put(r"C:\Cases", &["Acme", "Beta"]);
    put(r"C:\Cases\Acme", &[]);
    put(r"C:\Users", &["All Users", "rmehra"]);
    put(HOME, &["Desktop", "Work"]);
    put(r"C:\Users\rmehra\Work", &[]);
    put(r"C:\Users\rmehra\AppData\Local", &[]);
    put(r"C:\Users\All Users\Data", &[]);
    put(r"\\srv\share", &["Team"]);
    put(r"\\srv\share\Team", &[]);
    put(r"Z:\", &["Mapped"]);
    tree.insert(
        r"c:\locked".to_owned(),
        Err(std::io::ErrorKind::PermissionDenied),
    );
    let fake = Fake {
        tree,
        desktop: desktop.to_owned(),
        servers: servers.iter().map(|s| (*s).to_owned()).collect(),
        slow_drives,
        gate: Mutex::new(gate),
    };
    (Chooser::with(Arc::new(fake), None), release)
}

/// A session and its start points.
struct Opened {
    chooser: u32,
    starts: Vec<StartPoint>,
}

/// A session opened at `now`, with `C:\Windows` refused, and its start points.
fn open(chooser: &Chooser, now: Instant) -> Opened {
    let refused = [Path::new(r"C:\Windows").to_path_buf()];
    let last = Path::new(r"C:\Cases\Acme");
    let id = chooser
        .open(Some(Path::new(HOME)), Some(last), &refused, now)
        .unwrap()
        .chooser;
    Opened {
        chooser: id,
        starts: chooser.starts(id, now).unwrap(),
    }
}

fn key(start: &Opened, kind: StartKind) -> u8 {
    start.starts.iter().find(|p| p.kind == kind).unwrap().key
}

fn names(listing: &Listing) -> Vec<&str> {
    match &listing.state {
        ListState::Folders { items, .. } => items.iter().map(|i| i.name.as_str()).collect(),
        other => panic!("not folders: {other:?}"),
    }
}

fn code(answer: Result<Listing, HostError>) -> ErrorCode {
    answer.unwrap_err().code()
}

/// The start points are the local drives, home (shown empty), the Desktop (shown relative to
/// home) and the last pick's folder: never a network drive or a network Desktop.
#[test]
fn the_start_points_are_local_and_never_show_the_user_name() {
    let (chooser, _release) = fake_chooser(r"C:\Users\rmehra\Desktop");
    let start = open(&chooser, crate::open::now());
    let shown: Vec<(StartKind, &str)> = start
        .starts
        .iter()
        .map(|p| (p.kind, p.display.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            (StartKind::Drive, "C:"),
            (StartKind::Drive, "D:"),
            (StartKind::Home, ""),
            (StartKind::Desktop, "Desktop"),
            (StartKind::LastPick, r"C:\Cases"),
        ]
    );
    let keys: Vec<u8> = start.starts.iter().map(|p| p.key).collect();
    assert_eq!(keys, [0, 1, 2, 3, 4]);
    let (network, _release) = fake_chooser(r"\\srv\share\Desktop");
    let start = open(&network, crate::open::now());
    assert!(start.starts.iter().all(|p| p.kind != StartKind::Desktop));
    let home = network.list(
        start.chooser,
        Go::Start {
            key: key(&start, StartKind::Home),
        },
        crate::open::now(),
    );
    let home = home.unwrap();
    assert_eq!(
        (home.display.as_str(), home.crumbs.as_slice()),
        ("", &[String::new()][..])
    );
}

/// `Into` takes only an index into the last listing; past it, or before any listing, it is
/// `command.invalid`. A start key it did not give is too.
#[test]
fn into_takes_only_an_index_of_the_last_listing() {
    let (chooser, _release) = fake_chooser(r"C:\Users\rmehra\Desktop");
    let now = crate::open::now();
    let start = open(&chooser, now);
    let id = start.chooser;
    assert_eq!(
        code(chooser.list(id, Go::Into { index: 0 }, now)),
        ErrorCode::CommandInvalid
    );
    assert_eq!(
        code(chooser.list(id, Go::Start { key: 9 }, now)),
        ErrorCode::CommandInvalid
    );
    let home = chooser
        .list(
            id,
            Go::Start {
                key: key(&start, StartKind::Home),
            },
            now,
        )
        .unwrap();
    assert_eq!(names(&home), ["Desktop", "Work"]);
    let work = chooser.list(id, Go::Into { index: 1 }, now).unwrap();
    assert_eq!((work.display.as_str(), work.crumbs.len()), ("Work", 2));
    assert_eq!(
        code(chooser.list(id, Go::Into { index: 0 }, now)),
        ErrorCode::CommandInvalid
    );
    let c = chooser.list(id, Go::Start { key: 0 }, now).unwrap();
    assert_eq!(names(&c), ["Cases", "Users", "Windows"]);
    assert_eq!(
        code(chooser.list(id, Go::Into { index: 3 }, now)),
        ErrorCode::CommandInvalid
    );
    let cases = chooser.list(id, Go::Into { index: 0 }, now).unwrap();
    assert_eq!(
        (cases.display.as_str(), cases.crumbs.clone()),
        (r"C:\Cases", vec!["C:".to_owned(), "Cases".to_owned()])
    );
}

/// A typed path is a drive or share path with no way out: `..`, `\\?\`, `\\.\`, relative and
/// drive-relative paths, alternate streams, trimmed names and overlong paths are refused.
#[test]
fn a_typed_path_cannot_climb_or_escape() {
    for refused in [
        r"C:\Cases\..\Windows",
        r"C:\Cases\..",
        r"\\?\C:\Windows",
        r"\\.\PhysicalDrive0",
        r"//?/C:/Windows",
        r"Cases\Acme",
        r"C:Cases",
        r"C:",
        r"C:\Cases\Acme:secret",
        r"C:\Cases\.",
        r"C:\Cases\Acme.",
        r"C:\Cases\Acme ",
        r"\\srv",
        r"\\srv\\share",
        r"C:\a*b",
        "",
    ] {
        assert_eq!(Place::typed(refused), None, "{refused}");
    }
    let long = format!(r"C:\{}", "a".repeat(TYPED_MAX - 2));
    assert!(Place::typed(&long).is_none());
    assert!(Place::typed(&long[..TYPED_MAX]).is_some());
    assert_eq!(
        Place::typed("c:/Cases//Acme/").unwrap().text(),
        r"C:\Cases\Acme"
    );
    assert_eq!(Place::typed(r"\\srv\share").unwrap().text(), r"\\srv\share");

    let (chooser, _release) = fake_chooser(r"C:\Users\rmehra\Desktop");
    let now = crate::open::now();
    let id = open(&chooser, now).chooser;
    for path in [r"C:\Cases\..\Windows", r"\\?\C:\Windows"] {
        let typed = chooser.list(
            id,
            Go::Typed {
                path: path.to_owned(),
            },
            now,
        );
        assert_eq!(code(typed), ErrorCode::CommandInvalid, "{path}");
    }
    let typed = chooser.list(
        id,
        Go::Typed {
            path: r"C:\Cases".to_owned(),
        },
        now,
    );
    assert_eq!(names(&typed.unwrap()), ["Acme", "Beta"]);
    let typed = chooser
        .list(
            id,
            Go::Typed {
                path: r"C:\Nowhere".to_owned(),
            },
            now,
        )
        .unwrap();
    assert_eq!(typed.state, ListState::NotFound);
}

/// `Up` stops at a drive root and at a typed share's root; a network place is reachable only
/// under the share the lawyer typed, and a typed network drive counts as typed.
#[test]
fn up_stops_at_the_root_and_the_network_needs_a_typed_share() {
    let (chooser, _release) = fake_chooser(r"C:\Users\rmehra\Desktop");
    let now = crate::open::now();
    let id = open(&chooser, now).chooser;
    chooser
        .list(
            id,
            Go::Typed {
                path: r"C:\Cases\Acme".to_owned(),
            },
            now,
        )
        .unwrap();
    for _ in 0..3 {
        chooser.list(id, Go::Up, now).unwrap();
    }
    assert_eq!(chooser.current(id, now).unwrap(), r"C:\");

    let share = chooser
        .list(
            id,
            Go::Typed {
                path: r"\\srv\share\Team".to_owned(),
            },
            now,
        )
        .unwrap();
    assert_eq!(share.crumbs, [r"\\srv\share", "Team"]);
    for _ in 0..3 {
        chooser.list(id, Go::Up, now).unwrap();
    }
    assert_eq!(chooser.current(id, now).unwrap(), r"\\srv\share");
    let team = chooser.list(id, Go::Into { index: 0 }, now).unwrap();
    assert_eq!(team.display, r"\\srv\share\Team");

    let mapped = chooser
        .list(
            id,
            Go::Typed {
                path: r"Z:\".to_owned(),
            },
            now,
        )
        .unwrap();
    assert_eq!(names(&mapped), ["Mapped"]);
    // The share typed before is no longer the one the session may reach.
    chooser
        .list(
            id,
            Go::Typed {
                path: r"C:\".to_owned(),
            },
            now,
        )
        .unwrap();
    assert_eq!(chooser.current(id, now).unwrap(), r"C:\");
}

/// A listing shows at most 2,000 folders, sorted ignoring case, with `more`; files, hidden and
/// system folders and links are left out; a refused folder is marked and does not open, and
/// the folder shown stays.
#[test]
fn a_listing_caps_sorts_skips_and_marks() {
    let mut entries: Vec<Entry> = (0..2500).map(|n| dir(&format!("Matter {n:04}"))).collect();
    entries.push(dir("alpha"));
    entries.push(dir("Windows"));
    entries.push(Entry {
        dir: false,
        ..dir("a file.docx")
    });
    entries.push(Entry {
        hidden: true,
        ..dir("AppData")
    });
    entries.push(Entry {
        link: true,
        ..dir("Application Data")
    });
    entries.push(dir(".."));
    let (names, more) = folders(entries.clone());
    assert_eq!(names.len(), LISTING_MAX);
    assert!(more);
    assert_eq!(names.first().map(String::as_str), Some("alpha"));
    assert!(
        !names
            .iter()
            .any(|n| ["a file.docx", "AppData", "Application Data", ".."].contains(&n.as_str()))
    );
    let (few, more) = folders(vec![dir("b"), dir("C"), dir("a")]);
    assert_eq!(
        (few, more),
        (vec!["a".to_owned(), "b".to_owned(), "C".to_owned()], false)
    );

    let (chooser, _release) = fake_chooser(r"C:\Users\rmehra\Desktop");
    let now = crate::open::now();
    let id = open(&chooser, now).chooser;
    let root = chooser.list(id, Go::Start { key: 0 }, now).unwrap();
    let ListState::Folders { items, more } = &root.state else {
        panic!("{root:?}")
    };
    assert!(!more);
    let refused: Vec<&str> = items
        .iter()
        .filter(|i| i.refused)
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(refused, ["Windows"]);
    let windows = chooser.list(id, Go::Into { index: 2 }, now).unwrap();
    assert_eq!(
        (windows.state, windows.display.as_str()),
        (ListState::NotAllowed, r"C:\")
    );
    let typed = chooser
        .list(
            id,
            Go::Typed {
                path: r"c:\windows".to_owned(),
            },
            now,
        )
        .unwrap();
    assert_eq!(typed.state, ListState::NotAllowed);
    // A folder Windows refuses to read does not open either.
    let locked = Go::Typed {
        path: r"C:\Locked".to_owned(),
    };
    let locked = chooser.list(id, locked, now).unwrap();
    assert_eq!(locked.state, ListState::NotAllowed);
    assert_eq!(chooser.current(id, now).unwrap(), r"C:\");
}

/// A read that hangs answers `no_answer` after about 2 s and the folder becomes the one shown,
/// so `Again` reads it again. Past four reads still running, a read answers at once without a
/// thread.
#[test]
fn a_hanging_read_answers_no_answer_in_about_two_seconds() {
    let (chooser, release) = fake_chooser(r"C:\Users\rmehra\Desktop");
    let now = crate::open::now();
    let id = open(&chooser, now).chooser;
    let started = crate::open::now();
    let slow = chooser
        .list(
            id,
            Go::Typed {
                path: r"C:\Slow".to_owned(),
            },
            now,
        )
        .unwrap();
    let waited = started.elapsed();
    assert_eq!(slow.state, ListState::NoAnswer);
    assert!(
        waited >= Duration::from_millis(1900) && waited < Duration::from_secs(5),
        "{waited:?}"
    );
    assert_eq!(chooser.current(id, now).unwrap(), r"C:\Slow");
    drop(release);
    let again = chooser.list(id, Go::Again, now).unwrap();
    assert_eq!(again.state, ListState::NotFound);

    let outstanding = Arc::new(AtomicUsize::new(0));
    let (hold, held) = mpsc::channel::<()>();
    let held = Arc::new(Mutex::new(held));
    for _ in 0..OUTSTANDING_MAX {
        let held = Arc::clone(&held);
        let answer = bounded(&outstanding, Duration::from_millis(10), move || {
            let _released = held.lock().unwrap().recv_timeout(Duration::from_secs(30));
        });
        assert_eq!(answer, None);
    }
    let ran = Arc::new(AtomicBool::new(false));
    let marker = Arc::clone(&ran);
    let started = crate::open::now();
    let answer = bounded(&outstanding, Duration::from_secs(2), move || {
        marker.store(true, Ordering::SeqCst);
    });
    assert_eq!(answer, None);
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(!ran.load(Ordering::SeqCst));
    drop(hold);
}

/// One session at a time: a new one closes the old; it expires after 10 minutes idle, closes on
/// cancel, and gives at most 300 listings.
#[test]
fn sessions_are_replaced_expire_and_are_bounded() {
    let (chooser, _release) = fake_chooser(r"C:\Users\rmehra\Desktop");
    let now = crate::open::now();
    let first = open(&chooser, now).chooser;
    let second = open(&chooser, now + Duration::from_millis(1)).chooser;
    assert_ne!(first, second);
    assert_eq!(
        code(chooser.list(first, Go::Start { key: 0 }, now)),
        ErrorCode::CommandInvalid
    );
    chooser.list(second, Go::Start { key: 0 }, now).unwrap();
    let later = now + IDLE_MAX / 2;
    chooser.list(second, Go::Again, later).unwrap();
    let idle = later + IDLE_MAX + Duration::from_secs(1);
    assert_eq!(
        code(chooser.list(second, Go::Again, idle)),
        ErrorCode::CommandInvalid
    );

    let third = open(&chooser, idle).chooser;
    assert!(chooser.close(third).is_ok());
    assert!(chooser.close(third).is_err());
    assert_eq!(
        code(chooser.list(third, Go::Start { key: 0 }, idle)),
        ErrorCode::CommandInvalid
    );

    let fourth = open(&chooser, idle).chooser;
    for _ in 0..LISTINGS_MAX {
        chooser.list(fourth, Go::Start { key: 0 }, idle).unwrap();
    }
    let over = chooser.list(fourth, Go::Start { key: 0 }, idle);
    assert_eq!(code(over), ErrorCode::TransportUnauthorized);
}

/// Off Windows the pick never opens the chooser, whatever the variable or the pre-check says.
#[cfg(not(windows))]
#[test]
fn off_windows_the_pick_never_chooses() {
    for setting in [Some("app"), Some("native"), None] {
        assert_eq!(route(false, cfg!(windows), setting, || true), Route::Native);
    }
    assert!(!Chooser::system().would_stall());
}
