use super::{is_excluded_file, is_excluded_folder, is_excluded_name, is_excluded_path};

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
fn every_dot_folder_is_excluded_and_no_other() {
    for name in [
        ".git", ".CLAUSEO", ".github", ".claude", ".codex", ".Agents", ".", ".x y",
    ] {
        assert!(is_excluded_folder(name), "{name} should be excluded");
    }
    for name in [
        "~$x",
        "x.tmp",
        "Thumbs.db",
        "claude",
        "a.claude",
        "Contract",
    ] {
        assert!(!is_excluded_folder(name), "{name} should be kept");
    }
}

#[test]
fn a_dot_folder_excludes_everything_under_it_at_any_depth() {
    assert!(is_excluded_path(".claude/settings.local.json"));
    assert!(is_excluded_path("a/.codex/x"));
    assert!(is_excluded_path(".Agents/skills/a.md"));
    assert!(is_excluded_path(".github/workflows/ci.yml"));
    // Dot-files are papers, at any depth; only the named ones are not.
    assert!(!is_excluded_path(".gitignore"));
    assert!(!is_excluded_path("a/.env"));
    assert!(!is_excluded_path(".claude"));
    assert!(is_excluded_path("a/.DS_Store"));
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
    assert!(is_excluded_name("x.tmp", false));
    assert!(!is_excluded_name("x.tmp", true));
    assert!(is_excluded_name(".git", true));
    assert!(is_excluded_name(".claude", true));
    assert!(!is_excluded_name(".claude", false));
}
