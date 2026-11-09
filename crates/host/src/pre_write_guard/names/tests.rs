use std::cell::RefCell;
use std::path::{Path, PathBuf};

use super::{bounds, held_files_with, holds, named_in, raw_text};
use crate::network_paths::Access;
use crate::network_paths::no_follow::{MatterRoot, NoFollow, open_path};
use crate::network_paths::tests::plain_net;
use crate::pre_write_guard::timings;
use crate::pre_write_guard::words::Escape;
use crate::write;

/// A matter folder of `Contract.docx` (held: `~$ntract.docx` beside it), `Free.docx` and 200
/// `Doc N.txt` files, in a fresh temporary folder; removed on drop.
struct Folder(std::path::PathBuf);

impl Folder {
    fn new(test: &str) -> Self {
        // Owner files planted here stay held through the stale rule: no Word runs in a test.
        clauseo_matters::test_support::pretend_word_running_since_epoch();
        let root =
            std::env::temp_dir().join(format!("clauseo-host-walk-{test}-{}", std::process::id()));
        write::remove_dir_all(&root).unwrap();
        write::create_dir_all(&root).unwrap();
        let docs = (0..200).map(|n| format!("Doc {n}.txt"));
        for name in ["Contract.docx", "~$ntract.docx", "Free.docx"]
            .map(str::to_owned)
            .into_iter()
            .chain(docs)
        {
            write::write_file(&root.join(name), b"x").unwrap();
        }
        Self(root)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        drop(write::remove_dir_all(&self.0));
    }
}

/// The root of the local matter `folder`.
fn local(folder: &Path) -> MatterRoot<'static> {
    MatterRoot::new(folder, &plain_net(cfg!(windows)))
}

/// A Windows matter on a share (`\\server\deals\Acme`, its root as stored): the scan opens that
/// root by the share-matter exception's rule, through the seam (CI mounts no share), and every
/// folder below it with no link followed from that handle, so a document held open on the share
/// is found. A root the rule does not admit is never opened nor canonicalised: nothing is found.
#[test]
fn a_share_matters_held_files_are_found_from_its_root_as_stored() {
    let folder = Folder::new("share");
    write::create_dir_all(&folder.0.join("sub")).unwrap();
    write::write_file(&folder.0.join("sub/Contract.docx"), b"x").unwrap();
    write::write_file(&folder.0.join("sub/~$ntract.docx"), b"x").unwrap();
    let asked: RefCell<Vec<PathBuf>> = RefCell::new(Vec::new());
    let open = |path: &Path| -> std::io::Result<NoFollow> {
        asked.borrow_mut().push(path.to_path_buf());
        open_path(&dunce::canonicalize(&folder.0)?, Access::List)
    };
    let net = plain_net(true);
    for stored in [r"\\server\deals\Acme", r"\\server\deals\Acme\", r"Z:\Acme"] {
        let net = crate::network_paths::NetEnv {
            remote_drive: Box::new(|letter| letter == 'Z'),
            ..plain_net(true)
        };
        asked.borrow_mut().clear();
        let held = held_files_with(&MatterRoot::with(Path::new(stored), &net, &open), false);
        let mut paths: Vec<&str> = held.iter().map(|file| file.path.as_str()).collect();
        paths.sort_unstable();
        assert_eq!(paths, ["Contract.docx", "sub/Contract.docx"], "{stored}");
        // The root as stored, once to start and once to list it; never another text.
        assert!(!asked.borrow().is_empty());
        assert!(
            asked.borrow().iter().all(|path| path == Path::new(stored)),
            "{stored}"
        );
    }
    for refused in [
        r"\\server\deals\..\Acme",
        r"\\server\deals\.\Acme",
        r"\\server\deals\Acme.",
        r"\\server\\deals\Acme",
        r"\\?\UNC\server\deals\Acme",
        r"\\.\pipe\Acme",
        "//server/deals/Acme",
    ] {
        asked.borrow_mut().clear();
        let held = held_files_with(&MatterRoot::with(Path::new(refused), &net, &open), false);
        assert!(held.is_empty(), "{refused}");
        assert!(asked.borrow().is_empty(), "{refused}");
    }
    // With Unix's rules a share's mount is a local folder: the exception never applies.
    #[cfg(unix)]
    {
        asked.borrow_mut().clear();
        let unix = MatterRoot::with(Path::new(r"\\server\deals\Acme"), &plain_net(false), &open);
        assert!(held_files_with(&unix, false).is_empty(), "Unix's rules");
        assert!(asked.borrow().is_empty());
    }
}

/// A share root the system opened as a link itself (a name-surrogate tag on its handle on
/// Windows; here a symlink opened as itself, through the seam) is refused: nothing is listed
/// below it, at the start or when the root is opened again for its own listing.
#[cfg(target_os = "linux")]
#[test]
fn a_share_root_whose_handle_is_a_link_is_refused() {
    let folder = Folder::new("share-link");
    let link = folder.0.with_extension("link");
    drop(write::remove_file(&link));
    write::symlink(&folder.0, &link).unwrap();
    let net = plain_net(true);
    let stored = Path::new(r"\\server\deals\Acme");
    let as_link = |_: &Path| NoFollow::link_itself(&link);
    let held = held_files_with(&MatterRoot::with(stored, &net, &as_link), false);
    assert!(held.is_empty(), "a root that is a link");
    // The same seam answering with the folder itself finds the held file.
    let as_folder = |_: &Path| open_path(&dunce::canonicalize(&folder.0)?, Access::List);
    let held = held_files_with(&MatterRoot::with(stored, &net, &as_folder), false);
    assert_eq!(held.len(), 1);
    // The root opened again for its own listing is checked too.
    let calls = std::cell::Cell::new(0);
    let link_second = |_: &Path| {
        calls.set(calls.get() + 1);
        if calls.get() == 1 {
            open_path(&dunce::canonicalize(&folder.0)?, Access::List)
        } else {
            NoFollow::link_itself(&link)
        }
    };
    let root = MatterRoot::with(stored, &net, &link_second);
    let error = root.open(Path::new(""), Access::List).unwrap_err();
    assert!(
        crate::network_paths::no_follow::is_link_on_the_way(&error),
        "{error}"
    );
    write::remove_file(&link).unwrap();
}

/// The counts at the end of the timings line `line`: `checked` and `probed`.
fn counts(line: &str) -> (usize, usize) {
    let number = |key: &str| {
        line.split_whitespace()
            .find_map(|part| part.strip_prefix(key))
            .unwrap()
            .parse()
            .unwrap()
    };
    (number("checked="), number("probed="))
}

/// The walk looks up on disk only the file the owner file points at, not the 201 others beside
/// it; nothing more is looked up when the command names none of them.
#[test]
fn the_walk_looks_up_only_what_an_owner_file_may_point_at() {
    for share_modes in [false, true] {
        let folder = Folder::new(&format!("only-{share_modes}"));
        timings::start(true);
        let held = held_files_with(&local(&folder.0), share_modes);
        assert_eq!(
            named_in("echo hi > new.txt", Escape::Backslash, &held),
            None
        );
        let found = named_in("cat Contract.docx", Escape::Backslash, &held);
        assert_eq!(found.map(|file| file.path.as_str()), Some("Contract.docx"));
        let line = timings::finish().unwrap();
        assert_eq!(counts(&line), (202, 1), "share modes {share_modes}: {line}");
        // Only the held file is known held; the rest are kept, unknown, only with share modes.
        assert_eq!(held.len(), if share_modes { 202 } else { 1 });
    }
}

/// With share modes (Windows), a file no owner file points at is opened the first time a command
/// names it, by name or wildcard, and only then; the answer is kept.
#[test]
fn with_share_modes_a_named_file_is_looked_up_once_when_named() {
    let folder = Folder::new("lazy");
    timings::start(true);
    let held = held_files_with(&local(&folder.0), true);
    for command in ["cat Free.docx", "cat Free.docx", "rm Doc\\ 7.txt"] {
        assert_eq!(
            named_in(command, Escape::Backslash, &held),
            None,
            "{command}"
        );
    }
    let (_, probed) = counts(&timings::finish().unwrap());
    assert_eq!(probed, 3, "Contract.docx, Free.docx once, Doc 7.txt");
    timings::start(true);
    // A wildcard opens what it matches until a held file: none of the 200 is held.
    assert_eq!(named_in("rm Doc*.txt", Escape::Backslash, &held), None);
    let (_, probed) = counts(&timings::finish().unwrap());
    assert_eq!(probed, 199, "every Doc file but Doc 7.txt, known already");
}

#[test]
fn a_name_counts_only_as_a_whole_name() {
    for text in [
        "cat Contract.docx",
        "cat ./Contract.docx",
        r"type C:\m\Contract.docx",
        "x>Contract.docx",
        "cp a 'Contract.docx'",
        "--out=Contract.docx",
        "-Path:Contract.docx",
        "cat Contract.docx;ls",
        "Contract.docx",
        "(Contract.docx)",
    ] {
        assert!(holds(text, "Contract.docx"), "{text}");
    }
    for text in [
        "cat Contract.docx.bak",
        "cat MyContract.docx",
        "cat Contract.docx/x",
        "cat Contract",
        "cat Contract.docxx",
    ] {
        assert!(!holds(text, "Contract.docx"), "{text}");
    }
    assert!(!holds("anything", ""));
    assert!(bounds('/', true) && !bounds('/', false));
}

#[test]
fn the_raw_text_joins_lines_and_decodes_ansi_c_quotes() {
    assert_eq!(
        raw_text(
            "echo > Con\\\ntract.docx $'Con\\x74ract.docx'",
            Escape::Backslash
        ),
        "echo > Contract.docx Contract.docx"
    );
    assert_eq!(
        raw_text("Set-Content Con`\ntract.docx $'a'", Escape::Backtick),
        "Set-Content Contract.docx $'a'"
    );
}
