use super::{is_excluded, is_excluded_any, is_excluded_file, is_excluded_path};

#[test]
fn excluded_file_names_in_any_case() {
    for name in [
        "~$Contract.docx",
        "~$X",
        "a.tmp",
        "A.TMP",
        "~WRL0001.tmp",
        ".clauseo-3.tmp",
        "Thumbs.db",
        "THUMBS.DB",
        "desktop.ini",
        "Desktop.INI",
        ".DS_Store",
        ".ds_store",
        ".~lock.Contract.docx#",
        ".~LOCK.x#",
        ".~lock.#",
        ".git",
        ".GIT",
        ".clauseo",
        ".Clauseo",
    ] {
        assert!(is_excluded_file(name), "{name} should be excluded");
    }
}

#[test]
fn near_misses_are_kept() {
    for name in [
        "a.tmpx",
        "x~$",
        "tmp",
        "Thumbs.db.bak",
        "my desktop.ini",
        ".DS_Store2",
        ".~lock.x",
        ".gitignore",
        ".git2",
        "clauseo",
        "Contract.docx",
        "é.tmp.docx",
    ] {
        assert!(!is_excluded_file(name), "{name} should be kept");
    }
}

#[test]
fn only_git_and_clauseo_exclude_folders() {
    assert!(is_excluded_any(".git"));
    assert!(is_excluded_any(".CLAUSEO"));
    assert!(!is_excluded_any("~$x"));
    assert!(!is_excluded_any("x.tmp"));
    assert!(!is_excluded_any("Thumbs.db"));
    assert!(!is_excluded_any(".github"));
}

#[test]
fn a_path_is_excluded_by_any_folder_on_the_way_or_its_file_name() {
    assert!(is_excluded_path(".git/config"));
    assert!(is_excluded_path("a/.Clauseo/b.txt"));
    assert!(is_excluded_path("a/~$memo.docx"));
    assert!(is_excluded_path("Thumbs.db"));
    // File-name rules do not apply to folders.
    assert!(!is_excluded_path("x.tmp/a.txt"));
    assert!(!is_excluded_path("a/b.txt"));
    assert!(is_excluded("x.tmp", false));
    assert!(!is_excluded("x.tmp", true));
    assert!(is_excluded(".git", true));
}
