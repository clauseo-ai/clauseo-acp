//! `holds_now`: whether the folder holds one file now, by the checkpoint walk's rules and its
//! exact name, with nothing opened and no history read.

use clauseo_matters::{Limits, holds_now};

use crate::fixture::{Fixture, path};
use crate::support;

#[test]
fn a_file_the_walk_would_find_is_held_and_a_missing_one_is_not() {
    let mut fx = Fixture::new("holds-now-plain");
    fx.limits = Limits {
        file_cap_bytes: 4,
        import_warn_bytes: 1_000_000,
    };
    fx.write("brief.txt", "b");
    fx.write("pleadings/claim.docx", "c");
    let matter = fx.import();
    fx.write("big.bin", [7; 20]);
    fx.write("cloud.docx", "cloud");
    fx.probe.mark(fx.file("cloud.docx"));
    for held in ["brief.txt", "pleadings/claim.docx", "big.bin", "cloud.docx"] {
        assert!(holds_now(&matter, &path(held)).unwrap(), "{held}");
    }
    for missing in ["gone.txt", "pleadings/gone.docx", "nowhere/claim.docx"] {
        assert!(!holds_now(&matter, &path(missing)).unwrap(), "{missing}");
    }
    support::remove_file(fx.file("brief.txt"));
    assert!(!holds_now(&matter, &path("brief.txt")).unwrap());
}

#[test]
fn an_excluded_path_is_never_held() {
    let fx = Fixture::new("holds-now-excluded");
    fx.write("brief.txt", "b");
    let matter = fx.import();
    for excluded in [
        ".claude/settings.json",
        ".codex/config.toml",
        ".git/HEAD",
        ".clauseo/matter.json",
        "~$brief.docx",
        "notes.tmp",
    ] {
        fx.write(excluded, "x");
        assert!(!holds_now(&matter, &path(excluded)).unwrap(), "{excluded}");
    }
}

#[cfg(unix)]
#[test]
fn a_link_or_a_file_under_a_linked_folder_is_not_held() {
    let fx = Fixture::new("holds-now-links");
    fx.write("brief.txt", "b");
    let matter = fx.import();
    let outside = fx.dir.path().join("outside");
    support::create_dir_all(&outside);
    support::write(outside.join("note.txt"), "n");
    support::symlink(outside.join("note.txt"), fx.file("linked.txt"));
    support::symlink(&outside, fx.file("linked-folder"));
    support::create_dir_all(fx.file("real"));
    support::symlink(fx.file("brief.txt"), fx.file("real/brief.txt"));
    for link in ["linked.txt", "linked-folder/note.txt", "real/brief.txt"] {
        assert!(!holds_now(&matter, &path(link)).unwrap(), "{link}");
    }
}

#[test]
fn a_file_spelled_otherwise_is_not_held_by_the_name_the_history_has() {
    let fx = Fixture::new("holds-now-case");
    fx.write("Memo.txt", "m");
    fx.write("Drafts/one.txt", "o");
    let matter = fx.import();
    assert!(holds_now(&matter, &path("Memo.txt")).unwrap());
    assert!(!holds_now(&matter, &path("memo.txt")).unwrap());
    assert!(!holds_now(&matter, &path("drafts/one.txt")).unwrap());
    assert!(!holds_now(&matter, &path("Drafts/One.txt")).unwrap());
}

#[test]
fn a_folder_is_not_a_file() {
    let fx = Fixture::new("holds-now-folder");
    fx.write("pleadings/claim.docx", "c");
    let matter = fx.import();
    assert!(!holds_now(&matter, &path("pleadings")).unwrap());
    assert!(!holds_now(&matter, &path("pleadings/claim.docx/x")).unwrap());
}

#[cfg(windows)]
#[test]
fn parts_windows_reads_otherwise_are_never_held_on_windows() {
    let fx = Fixture::new("holds-now-windows");
    fx.write("brief.txt", "b");
    fx.write("sub/brief.txt", "b");
    let matter = fx.import();
    assert!(holds_now(&matter, &path("brief.txt")).unwrap());
    for asked in [
        "brief.txt.",
        "brief.txt ",
        "brief.txt:stream",
        "sub/C:brief.txt",
        "CON",
        "sub/NUL.txt",
    ] {
        assert!(!holds_now(&matter, &path(asked)).unwrap(), "{asked}");
    }
}
