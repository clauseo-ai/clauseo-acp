use super::{
    Held, apfs_key, as_listed_by, bounds, held_files_with, holds, named_in, raw_text,
    stored_spelling,
};
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
        let held = held_files_with(&folder.0, share_modes);
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
    let held = held_files_with(&folder.0, true);
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

/// A held file found by a path the agent spelled in another case or Unicode form is shown as the
/// matter lists it, on APFS's rules, run on every system: the exact path first, the folded one
/// next, the agent's spelling when none is listed; with no folding (ext4) nothing is mapped.
#[test]
fn a_held_file_found_in_another_spelling_is_shown_as_listed() {
    let path = |text: &str| {
        clauseo_protocol::MatterPath::from_relative(std::path::Path::new(text)).unwrap()
    };
    let held = [
        Held::new(path("Drafts/Contract.docx")),
        Held::new(path("P\u{e9}tition.docx")),
        Held::new(path("CONTRACT.DOCX")),
    ];
    for (typed, shown) in [
        ("drafts/CONTRACT.docx", "Drafts/Contract.docx"),
        ("Pe\u{301}tition.docx", "P\u{e9}tition.docx"),
        ("PE\u{301}TITION.DOCX", "P\u{e9}tition.docx"),
        ("CONTRACT.DOCX", "CONTRACT.DOCX"),
        ("Free.docx", "Free.docx"),
    ] {
        assert_eq!(
            as_listed_by(path(typed), &held, apfs_key),
            path(shown),
            "{typed}"
        );
    }
    assert_eq!(
        as_listed_by(path("Pe\u{301}tition.docx"), &held, str::to_owned),
        path("Pe\u{301}tition.docx")
    );
}

/// A held file's stored spelling is read from the folders on its way, on APFS's rules, run on
/// every system: each name as listed, the exact one first; `None` for a name no folder lists.
#[test]
fn a_held_file_found_by_a_tool_is_shown_as_its_folders_list_it() {
    let path = |text: &str| {
        clauseo_protocol::MatterPath::from_relative(std::path::Path::new(text)).unwrap()
    };
    let root = std::env::temp_dir().join(format!("clauseo-host-stored-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    write::create_dir_all(&root.join("Drafts")).unwrap();
    for name in ["Drafts/Contract.docx", "P\u{e9}tition.docx", "Free.docx"] {
        write::write_file(&root.join(name), b"x").unwrap();
    }
    for (typed, shown) in [
        ("drafts/CONTRACT.docx", Some("Drafts/Contract.docx")),
        ("Pe\u{301}tition.docx", Some("P\u{e9}tition.docx")),
        ("FREE.DOCX", Some("Free.docx")),
        ("Free.docx", Some("Free.docx")),
        ("Missing.docx", None),
    ] {
        assert_eq!(
            stored_spelling(&root, &path(typed), apfs_key),
            shown.map(path),
            "{typed}"
        );
    }
    assert_eq!(
        stored_spelling(&root, &path("FREE.DOCX"), str::to_owned),
        None
    );
    drop(write::remove_dir_all(&root));
}
