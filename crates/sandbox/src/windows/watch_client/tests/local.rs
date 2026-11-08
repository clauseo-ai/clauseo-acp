//! Which refusals count as local, so that the Authz check may open them: `"net":false` sent,
//! and an `X:\...` path, opened part by part with no reparse point on the way.

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

/// A file system of folders, some of them reparse points, that records every open.
#[derive(Default)]
struct FakeWalk {
    /// Full paths that are reparse points.
    links: Vec<String>,
    /// Full paths whose open fails.
    failing: Vec<String>,
    /// Each open: `root X:` or `child <dir> <name> <last>`.
    opens: Vec<String>,
}

impl Walker for FakeWalk {
    type Handle = String;

    fn root(&mut self, root: &str) -> Option<String> {
        self.opens.push(format!("root {root}"));
        Some(root.to_owned())
    }

    fn child(&mut self, dir: &String, name: &str, last: bool) -> Option<(String, bool)> {
        assert!(!name.contains('\\'), "one part at a time: {name}");
        self.opens.push(format!("child {dir} {name} {last}"));
        let full = format!("{}\\{name}", dir.trim_end_matches('\\'));
        if self.failing.contains(&full) {
            return None;
        }
        let link = self.links.contains(&full);
        Some((full, link))
    }
}

#[test]
fn a_path_is_opened_part_by_part_relative_to_its_folder_and_never_whole() {
    let mut walk = FakeWalk::default();
    let handle = open_no_reparse(Path::new(r"C:\a\b\c.txt"), &mut walk);
    assert_eq!(handle.as_deref(), Some(r"C:\a\b\c.txt"));
    assert_eq!(
        walk.opens,
        [
            r"root C:\",
            r"child C:\ a false",
            r"child C:\a b false",
            r"child C:\a\b c.txt true",
        ]
    );
    // A folder with a trailing separator, and the root alone.
    let mut walk = FakeWalk::default();
    assert!(open_no_reparse(Path::new(r"C:\a\"), &mut walk).is_some());
    assert_eq!(walk.opens, [r"root C:\", r"child C:\ a true"]);
    let mut walk = FakeWalk::default();
    assert_eq!(
        open_no_reparse(Path::new(r"C:\"), &mut walk).as_deref(),
        Some(r"C:\")
    );
}

#[test]
fn a_reparse_point_or_a_refused_open_mid_walk_gives_none_and_stops() {
    for link in [r"C:\a", r"C:\a\b", r"C:\a\b\c.txt"] {
        let mut walk = FakeWalk {
            links: vec![link.to_owned()],
            ..FakeWalk::default()
        };
        assert_eq!(
            open_no_reparse(Path::new(r"C:\a\b\c.txt"), &mut walk),
            None,
            "{link}"
        );
        let last = walk.opens.last().unwrap();
        let opened = last.split(' ').nth(2).unwrap();
        assert!(
            link.ends_with(opened),
            "nothing is opened past {link}: {last}"
        );
    }
    // A folder swapped for a link to a share after it was checked fails its relative open
    // (`OBJ_DONT_REPARSE`): `None`, and nothing past it.
    let mut walk = FakeWalk {
        failing: vec![r"C:\a\b".to_owned()],
        ..FakeWalk::default()
    };
    assert_eq!(open_no_reparse(Path::new(r"C:\a\b\c.txt"), &mut walk), None);
    assert_eq!(walk.opens.len(), 3);
    // A path that isn't local opens nothing.
    let mut walk = FakeWalk::default();
    assert_eq!(
        open_no_reparse(Path::new(r"\\server\share\x"), &mut walk),
        None
    );
    assert_eq!(walk.opens, Vec::<String>::new());
}
