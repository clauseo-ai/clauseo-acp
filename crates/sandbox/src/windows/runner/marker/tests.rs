//! The unclean-session marker on a scratch folder: its name and place, its write and removal,
//! and the listing of what was left.

use super::*;
use crate::test_support::TempDir;

const POOL: &str = "S-1-5-21-1111111111-2222222222-3333333333-1001";
const ID: &str = "0123456789abcdef";

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

/// The run folder, as the host's `write_launch` makes it.
fn run_folder(data: &TempDir) -> PathBuf {
    data.dir("sandbox/run")
}

/// The marker made the desktop user's alone: mode 0600, or on Windows through
/// `check_owned_file` and `own_file`, as `write_marker_at` makes it. A file the test writes
/// itself has the temp folder's inherited DACL, which the read takes as missing.
fn owner_only(path: &Path) {
    #[cfg(unix)]
    crate::test_support::set_mode(path, 0o600);
    #[cfg(windows)]
    {
        crate::windows::write::check_owned_file(path).unwrap();
        crate::windows::private_dir::own_file(path).unwrap();
    }
}

#[test]
fn the_marker_is_named_for_the_launch_id_in_the_run_folder() {
    let data = Path::new("data");
    assert_eq!(
        marker_path(data, ID),
        data.join("sandbox")
            .join("run")
            .join("0123456789abcdef.session")
    );
    assert_eq!(marker_id("0123456789abcdef.session"), Some(ID));
    for bad in [
        "0123456789abcdef.json",
        "0123456789ABCDEF.session",
        "0123456789abcde.session",
        ".0123456789abcdef.session.0011223344556677.tmp",
        "0123456789abcdef.session.tmp",
    ] {
        assert_eq!(marker_id(bad), None, "{bad}");
    }
    let launch = data
        .join("sandbox")
        .join("run")
        .join("0123456789abcdef.json");
    assert_eq!(launch_id(&launch), Some(ID));
    assert_eq!(launch_id(Path::new("0123456789abcdef.toml")), None);
    assert_eq!(launch_id(Path::new("launch.json")), None);
}

#[test]
fn a_marker_is_left_until_it_is_removed() {
    let data = TempDir::new("marker-ends");
    run_folder(&data);
    // Three sessions start: each writes its marker first.
    let ids = ["00000000000000aa", "00000000000000bb", "00000000000000cc"];
    let paths: Vec<PathBuf> = ids
        .iter()
        .map(|id| write_marker(data.path(), id, &sid(POOL), None).unwrap())
        .collect();
    for path in &paths {
        assert!(path.is_file(), "{}", path.display());
        owner_only(path);
    }
    // The first is found clean and removed; the second is still running and the third's outer
    // died, and neither is removed.
    remove_marker(&paths[0]).unwrap();
    assert!(!paths[0].exists());
    // A second removal is fine.
    remove_marker(&paths[0]).unwrap();
    let left = unclean_sessions(data.path()).unwrap();
    assert_eq!(
        left,
        [
            UncleanSession {
                id: ids[1].to_owned(),
                pool_user: Some(sid(POOL)),
                state: Some(MarkerState::Open),
                install: None,
            },
            UncleanSession {
                id: ids[2].to_owned(),
                pool_user: Some(sid(POOL)),
                state: Some(MarkerState::Open),
                install: None,
            },
        ]
    );
}

#[test]
fn the_listing_skips_other_files_and_still_lists_a_marker_it_cant_read() {
    let data = TempDir::new("marker-listing");
    // No run folder yet: nothing left.
    assert_eq!(unclean_sessions(data.path()).unwrap(), []);
    run_folder(&data);
    data.file("sandbox/run/0123456789abcdef.json", "{}");
    data.file(
        "sandbox/run/.0123456789abcdef.session.0011223344556677.tmp",
        "x",
    );
    data.file("sandbox/run/notes.session", "x");
    // Not a marker's content, and not the user's alone: listed, with no pool user.
    let garbled = data.file("sandbox/run/00000000000000dd.session", "not json");
    owner_only(&garbled);
    let shared = write_marker(data.path(), "00000000000000ee", &sid(POOL), None).unwrap();
    #[cfg(unix)]
    crate::test_support::set_mode(&shared, 0o644);
    #[cfg(not(unix))]
    let _ = shared;
    let left = unclean_sessions(data.path()).unwrap();
    assert_eq!(
        left,
        [
            UncleanSession {
                id: "00000000000000dd".to_owned(),
                pool_user: None,
                state: None,
                install: None,
            },
            UncleanSession {
                id: "00000000000000ee".to_owned(),
                pool_user: if cfg!(unix) { None } else { Some(sid(POOL)) },
                state: if cfg!(unix) {
                    None
                } else {
                    Some(MarkerState::Open)
                },
                install: None,
            },
        ]
    );
}

#[test]
fn a_marker_takes_only_a_launch_id() {
    let data = TempDir::new("marker-id");
    run_folder(&data);
    for bad in ["../escape", "0123456789ABCDEF", "short"] {
        let got = write_marker(data.path(), bad, &sid(POOL), None);
        assert_eq!(
            got.map_err(|e| e.kind()),
            Err(io::ErrorKind::InvalidInput),
            "{bad}"
        );
    }
}

#[test]
fn a_marker_that_cant_be_written_refuses_the_launch_before_any_logon() {
    let data = TempDir::new("marker-refused");
    let launch = data
        .path()
        .join("sandbox")
        .join("run")
        .join(format!("{ID}.json"));
    // No run folder: the write fails, and the session (the logons) never runs.
    let mut logged_on = false;
    let refused = marked(data.path(), &launch, &sid(POOL), None, |_| logged_on = true);
    assert!(refused.is_err());
    assert!(!logged_on, "a session ran with no marker");
    // Nor with a launch file whose name gives no id.
    run_folder(&data);
    let unnamed = data.path().join("sandbox").join("run").join("launch.json");
    let refused = marked(data.path(), &unnamed, &sid(POOL), None, |_| {
        logged_on = true;
    });
    assert_eq!(
        refused.map_err(|e| e.kind()),
        Err(io::ErrorKind::InvalidInput)
    );
    assert!(!logged_on);
    // Written, the session runs with the marker's path, already on disk.
    let ran = marked(data.path(), &launch, &sid(POOL), None, |marker| {
        logged_on = true;
        marker.is_file().then_some(marker)
    });
    assert!(logged_on);
    assert_eq!(ran.unwrap(), Some(marker_path(data.path(), ID)));
}

#[test]
fn a_marker_is_settled_in_place_and_reads_back_what_it_waits_for() {
    let data = TempDir::new("marker-states");
    run_folder(&data);
    let path = write_marker(data.path(), ID, &sid(POOL), None).unwrap();
    owner_only(&path);
    let states = [
        MarkerState::AwaitingSweep { sweep: 7 },
        MarkerState::AwaitingStop { stop: 3 },
    ];
    for state in states {
        write_marker_at(&path, Some(&sid(POOL)), None, state).unwrap();
        owner_only(&path);
        let left = unclean_sessions(data.path()).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].state, Some(state));
        assert_eq!(left[0].pool_user, Some(sid(POOL)));
    }
    // One the desktop's start wrote for a marker it couldn't read names no pool user.
    write_marker_at(&path, None, None, states[1]).unwrap();
    owner_only(&path);
    let left = unclean_sessions(data.path()).unwrap();
    assert_eq!(
        (left[0].pool_user.clone(), left[0].state),
        (None, Some(states[1]))
    );
    // Only a marker's own name is written.
    let other = data
        .path()
        .join("sandbox")
        .join("run")
        .join("notes.session");
    let refused = write_marker_at(&other, Some(&sid(POOL)), None, states[0]);
    assert_eq!(
        refused.map_err(|e| e.kind()),
        Err(io::ErrorKind::InvalidInput)
    );
}

#[test]
fn a_marker_from_before_the_states_reads_as_open() {
    let data = TempDir::new("marker-old");
    run_folder(&data);
    let old = format!("{{\"pool_user\":\"{POOL}\"}}");
    let path = data.file(&format!("sandbox/run/{ID}.session"), &old);
    owner_only(&path);
    let left = unclean_sessions(data.path()).unwrap();
    assert_eq!(left[0].state, Some(MarkerState::Open));
    // An unknown state isn't a marker's content.
    let odd = format!("{{\"pool_user\":\"{POOL}\",\"state\":{{\"kind\":\"done\"}}}}");
    let path = data.file(&format!("sandbox/run/{ID}.session"), &odd);
    owner_only(&path);
    assert_eq!(unclean_sessions(data.path()).unwrap()[0].state, None);
}

#[test]
fn a_stop_awaiting_marker_written_with_a_sweep_number_reads_without_it() {
    let data = TempDir::new("marker-compat");
    run_folder(&data);
    let state = r#"{"kind":"awaiting_stop","stop":3,"sweep":2}"#;
    let old = format!(r#"{{"pool_user":"{POOL}","state":{state}}}"#);
    let path = data.file(&format!("sandbox/run/{ID}.session"), &old);
    owner_only(&path);
    let left = unclean_sessions(data.path()).unwrap();
    assert_eq!(left[0].state, Some(MarkerState::AwaitingStop { stop: 3 }));
    // Written again, it holds no sweep number.
    let stop = MarkerState::AwaitingStop { stop: 3 };
    write_marker_at(&path, Some(&sid(POOL)), None, stop).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("sweep"), "{text}");
}

#[test]
fn a_marker_is_clean_once_every_number_it_waits_for_is_met() {
    let done = |stop, sweep| Sequences { stop, sweep };
    // Open is never clean; absent sequences read as 0.
    assert!(!MarkerState::Open.clean(done(u64::MAX, u64::MAX)));
    let sweep = MarkerState::AwaitingSweep { sweep: 5 };
    assert!(!sweep.clean(done(9, 4)));
    assert!(sweep.clean(done(0, 5)));
    assert!(sweep.clean(done(0, 6)));
    // A stop-awaiting marker is never clean: once its stop completed it is re-marked.
    let stop = MarkerState::AwaitingStop { stop: 2 };
    assert!(!stop.clean(done(u64::MAX, u64::MAX)));
    assert!(!stop.stopped(done(1, 9)));
    assert!(stop.stopped(done(2, 0)));
    assert!(!sweep.stopped(done(9, 9)));
    assert_eq!((stop.stop(), stop.sweep()), (Some(2), None));
    assert_eq!((sweep.stop(), sweep.sweep()), (None, Some(5)));
    assert_eq!(
        (MarkerState::Open.stop(), MarkerState::Open.sweep()),
        (None, None)
    );
}

#[test]
fn a_marker_records_the_install_it_was_written_under() {
    let data = TempDir::new("marker-install");
    run_folder(&data);
    let install = "0123456789abcdef0123456789abcdef";
    let path = write_marker(data.path(), ID, &sid(POOL), Some(install)).unwrap();
    owner_only(&path);
    let left = unclean_sessions(data.path()).unwrap();
    assert_eq!(left[0].install.as_deref(), Some(install));
    // Under an install without one, none is written, and one from before reads as none.
    write_marker_at(&path, Some(&sid(POOL)), None, MarkerState::Open).unwrap();
    owner_only(&path);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("install"), "{text}");
    assert_eq!(unclean_sessions(data.path()).unwrap()[0].install, None);
}
