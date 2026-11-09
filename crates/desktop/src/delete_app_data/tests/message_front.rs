//! Every message box the installer and uninstaller show comes to the front: on the VM, the box
//! after a failed Safe workspace set-up opened behind Explorer with only a taskbar button. Every
//! `MessageBox` in the repo's own scripts (`windows/*.nsh`) and in the template's named blocks
//! carries `MB_SETFOREGROUND|MB_TOPMOST`, and the template's upstream text keeps no box of its
//! own: a box added anywhere without them fails here.

use super::sources::{manifest_dir, read};
use super::template::{TEMPLATE, split_blocks};

/// The flags every box carries.
const FRONT: [&str; 2] = ["MB_SETFOREGROUND", "MB_TOPMOST"];

/// Whether the box on `line` comes to the front: its flags, the word after `MessageBox`, hold
/// both of `FRONT`.
fn in_front(line: &str) -> bool {
    let flags = line.split_whitespace().nth(1).unwrap_or("");
    FRONT
        .iter()
        .all(|flag| flags.split('|').any(|have| have == *flag))
}

/// The code lines of `text` that show a box, trimmed.
fn boxes(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with(';') && line.contains("MessageBox"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn in_front_reads_the_flags_word() {
    assert!(in_front(
        "MessageBox MB_OK|MB_ICONSTOP|MB_SETFOREGROUND|MB_TOPMOST \"$(x)\" /SD IDOK"
    ));
    assert!(!in_front("MessageBox MB_OK|MB_ICONEXCLAMATION \"$(x)\""));
    assert!(!in_front(
        "MessageBox MB_OK|MB_SETFOREGROUND \"MB_TOPMOST\""
    ));
}

#[test]
fn every_box_in_the_repos_scripts_comes_to_the_front() {
    let mut files: Vec<_> = std::fs::read_dir(manifest_dir().join("windows"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "nsh"))
        .collect();
    files.sort();
    // hooks.nsh, admin_copy.nsh, admin_elevated.nsh, safe_temp.nsh, uninstall_in_place.nsh.
    assert!(files.len() >= 5, "{files:?}");
    let mut seen = 0;
    for path in &files {
        for line in boxes(&std::fs::read_to_string(path).unwrap()) {
            assert!(in_front(&line), "{}: {line}", path.display());
            seen += 1;
        }
    }
    // The set-up's outcomes (10 in admin_copy.nsh), the uninstall's (6 in hooks.nsh), the
    // temporary folder's (2) and the in-place check's (1).
    assert!(seen >= 19, "{seen}");
    // The box after a failed set-up, the one the VM saw behind Explorer.
    let admin = read("windows/admin_copy.nsh");
    assert!(
        boxes(&admin).contains(
            &"MessageBox MB_OK|MB_ICONEXCLAMATION|MB_SETFOREGROUND|MB_TOPMOST \"$(adminCopyFailed)\""
                .to_owned()
        ),
        "admin_copy.nsh"
    );
}

#[test]
fn every_box_in_the_template_is_in_a_named_block_and_comes_to_the_front() {
    let (blocks, kept) = split_blocks(&read(TEMPLATE));
    let upstream = boxes(&kept.join("\n"));
    assert!(upstream.is_empty(), "{upstream:?}");
    let mut seen = Vec::new();
    for block in &blocks {
        for line in boxes(&block.lines.join("\n")) {
            assert!(in_front(&line), "{}: {line}", block.name);
            seen.push(block.name.as_str());
        }
    }
    assert_eq!(
        seen,
        ["unable-to-uninstall-front", "silent-downgrades-front"]
    );
}
