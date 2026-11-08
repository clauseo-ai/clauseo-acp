//! Which refusals count as local, so that the Authz check may open them: `"net":false` sent,
//! and an `X:\...` path with no reparse point on the way.

use super::*;

#[test]
fn only_a_sent_net_false_on_a_drive_letter_path_is_checked() {
    let fake = Fake::default();
    fake.script()
        .deny
        .insert(PathBuf::from(r"C:\readable"), Some(false));
    let pipe = fake.pipe();
    let hub = Hub::new(fake.clone());
    let dir = TempDir::new("watch-client-local");
    let policy = policy(&dir);
    let a = launch();
    let _a = hub.register(sid(POOL_A), Arc::clone(&policy), Arc::clone(&a.sink));
    // No `net` at all: a network path, never checked, whatever the protocol's default is.
    let missing = serde_json::json!({
        "v": 1, "sid": POOL_A, "pid": 7, "path": r"C:\readable", "access": "unknown",
    });
    let mut missing = serde_json::to_vec(&missing).unwrap();
    missing.push(b'\n');
    pipe.send(missing).unwrap();
    // `net:false`, but not `X:\...`: a display string, never checked.
    for path in [
        r"\\server\share\x",
        "C:/readable",
        r"\Device\HarddiskVolume3\x",
    ] {
        pipe.send(line(POOL_A, path, "unknown", false)).unwrap();
    }
    // `net:false` and `X:\...`: checked.
    pipe.send(line(POOL_A, r"C:\readable", "unknown", false))
        .unwrap();
    eventually("five came", || a.keep.got().len() == 5);
    assert_eq!(
        a.keep.got(),
        [
            path_denial(&policy, r"C:\readable", Access::Read),
            path_denial(&policy, r"\\server\share\x", Access::Read),
            path_denial(&policy, "C:/readable", Access::Read),
            path_denial(&policy, r"\Device\HarddiskVolume3\x", Access::Read),
            path_denial(&policy, r"C:\readable", Access::Write),
        ]
    );
    let asked = fake.0.asked.lock().unwrap().clone();
    assert_eq!(
        asked,
        [PathBuf::from(r"C:\readable")],
        "only the last was checked"
    );
}

#[test]
fn a_reparse_point_anywhere_on_the_path_refuses_the_check() {
    let plain = |_: &Path| Some(false);
    let path = Path::new(r"C:\a\b\c.txt");
    // Every part from the drive's first folder down is asked, the last included.
    let mut asked = Vec::new();
    let walk = crosses_reparse(path, |part| {
        asked.push(part.to_owned());
        Some(false)
    });
    assert_eq!(walk, Some(false));
    assert_eq!(
        asked,
        [r"C:\a", r"C:\a\b", r"C:\a\b\c.txt"].map(PathBuf::from)
    );
    for link in [r"C:\a", r"C:\a\b", r"C:\a\b\c.txt"] {
        let found = crosses_reparse(path, |part| Some(part == Path::new(link)));
        assert_eq!(found, Some(true), "{link}");
    }
    // A folder with a trailing separator, and the root alone, which isn't asked.
    assert_eq!(crosses_reparse(Path::new(r"C:\a\"), plain), Some(false));
    assert_eq!(crosses_reparse(Path::new(r"C:\"), |_| None), Some(false));
    // A part that can't be read, or a path that isn't local, can't tell.
    assert_eq!(crosses_reparse(Path::new(r"C:\a\b"), |_| None), None);
    assert_eq!(crosses_reparse(Path::new(r"\\server\share\x"), plain), None);
}
