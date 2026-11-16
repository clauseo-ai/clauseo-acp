//! The fence's decisions on a fake disk (Unix paths): literal paths, case, links, `/var` and
//! `/private/var`, new files, search roots in and above a fenced place, identity, the network.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::decide::{Denial, FENCED_TOOLS, Fence, decide};
use super::real::{Ground, readings};
use super::{FenceFile, FencedRoot, FileId, RootKind};
use crate::network_paths::NetEnv;
use crate::network_paths::resolve::Disk;
use crate::network_paths::resolve::tests::{Fake, FakeFs};
use crate::network_paths::tests::{mac_net, plain_net};

const DATA: &str = "/Users/a/Library/Application Support/Clauseo";
pub(super) const MATTER: &str = "/Users/a/Matters/Smith";
const OTHER: &str = "/Users/a/Matters/Jones";

/// The data folder's root as the host writes it, with its identity `(1, 7)` and its line.
pub(super) fn data_root() -> FencedRoot {
    let read = [
        "run/*/relay-token",
        "run/*/file-fence.json",
        "matters/**",
        "state.sqlite*",
        "agents/claude-acp/login-source",
    ];
    FencedRoot {
        kind: RootKind::Data,
        spellings: vec![DATA.to_owned()],
        id: Some(FileId::of((1, 7))),
        line: vec![FileId::of((1, 7)), FileId::of((1, 3)), FileId::of((1, 1))],
        read: read.iter().map(|text| (*text).to_owned()).collect(),
        edit: vec!["tools/**".to_owned()],
    }
}

/// Another matter's folder, denied whole.
pub(super) fn other_root() -> FencedRoot {
    FencedRoot {
        kind: RootKind::Matter,
        spellings: vec![OTHER.to_owned()],
        id: Some(FileId::of((1, 9))),
        line: vec![FileId::of((1, 9)), FileId::of((1, 3)), FileId::of((1, 1))],
        read: vec!["**".to_owned()],
        edit: Vec::new(),
    }
}

/// The fake disk: the matter, the data folder, another matter, and `extra`.
fn disk(extra: &[(&str, Fake)]) -> FakeFs {
    let mut names = vec![
        (MATTER, Fake::Dir),
        ("/Users/a/Matters/Smith/brief.docx", Fake::File),
        (DATA, Fake::Dir),
        (
            "/Users/a/Library/Application Support/Clauseo/state.sqlite",
            Fake::File,
        ),
        (
            "/Users/a/Library/Application Support/Clauseo/tools/x",
            Fake::File,
        ),
        (
            "/Users/a/Library/Application Support/Clauseo/run/r1/work",
            Fake::Dir,
        ),
        ("/Users/a/Matters/Jones/lease.pdf", Fake::File),
    ];
    names.extend(extra.iter().cloned());
    let mut fs = FakeFs::with(&names);
    fs.ids.insert(PathBuf::from(DATA), (1, 7));
    fs.ids.insert(PathBuf::from(OTHER), (1, 9));
    fs.ids.insert(PathBuf::from("/Users/a"), (1, 3));
    fs.ids.insert(PathBuf::from("/"), (1, 1));
    fs
}

/// The decision for `tool` with `input` from the matter, on `fs` under `net`, names compared
/// ignoring case when `insensitive`.
fn decide_on(
    fs: &FakeFs,
    net: &NetEnv,
    insensitive: bool,
    tool: &str,
    input: &Value,
) -> Option<Denial> {
    let file = FenceFile {
        version: 1,
        roots: vec![data_root(), other_root()],
    };
    let ids = fs.ids.clone();
    let identity = move |path: &Path| ids.get(path).copied().map(FileId::of);
    let fence = Fence::new(&file, insensitive, Box::new(identity));
    let real = |_: &Path| None;
    let disk = Disk {
        fs,
        real: &real,
        net,
    };
    let home = PathBuf::from("/Users/a");
    let ground = Ground {
        cwd: Path::new(MATTER),
        home: Some(&home),
        shares: &[PathBuf::from(MATTER)],
        disk: &disk,
    };
    decide(&fence, tool, input, &ground)
}

/// [`decide_on`] with plain Unix rules, case kept.
fn decide_plain(fs: &FakeFs, tool: &str, input: &Value) -> Option<Denial> {
    decide_on(fs, &plain_net(false), false, tool, input)
}

const DATA_HIT: Option<Denial> = Some(Denial::Fenced(RootKind::Data));
const MATTER_HIT: Option<Denial> = Some(Denial::Fenced(RootKind::Matter));

#[test]
fn a_file_in_the_data_folder_or_another_matter_is_refused_to_every_file_tool() {
    let fs = disk(&[]);
    let state = format!("{DATA}/state.sqlite");
    let lease = format!("{OTHER}/lease.pdf");
    for tool in ["Read", "Edit", "Write", "MultiEdit"] {
        assert_eq!(
            decide_plain(&fs, tool, &json!({"file_path": state})),
            DATA_HIT,
            "{tool}"
        );
        assert_eq!(
            decide_plain(&fs, tool, &json!({"file_path": lease})),
            MATTER_HIT,
            "{tool}"
        );
    }
    let notebook = json!({"notebook_path": format!("{OTHER}/n.ipynb")});
    assert_eq!(decide_plain(&fs, "NotebookEdit", &notebook), MATTER_HIT);
    // The matter's own file, and the data folder's unfenced parts, go on to the other checks.
    let own = json!({"file_path": format!("{MATTER}/brief.docx")});
    assert_eq!(decide_plain(&fs, "Read", &own), None);
    assert_eq!(decide_plain(&fs, "Edit", &own), None);
    let work = json!({"file_path": format!("{DATA}/run/r1/work/notes.md")});
    assert_eq!(decide_plain(&fs, "Write", &work), None);
    // A relative path is taken from the working folder; `~` from the home.
    let relative = json!({"file_path": "../Jones/lease.pdf"});
    assert_eq!(decide_plain(&fs, "Read", &relative), MATTER_HIT);
    let tilde = json!({"file_path": "~/Matters/Jones/lease.pdf"});
    assert_eq!(decide_plain(&fs, "Read", &tilde), MATTER_HIT);
    let dotted = json!({"file_path": format!("{MATTER}/../Jones/./lease.pdf")});
    assert_eq!(decide_plain(&fs, "Read", &dotted), MATTER_HIT);
}

#[test]
fn a_read_only_place_is_refused_to_the_writing_tools_alone() {
    let fs = disk(&[]);
    let tool = json!({"file_path": format!("{DATA}/tools/x")});
    assert_eq!(decide_plain(&fs, "Read", &tool), None);
    assert_eq!(decide_plain(&fs, "Edit", &tool), DATA_HIT);
    assert_eq!(decide_plain(&fs, "Write", &tool), DATA_HIT);
}

#[test]
fn a_long_path_is_judged_by_where_it_lands() {
    let mut deep = format!("{OTHER}/");
    while deep.len() < 1200 {
        deep.push_str("A folder with a long name for the lease schedules/");
    }
    deep.push_str("lease.pdf");
    let fs = disk(&[]);
    assert_eq!(
        decide_plain(&fs, "Read", &json!({"file_path": deep})),
        MATTER_HIT
    );
    assert_eq!(
        decide_plain(&fs, "Write", &json!({"file_path": deep})),
        MATTER_HIT
    );
}

#[test]
fn another_case_is_refused_where_the_volume_ignores_case() {
    let fs = disk(&[]);
    let shouted = json!({"file_path": "/Users/a/LIBRARY/application support/CLAUSEO/State.SQLITE"});
    let mac = mac_net(&[]);
    assert_eq!(decide_on(&fs, &mac, true, "Read", &shouted), DATA_HIT);
    let other = json!({"file_path": "/users/A/matters/JONES/Lease.pdf"});
    assert_eq!(decide_on(&fs, &mac, true, "Read", &other), MATTER_HIT);
    // A volume that keeps case: another name, another file.
    assert_eq!(decide_plain(&fs, "Read", &shouted), None);
}

#[test]
fn a_link_into_a_fenced_place_is_followed() {
    let fs = disk(&[
        ("/Users/a/Matters/Smith/data", Fake::Link(DATA.into())),
        ("/Users/a/Matters/Smith/peek", Fake::Link("../Jones".into())),
        (
            "/Users/a/Matters/Smith/dangling",
            Fake::Link(format!("{DATA}/matters/m9/notes.md").into()),
        ),
    ]);
    let via = |path: &str| json!({"file_path": format!("{MATTER}/{path}")});
    assert_eq!(
        decide_plain(&fs, "Read", &via("data/state.sqlite")),
        DATA_HIT
    );
    assert_eq!(
        decide_plain(&fs, "Read", &via("peek/lease.pdf")),
        MATTER_HIT
    );
    // A link to a file that does not exist yet: a Write through it lands in the data folder.
    assert_eq!(decide_plain(&fs, "Write", &via("dangling")), DATA_HIT);
    // A search from a link to the data folder searches the data folder.
    assert_eq!(
        decide_plain(
            &fs,
            "Grep",
            &json!({"pattern": "x", "path": via("data")["file_path"]})
        ),
        Some(Denial::Holds(RootKind::Data))
    );
}

#[test]
fn var_and_private_var_are_one_place_on_macos() {
    // macOS's `/var` is a link to `private/var`; the host writes the root as resolved.
    let fs = FakeFs::with(&[
        ("/var", Fake::Link("private/var".into())),
        ("/private/var/folders/xy/T/Clauseo/state.sqlite", Fake::File),
        (MATTER, Fake::Dir),
    ]);
    let root = FencedRoot {
        spellings: vec!["/private/var/folders/xy/T/Clauseo".to_owned()],
        id: None,
        line: Vec::new(),
        ..data_root()
    };
    let file = FenceFile {
        version: 1,
        roots: vec![root],
    };
    let fence = Fence::new(&file, true, Box::new(|_: &Path| -> Option<FileId> { None }));
    let net = mac_net(&[]);
    let real = |_: &Path| None;
    let disk = Disk {
        fs: &fs,
        real: &real,
        net: &net,
    };
    let ground = Ground {
        cwd: Path::new(MATTER),
        home: None,
        shares: &[],
        disk: &disk,
    };
    for path in [
        "/var/folders/xy/T/Clauseo/state.sqlite",
        "/private/var/folders/xy/T/Clauseo/state.sqlite",
    ] {
        let input = json!({"file_path": path});
        assert_eq!(decide(&fence, "Read", &input, &ground), DATA_HIT, "{path}");
    }
    let search = json!({"pattern": "/var/folders/xy/**/*.sqlite"});
    assert_eq!(
        decide(&fence, "Glob", &search, &ground),
        Some(Denial::Holds(RootKind::Data))
    );
}

#[test]
fn a_new_file_under_a_fenced_folder_is_refused() {
    let fs = disk(&[]);
    let new = json!({"file_path": format!("{OTHER}/Drafts/2026/new.docx")});
    assert_eq!(decide_plain(&fs, "Write", &new), MATTER_HIT);
    let missing = json!({"file_path": format!("{DATA}/matters/m2/notes/x.md")});
    assert_eq!(decide_plain(&fs, "Write", &missing), DATA_HIT);
    let back_out = json!({"file_path": format!("{MATTER}/Drafts/../../Jones/new.docx")});
    assert_eq!(decide_plain(&fs, "Write", &back_out), MATTER_HIT);
    let own = json!({"file_path": format!("{MATTER}/Drafts/new.docx")});
    assert_eq!(decide_plain(&fs, "Write", &own), None);
}

#[test]
fn a_search_root_inside_or_above_a_fenced_place_is_refused() {
    let fs = disk(&[]);
    let grep = |path: &str| json!({"pattern": "lease", "path": path});
    let holds = |kind| Some(Denial::Holds(kind));
    assert_eq!(
        decide_plain(&fs, "Grep", &grep(OTHER)),
        holds(RootKind::Matter)
    );
    assert_eq!(
        decide_plain(&fs, "Grep", &grep(&format!("{DATA}/matters"))),
        holds(RootKind::Data)
    );
    // Above: the home holds the data folder and the other matter.
    assert!(decide_plain(&fs, "Grep", &grep("/Users/a")).is_some());
    assert!(decide_plain(&fs, "Grep", &grep("/")).is_some());
    assert_eq!(
        decide_plain(&fs, "Grep", &grep("/Users/a/Matters")),
        holds(RootKind::Matter)
    );
    // The run folder holds a token; its work folder holds nothing fenced.
    assert_eq!(
        decide_plain(&fs, "Grep", &grep(&format!("{DATA}/run/r1"))),
        holds(RootKind::Data)
    );
    assert_eq!(
        decide_plain(&fs, "Grep", &grep(&format!("{DATA}/run/r1/work"))),
        None
    );
    assert_eq!(decide_plain(&fs, "Grep", &grep(MATTER)), None);
    assert_eq!(decide_plain(&fs, "Grep", &json!({"pattern": "x"})), None);
    // Glob: the folder before the first glob character.
    let glob = |pattern: &str| json!({"pattern": pattern});
    assert_eq!(decide_plain(&fs, "Glob", &glob("**/*.docx")), None);
    assert_eq!(
        decide_plain(&fs, "Glob", &glob("../Jones/**/*.pdf")),
        holds(RootKind::Matter)
    );
    assert!(decide_plain(&fs, "Glob", &glob("/Users/a/**/*.pdf")).is_some());
    let from = json!({"pattern": "*.pdf", "path": "/Users/a/Matters"});
    assert_eq!(decide_plain(&fs, "Glob", &from), holds(RootKind::Matter));
    for unbounded in ["*/../../Jones/*.pdf", "{a,../x}/*", "x/{/etc,/Users}/**"] {
        assert_eq!(
            decide_plain(&fs, "Glob", &glob(unbounded)),
            Some(Denial::Unbounded),
            "{unbounded}"
        );
    }
}

#[test]
fn a_second_spelling_of_a_fenced_folder_is_caught_by_its_identity() {
    // A bind mount, a firmlinked path or a second mount point: another text, the same folder.
    let mut fs = disk(&[
        ("/mnt/alias/state.sqlite", Fake::File),
        ("/mnt/home", Fake::Dir),
    ]);
    fs.ids.insert(PathBuf::from("/mnt/alias"), (1, 7));
    fs.ids.insert(PathBuf::from("/mnt/home"), (1, 3));
    let file = json!({"file_path": "/mnt/alias/state.sqlite"});
    assert_eq!(decide_plain(&fs, "Read", &file), DATA_HIT);
    let tools = json!({"file_path": "/mnt/alias/tools/x"});
    assert_eq!(decide_plain(&fs, "Read", &tools), None);
    assert_eq!(
        decide_plain(&fs, "Grep", &json!({"pattern": "x", "path": "/mnt/alias"})),
        Some(Denial::Holds(RootKind::Data))
    );
    let home = json!({"pattern": "x", "path": "/mnt/home"});
    assert!(decide_plain(&fs, "Grep", &home).is_some());
}

#[test]
fn a_path_the_fence_cannot_vouch_for_is_refused() {
    let fs = disk(&[
        ("/Users/a/Matters/Smith/bad", Fake::BadLink),
        (
            "/Users/a/Matters/Smith/server",
            Fake::Link("/net/evil/share".into()),
        ),
    ]);
    let network = json!({"file_path": "/net/evil/share/x"});
    assert_eq!(decide_plain(&fs, "Read", &network), Some(Denial::Network));
    for path in [
        "/Users/a/Matters/Smith/bad/x",
        "/Users/a/Matters/Smith/server/x",
    ] {
        let input = json!({"file_path": path});
        assert_eq!(
            decide_plain(&fs, "Read", &input),
            Some(Denial::Unvouched),
            "{path}"
        );
    }
    assert!(
        fs.touched().iter().all(|path| !path.starts_with("/net")),
        "{:?}",
        fs.touched()
    );
}

#[test]
fn a_checked_tool_without_its_path_is_refused_and_another_tool_is_left_alone() {
    let fs = disk(&[]);
    for tool in ["Read", "Edit", "Write", "MultiEdit", "NotebookEdit", "Glob"] {
        assert_eq!(
            decide_plain(&fs, tool, &json!({})),
            Some(Denial::Broken),
            "{tool}"
        );
        assert_eq!(
            decide_plain(&fs, tool, &json!({"file_path": 3, "pattern": 3})),
            Some(Denial::Broken),
            "{tool}"
        );
    }
    let state = json!({"file_path": format!("{DATA}/state.sqlite"), "command": "cat x"});
    for tool in ["Bash", "PowerShell", "WebFetch", "mcp__x__y"] {
        assert!(!FENCED_TOOLS.contains(&tool));
        assert_eq!(decide_plain(&fs, tool, &state), None, "{tool}");
    }
}

#[test]
fn every_refusal_holds_the_words_the_refusal_detector_reads() {
    let kinds = [RootKind::Data, RootKind::Matter, RootKind::Protected];
    let mut all: Vec<Denial> = kinds
        .iter()
        .flat_map(|kind| [Denial::Fenced(*kind), Denial::Holds(*kind)])
        .collect();
    all.extend([
        Denial::Unbounded,
        Denial::Unvouched,
        Denial::Broken,
        Denial::Late,
    ]);
    for denial in all {
        let reason = denial.reason().to_lowercase();
        assert!(reason.contains("has been denied"), "{denial:?}");
        assert!(reason.contains("your permission settings"), "{denial:?}");
        assert!(
            !reason.contains('/') || reason.contains("a `..`"),
            "{denial:?}: no path"
        );
    }
}

#[test]
fn windows_spellings_of_a_drive_are_each_read() {
    let net = plain_net(true);
    let fs = FakeFs::default();
    let real = |_: &Path| None;
    let disk = Disk {
        fs: &fs,
        real: &real,
        net: &net,
    };
    let ground = Ground {
        cwd: Path::new("C:\\m"),
        home: None,
        shares: &[],
        disk: &disk,
    };
    let read = |text: &str| readings(text, &ground);
    assert_eq!(
        read("/c/Users/a").first(),
        Some(&PathBuf::from("C:\\Users\\a"))
    );
    assert_eq!(read("//c/Users/a"), vec![PathBuf::from("C:\\Users\\a")]);
    assert_eq!(read("/cd/x").len(), 1);
}
