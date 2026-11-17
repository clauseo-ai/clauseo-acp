//! The paths a delete or a move names count as the turn's writes (`shell_writes::deletes`):
//! `rm`, `unlink`, both sides of `mv`, PowerShell's `Remove-Item`, `del` and `Move-Item`, with
//! quoted paths, spaces and Windows paths; options and pattern values never count.

use std::collections::BTreeSet;

use super::super::deletes::removed;
use super::super::{Dialect, add_script};

fn posix(script: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    add_script(&mut out, Some(""), script, Dialect::Posix);
    out.into_iter().collect()
}

fn powershell(script: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    add_script(&mut out, Some(""), script, Dialect::PowerShell);
    out.into_iter().collect()
}

#[test]
fn rm_and_unlink_name_what_they_delete() {
    assert_eq!(posix("rm Memo.docx"), ["Memo.docx"]);
    assert_eq!(
        posix(r#"rm -f "Brief to counsel.docx" 'Old notes.txt'"#),
        ["Brief to counsel.docx", "Old notes.txt"]
    );
    assert_eq!(posix("rm -rf Drafts/old"), ["Drafts/old"]);
    assert_eq!(posix("rm -- -odd.txt"), ["-odd.txt"]);
    assert_eq!(posix("unlink scratch.txt"), ["scratch.txt"]);
    assert_eq!(posix("cd Sub && rm a.txt"), ["Sub/a.txt"]);
    assert_eq!(posix("/bin/rm a.txt; sudo rm b.txt"), ["a.txt", "b.txt"]);
    // A glob or an expansion leaves the target unknown.
    assert_eq!(posix("rm *.docx"), Vec::<String>::new());
    assert_eq!(posix("rm \"$F\""), Vec::<String>::new());
    // Listing or searching for the name deletes nothing.
    assert_eq!(posix("ls rm Memo.docx"), Vec::<String>::new());
}

#[test]
fn mv_names_both_sides() {
    assert_eq!(posix("mv a.docx b.docx"), ["a.docx", "b.docx"]);
    assert_eq!(
        posix(r#"mv -f "Draft v1.docx" "Archive/Draft v1.docx""#),
        ["Archive/Draft v1.docx", "Draft v1.docx"]
    );
    assert_eq!(posix("mv -S .bak a.txt b.txt"), ["a.txt", "b.txt"]);
}

#[test]
fn powershell_remove_item_del_and_move_item_name_their_paths() {
    assert_eq!(
        powershell(r"Remove-Item -Path 'C:\Matters\Acme\Old brief.docx' -Force"),
        ["C:/Matters/Acme/Old brief.docx"]
    );
    assert_eq!(
        powershell(r#"Remove-Item -LiteralPath "Drafts\Memo v2.docx""#),
        ["Drafts/Memo v2.docx"]
    );
    assert_eq!(powershell("del notes.txt"), ["notes.txt"]);
    assert_eq!(powershell("rm -Recurse -Force Scans"), ["Scans"]);
    assert_eq!(
        powershell(r"remove-item.exe -Filter *.tmp -Path C:\Matters\Acme"),
        ["C:/Matters/Acme"]
    );
    assert_eq!(
        powershell(r"Move-Item -Path 'a b.docx' -Destination 'Archive\a b.docx'"),
        ["Archive/a b.docx", "a b.docx"]
    );
    assert_eq!(powershell("Get-Item notes.txt"), Vec::<String>::new());
}

#[test]
fn the_reader_gives_the_paths_index_by_index() {
    assert_eq!(removed("rm", &["-rf", "a", "b"]), [1, 2]);
    assert_eq!(removed("RM.EXE", &["a"]), [0]);
    assert_eq!(removed(r"C:\Windows\del", &["a"]), [0]);
    assert_eq!(removed("Remove-Item", &["-Include", "*.tmp", "x"]), [2]);
    assert_eq!(removed("cat", &["a"]), Vec::<usize>::new());
    assert_eq!(removed::<&str>("rm", &[]), Vec::<usize>::new());
}
