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

/// The marker made the desktop user's alone, as `check_owned_file` and `own_file` make it on
/// Windows.
fn owner_only(path: &Path) {
    #[cfg(unix)]
    crate::test_support::set_mode(path, 0o600);
    #[cfg(not(unix))]
    let _ = path;
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
fn a_marker_is_left_by_every_end_but_a_clean_one() {
    let data = TempDir::new("marker-ends");
    run_folder(&data);
    // Three sessions start: each writes its marker first.
    let ids = ["00000000000000aa", "00000000000000bb", "00000000000000cc"];
    let paths: Vec<PathBuf> = ids
        .iter()
        .map(|id| write_marker(data.path(), id, &sid(POOL)).unwrap())
        .collect();
    for path in &paths {
        assert!(path.is_file(), "{}", path.display());
        owner_only(path);
    }
    // The first ends clean and removes its marker; the second was cancelled and the third
    // crashed, and neither removes it.
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
            },
            UncleanSession {
                id: ids[2].to_owned(),
                pool_user: Some(sid(POOL)),
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
    let shared = write_marker(data.path(), "00000000000000ee", &sid(POOL)).unwrap();
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
            },
            UncleanSession {
                id: "00000000000000ee".to_owned(),
                pool_user: if cfg!(unix) { None } else { Some(sid(POOL)) },
            },
        ]
    );
}

#[test]
fn a_marker_takes_only_a_launch_id() {
    let data = TempDir::new("marker-id");
    run_folder(&data);
    for bad in ["../escape", "0123456789ABCDEF", "short"] {
        let got = write_marker(data.path(), bad, &sid(POOL));
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
    let refused = marked(data.path(), &launch, &sid(POOL), |_| logged_on = true);
    assert!(refused.is_err());
    assert!(!logged_on, "a session ran with no marker");
    // Nor with a launch file whose name gives no id.
    run_folder(&data);
    let unnamed = data.path().join("sandbox").join("run").join("launch.json");
    let refused = marked(data.path(), &unnamed, &sid(POOL), |_| logged_on = true);
    assert_eq!(
        refused.map_err(|e| e.kind()),
        Err(io::ErrorKind::InvalidInput)
    );
    assert!(!logged_on);
    // Written, the session runs with the marker's path, already on disk.
    let ran = marked(data.path(), &launch, &sid(POOL), |marker| {
        logged_on = true;
        marker.is_file().then_some(marker)
    });
    assert!(logged_on);
    assert_eq!(ran.unwrap(), Some(marker_path(data.path(), ID)));
}
