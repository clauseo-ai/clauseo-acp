//! What a typed path may reach (security review round 1): shares only on servers this machine
//! already uses, no link, hidden or system folder on the way, no device name; and how many
//! sessions page script can open.

use super::*;

/// A typed path whose folder before the last cannot be read does not open; a missing last
/// folder is `not_found`.
#[test]
fn a_typed_path_through_an_unreadable_folder_does_not_open() {
    let (chooser, _release) = fake_chooser(r"C:\Users\rmehra\Desktop");
    let id = open(&chooser, crate::open::now()).chooser;
    let ghost = go(&chooser, id, r"C:\Users\Ghost\Work").unwrap();
    assert_eq!(ghost.state, ListState::NotAllowed);
    let missing = go(&chooser, id, r"C:\Users\Ghost").unwrap();
    assert_eq!(missing.state, ListState::NotFound);
}

/// Lists the typed `path` in session `id` of `chooser`.
fn go(chooser: &Chooser, id: u32, path: &str) -> Result<Listing, HostError> {
    let path = path.to_owned();
    chooser.list(id, Go::Typed { path }, crate::open::now())
}

/// A typed share on an outside server, through `WebDAV`, with data in its name, on a loopback or
/// an administrative share is refused; a share on a server the machine uses opens, whatever the
/// case of its name.
#[test]
fn a_typed_share_opens_only_on_a_server_the_machine_uses() {
    let (chooser, _release) = fake_chooser(r"C:\Users\rmehra\Desktop");
    let id = open(&chooser, crate::open::now()).chooser;
    for attack in [
        r"\\evil.com\x",
        r"\\evil@SSL@443\x",
        r"\\evil@443\x",
        r"\\c2hhcmVk.evil.com\x",
        r"\\localhost\c$",
        r"\\localhost\x",
        r"\\127.0.0.1\x",
        r"\\127.1.2.3\x",
        r"\\srv\c$",
        r"\\srv\admin$",
        r"\\srv%40evil\x",
        r"\\srv.evil.com\share",
    ] {
        let listing = go(&chooser, id, attack).unwrap();
        assert_eq!(listing.state, ListState::NotAllowed, "{attack}");
    }
    for invalid in [
        r"\\.\pipe\x",
        r"\\?\UNC\evil\x",
        r"\\[::1]\x",
        r"\\evil:445\x",
    ] {
        let refused = go(&chooser, id, invalid).unwrap_err();
        assert_eq!(refused.code(), ErrorCode::CommandInvalid, "{invalid}");
    }
    let share = go(&chooser, id, r"\\Srv\share\Team").unwrap();
    assert_eq!(share.display, r"\\Srv\share\Team");
    assert!(matches!(share.state, ListState::Folders { .. }));

    // With no server in use, no typed share opens; a mapped drive still does.
    let (none, _release) = fake_with(r"C:\Users\rmehra\Desktop", &[], false);
    let none_id = open(&none, crate::open::now()).chooser;
    assert_eq!(
        go(&none, none_id, r"\\srv\share").unwrap().state,
        ListState::NotAllowed
    );
    assert_eq!(names(&go(&none, none_id, r"Z:\").unwrap()), ["Mapped"]);
}

/// A typed path through a link (`C:\Users\All Users`) or a hidden folder (`AppData`) does not
/// open, nor does one naming a device.
#[test]
fn a_typed_path_goes_through_no_link_or_hidden_folder() {
    let (chooser, _release) = fake_chooser(r"C:\Users\rmehra\Desktop");
    let id = open(&chooser, crate::open::now()).chooser;
    for path in [
        r"C:\Users\All Users",
        r"C:\Users\All Users\Data",
        r"C:\Users\rmehra\AppData",
        r"C:\Users\rmehra\AppData\Local",
    ] {
        assert_eq!(
            go(&chooser, id, path).unwrap().state,
            ListState::NotAllowed,
            "{path}"
        );
    }
    assert!(matches!(
        go(&chooser, id, r"C:\Users").unwrap().state,
        ListState::Folders { .. }
    ));
    for device in [
        r"C:\Cases\CON",
        r"C:\nul.txt",
        r"C:\nul .txt",
        r"C:\com1\x",
        r"C:\Lpt9.log",
        r"C:\aux",
        r"C:\aux.d",
        r"C:\CONIN$",
        r"C:\conout$.x",
        "C:\\COM\u{b9}",
        "C:\\lpt\u{b3}.txt",
    ] {
        assert_eq!(Place::typed(device), None, "{device}");
    }
    // A listed folder that only starts with a device name still shows and opens.
    let (listed, more) = folders(vec![dir("aux.d"), dir("CON")]);
    assert_eq!(
        (listed, more),
        (vec!["aux.d".to_owned(), "CON".to_owned()], false)
    );
    assert!(Place::drive('C').and_then(|c| c.child("aux.d")).is_some());
    assert!(Place::typed(r"C:\Console\Contracts").is_some());
}

/// At most 3 sessions open in a minute and 20 in a run; past either, `open` gives none (the pick
/// draws the native dialog).
#[test]
fn sessions_are_capped_per_minute_and_per_run() {
    let (chooser, _release) = fake_chooser(r"C:\Users\rmehra\Desktop");
    let start = crate::open::now();
    let mut at = start;
    let mut opened = 0;
    for minute in 0..10_u32 {
        at = start + Duration::from_secs(61) * minute;
        while chooser.may_open(at) {
            open(&chooser, at);
            opened += 1;
        }
        assert!(chooser.open(None, None, &[], at).is_none());
        if opened == SESSIONS_MAX {
            break;
        }
        assert_eq!(opened, SESSIONS_PER_MINUTE * (minute as usize + 1));
    }
    assert_eq!(opened, SESSIONS_MAX);
    let much_later = at + Duration::from_secs(3600);
    assert!(!chooser.may_open(much_later));
    assert!(chooser.open(None, None, &[], much_later).is_none());
}

/// A read of the drives that hangs leaves the session without drives after about 1.5 s: no
/// drive start points, and no network drive.
#[test]
fn a_hanging_drive_read_leaves_no_drives_after_one_and_a_half_seconds() {
    let (chooser, release) = fake_with(r"C:\Users\rmehra\Desktop", &["SRV"], true);
    let started = crate::open::now();
    let start = open(&chooser, started);
    let waited = crate::open::now().saturating_duration_since(started);
    assert!(
        waited >= Duration::from_millis(1400) && waited < Duration::from_secs(4),
        "{waited:?}"
    );
    assert!(start.starts.iter().all(|p| p.kind != StartKind::Drive));
    assert!(start.starts.iter().any(|p| p.kind == StartKind::Home));
    let share = chooser.list(
        start.chooser,
        Go::Typed {
            path: r"\\srv\share".to_owned(),
        },
        started,
    );
    assert_eq!(share.unwrap().state, ListState::NotAllowed);
    drop(release);
}
