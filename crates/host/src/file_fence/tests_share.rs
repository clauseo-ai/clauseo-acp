//! The fence and matters kept on a network share (UNC, a mapped drive, an SMB volume), and the
//! one switch for network paths outside every matter: what counts as network, read from text
//! and the system's lists, and never touched.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::decide::{Denial, Fence, decide_as};
use super::real::Ground;
use super::tests_decide::{MATTER, data_root, other_root};
use super::{FenceFile, FencedRoot, FileId, RootKind};
use crate::network_paths::NetEnv;
use crate::network_paths::resolve::Disk;
use crate::network_paths::resolve::tests::FakeFs;
use crate::network_paths::tests::{mac_net, plain_net};

const UNC: &str = r"\\server\share\Matters\A";
const MAPPED: &str = r"Z:\Matters\A";
const VOLUME: &str = "/Volumes/share/A";

/// Windows' rules with `Z:` a network drive.
fn windows_net() -> NetEnv {
    NetEnv {
        windows: true,
        fold_case: true,
        remote_mounts: Vec::new(),
        remote_drive: Box::new(|letter| letter == 'Z'),
        macos: false,
        firmlinks: Some(Vec::new()),
    }
}

/// The decision for `tool` with `input` in the session whose matter folder is `matter`, the
/// working folder too, under `net`, with `roots` fenced beside the data folder and another
/// matter, the network rule `refuse_network`; and whether the disk was touched.
fn in_matter(
    (net, matter, roots): (&NetEnv, &str, Vec<FencedRoot>),
    refuse_network: bool,
    tool: &str,
    input: &Value,
) -> (Option<Denial>, bool) {
    let fs = FakeFs::default();
    let mut all = vec![data_root(), other_root()];
    all.extend(roots);
    let file = FenceFile {
        version: 1,
        roots: all,
    };
    let identity = Box::new(|_: &Path| -> Option<FileId> { None });
    let fence = Fence::with_rules(&file, (true, net.windows), identity);
    let shares = super::guard::shares(&file, Some(Path::new(matter)));
    let real = |_: &Path| None;
    let disk = Disk {
        fs: &fs,
        real: &real,
        net,
    };
    let ground = Ground {
        cwd: Path::new(matter),
        home: None,
        shares: &shares,
        disk: &disk,
    };
    let decided = decide_as(&fence, tool, input, &ground, refuse_network);
    let touched = fs.touched().iter().any(|path| net.is_network(path));
    (decided, touched)
}

/// [`in_matter`] with no more roots and network paths outside the matters refused.
fn in_share_matter(
    net: &NetEnv,
    matter: &str,
    tool: &str,
    input: &Value,
) -> (Option<Denial>, bool) {
    in_matter((net, matter, Vec::new()), true, tool, input)
}

/// Every call inside the share matter, absolute and relative, for each file tool.
fn calls_inside(matter: &str, separator: &str) -> Vec<(&'static str, Value)> {
    let inside = |name: &str| format!("{matter}{separator}{name}");
    vec![
        ("Read", json!({"file_path": inside("brief.docx")})),
        ("Read", json!({"file_path": "brief.docx"})),
        (
            "Edit",
            json!({"file_path": inside("Drafts") + separator + "spa.docx"}),
        ),
        (
            "Edit",
            json!({"file_path": format!("Drafts{separator}spa.docx")}),
        ),
        ("Write", json!({"file_path": "new.docx"})),
        ("Grep", json!({"pattern": "lease", "path": matter})),
        (
            "Grep",
            json!({"pattern": "lease", "path": inside("Drafts")}),
        ),
        ("Grep", json!({"pattern": "lease"})),
        ("Glob", json!({"pattern": "**/*.docx"})),
        (
            "Glob",
            json!({"pattern": "*.docx", "path": inside("Drafts")}),
        ),
        (
            "Glob",
            json!({"pattern": format!("Drafts{separator}*.docx")}),
        ),
    ]
}

/// A matter kept on a share (a UNC path, a mapped drive, an SMB volume on macOS) works for every
/// file tool inside it, by its text alone: nothing on the share is touched.
#[test]
fn every_file_tool_works_inside_a_matter_kept_on_a_share() {
    let windows = windows_net();
    let mac = mac_net(&["/Volumes/share"]);
    let cases = [
        (&windows, UNC, r"\"),
        (&windows, MAPPED, r"\"),
        (&mac, VOLUME, "/"),
    ];
    for (net, matter, separator) in cases {
        for (tool, input) in calls_inside(matter, separator) {
            let (decided, touched) = in_share_matter(net, matter, tool, &input);
            assert_eq!(decided, None, "{matter} {tool} {input}");
            assert!(!touched, "{matter} {tool} {input}: the share was touched");
        }
    }
}

/// The share-matter rule is strict text: another spelling of the same share, a `.` or `..`
/// name or a name Windows trims (a trailing dot) is a network path an agent named, which the
/// fence can't vouch for, so it refuses it, untouched.
#[test]
fn another_spelling_of_a_share_matter_is_refused_as_unvouched() {
    let windows = windows_net();
    let mac = mac_net(&["/Volumes/share"]);
    let unvouched = Some(Denial::Unvouched);
    let cases = [
        (&windows, MAPPED, r"\\server\share\Matters\A\brief.docx"),
        (&windows, UNC, r"Z:\Matters\A\brief.docx"),
        (&windows, UNC, r"\\server\share\Matters\A\.\brief.docx"),
        (
            &windows,
            UNC,
            r"\\server\share\Matters\A\Drafts\..\brief.docx",
        ),
        (&windows, MAPPED, r"Z:\Matters\A\..\A\brief.docx"),
        (&windows, UNC, r"\\server\share\Matters\A\brief.docx."),
        (&windows, MAPPED, r"Z:\Matters\A\Drafts.\spa.docx"),
        (&mac, VOLUME, "/Volumes/share/A/./brief.docx"),
        (&mac, VOLUME, "/Volumes/share/A/Drafts/../brief.docx"),
    ];
    for (net, matter, path) in cases {
        for tool in ["Read", "Edit"] {
            let input = json!({"file_path": path});
            let (decided, touched) = in_share_matter(net, matter, tool, &input);
            assert_eq!(decided, unvouched, "{matter} {tool} {path}");
            assert!(!touched, "{matter} {path}: the share was touched");
        }
        let grep = json!({"pattern": "x", "path": path});
        assert_eq!(
            in_share_matter(net, matter, "Grep", &grep).0,
            unvouched,
            "{path}"
        );
    }
}

/// A matter on a share the session is invited into (denied to the writing tools alone) is read
/// by its text as the session's own is: Read, Grep and Glob go on, Edit is refused, nothing on
/// the share is touched; the fence file keeps the share root as written.
#[test]
fn an_invited_matter_on_a_share_lets_reads_through() {
    let windows = windows_net();
    let own = r"C:\Matters\A";
    for invited in [r"\\server\share\B", r"Z:\B"] {
        let root = FencedRoot::at(
            RootKind::Matter,
            &[Path::new(invited)],
            (vec![], vec!["**".into()]),
            &windows,
        );
        let root = root.unwrap();
        assert_eq!(
            (root.spellings.clone(), root.id.clone()),
            (vec![invited.to_owned()], None)
        );
        let on = |tool: &str, input: &Value| {
            in_matter((&windows, own, vec![root.clone()]), true, tool, input)
        };
        let file = json!({"file_path": format!(r"{invited}\Drafts\spa.docx")});
        assert_eq!(on("Read", &file), (None, false), "{invited}");
        assert_eq!(
            on("Grep", &json!({"pattern": "x", "path": invited})),
            (None, false)
        );
        assert_eq!(
            on("Glob", &json!({"pattern": "*.docx", "path": invited})),
            (None, false)
        );
        assert_eq!(
            on("Edit", &file),
            (Some(Denial::Fenced(RootKind::Matter)), false),
            "{invited}"
        );
        assert_eq!(on("Write", &file).0, Some(Denial::Fenced(RootKind::Matter)));
    }
}

/// The one switch: a network path outside every matter is refused while
/// `REFUSE_NETWORK_OUTSIDE_MATTERS` is on, and with it off goes on, judged by its text (another
/// matter's folder on a share still refused). Mapped drives, UNC paths, macOS SMB volumes and
/// Linux network mounts count, each told from text and the system's lists, never touched.
#[test]
fn network_paths_outside_the_matters_follow_the_one_switch() {
    let windows = windows_net();
    let mac = mac_net(&["/Volumes/share"]);
    let mut linux = plain_net(false);
    linux.remote_mounts = vec![PathBuf::from("/mnt/nfs")];
    let own = r"C:\Matters\A";
    let cases = [
        (&windows, own, r"\\server\docs\precedent.docx"),
        (&windows, own, r"Z:\Precedents\precedent.docx"),
        (&mac, MATTER, "/Volumes/share/Precedents/precedent.docx"),
        (&linux, MATTER, "/mnt/nfs/precedent.docx"),
    ];
    for (net, matter, path) in cases {
        for (tool, input) in [
            ("Read", json!({"file_path": path})),
            ("Grep", json!({"pattern": "x", "path": path})),
        ] {
            let on = in_matter((net, matter, Vec::new()), true, tool, &input);
            assert_eq!(on, (Some(Denial::Network), false), "{path} {tool}");
            let off = in_matter((net, matter, Vec::new()), false, tool, &input);
            assert_eq!(off, (None, false), "{path} {tool}");
        }
    }
    // A local volume is walked as before.
    let usb = json!({"file_path": "/Volumes/USB/x.docx"});
    assert_eq!(
        in_matter((&mac, MATTER, Vec::new()), true, "Read", &usb).0,
        None
    );
    // A drive whose type can't be read counts as network (`remote_drive` answers yes).
    let unknown = NetEnv {
        remote_drive: Box::new(|letter| letter != 'C'),
        ..windows_net()
    };
    let q = json!({"file_path": r"Q:\x.docx"});
    assert_eq!(
        in_matter((&unknown, own, Vec::new()), true, "Read", &q).0,
        Some(Denial::Network)
    );
    // Off, another matter's folder on a share is still refused by its text.
    let uninvited = FencedRoot::at(
        RootKind::Matter,
        &[Path::new(r"\\server\share\C")],
        (vec!["**".into()], vec![]),
        &windows,
    )
    .unwrap();
    let theirs = json!({"file_path": r"\\server\share\C\lease.pdf"});
    let off = in_matter(
        (&windows, own, vec![uninvited.clone()]),
        false,
        "Read",
        &theirs,
    );
    assert_eq!(off.0, Some(Denial::Fenced(RootKind::Matter)));
    let on = in_matter((&windows, own, vec![uninvited]), true, "Read", &theirs);
    assert_eq!(on.0, Some(Denial::Network));
}
