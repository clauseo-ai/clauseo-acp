//! What a delete or a move takes (`shell_writes::deletes`), through each reading: the paths a
//! delete names ([`add_deleted_input`]), a move's sources alone ([`add_moved_input`]), and both
//! sides with every write ([`add_changed_input`]); [`add_input`] keeps neither. POSIX and
//! PowerShell, quoted paths, spaces, apostrophes and Windows paths; options and pattern values
//! never count.

use std::collections::BTreeSet;

use super::super::deletes::{Taken, To, taken};
use super::super::{
    Dialect, Kind, add_changed_input, add_deleted_input, add_input, add_moved_input, add_script_as,
};

fn read(script: &str, dialect: Dialect, kind: Kind) -> Vec<String> {
    let mut out = BTreeSet::new();
    add_script_as(&mut out, Some(""), script, dialect, kind);
    out.into_iter().collect()
}

fn deleted(script: &str) -> Vec<String> {
    read(script, Dialect::Posix, Kind::Deletes)
}

fn moved(script: &str) -> Vec<String> {
    read(script, Dialect::Posix, Kind::MovedFrom)
}

fn changed(script: &str) -> Vec<String> {
    read(script, Dialect::Posix, Kind::Changes)
}

fn ps_moved(script: &str) -> Vec<String> {
    read(script, Dialect::PowerShell, Kind::MovedFrom)
}

fn ps_changed(script: &str) -> Vec<String> {
    read(script, Dialect::PowerShell, Kind::Changes)
}

fn ps_deleted(script: &str) -> Vec<String> {
    read(script, Dialect::PowerShell, Kind::Deletes)
}

fn none() -> Vec<String> {
    Vec::new()
}

#[test]
fn rm_and_unlink_name_what_they_delete() {
    assert_eq!(deleted("rm Memo.docx"), ["Memo.docx"]);
    assert_eq!(
        deleted(r#"rm -f "Brief to counsel.docx" 'Old notes.txt'"#),
        ["Brief to counsel.docx", "Old notes.txt"]
    );
    assert_eq!(deleted("rm -rf Drafts/old/"), ["Drafts/old"]);
    assert_eq!(deleted("rm -- -odd.txt"), ["-odd.txt"]);
    assert_eq!(deleted("unlink scratch.txt"), ["scratch.txt"]);
    assert_eq!(deleted("cd Sub && rm a.txt"), ["Sub/a.txt"]);
    assert_eq!(deleted("/bin/rm a.txt; sudo rm b.txt"), ["a.txt", "b.txt"]);
    // A glob or an expansion leaves the target unknown; a listing deletes nothing; a move is
    // no delete.
    assert_eq!(deleted("rm *.docx"), none());
    assert_eq!(deleted("rm \"$F\""), none());
    assert_eq!(deleted("ls rm Memo.docx"), none());
    assert_eq!(deleted("mv a.docx b.docx"), none());
}

/// `mv a.docx b.docx`: the source alone is moved away; the destination is changed too.
#[test]
fn mv_moves_its_sources_to_its_destination() {
    assert_eq!(moved("mv a.docx b.docx"), ["a.docx"]);
    assert_eq!(changed("mv a.docx b.docx"), ["a.docx", "b.docx"]);
    assert_eq!(
        moved(r#"mv -f "Draft v1.docx" "Archive/Draft v1.docx""#),
        ["Draft v1.docx"]
    );
    assert_eq!(moved("mv -S .bak a.txt b.txt"), ["a.txt"]);
    // Several sources go in the folder, each under its own name.
    assert_eq!(moved("mv a b Exhibits/"), ["a", "b"]);
    assert_eq!(
        changed("mv a b Exhibits/"),
        ["Exhibits/a", "Exhibits/b", "a", "b"]
    );
    assert_eq!(
        changed("mv a b Exhibits"),
        ["Exhibits/a", "Exhibits/b", "a", "b"]
    );
    assert_eq!(
        changed("mv -t Sub lawyer.docx"),
        ["Sub/lawyer.docx", "lawyer.docx"]
    );
    assert_eq!(
        moved("git mv 'Old name.docx' 'New name.docx'"),
        ["Old name.docx"]
    );
    assert_eq!(
        changed("git mv 'Old name.docx' 'New name.docx'"),
        ["New name.docx", "Old name.docx"]
    );
    // A delete is no move; one source alone moves nothing.
    assert_eq!(moved("rm a.docx"), none());
    assert_eq!(moved("mv a.docx"), none());
}

/// `add_input` keeps its behaviour: a move or a delete writes no content.
#[test]
fn the_write_reading_keeps_no_move_or_delete() {
    for (command, dialect) in [
        ("mv lawyer.docx tmp && mv tmp lawyer.docx", Dialect::Posix),
        ("rm Memo.docx", Dialect::Posix),
        (
            "Move-Item -Path a.docx -Destination b.docx",
            Dialect::PowerShell,
        ),
        ("Rename-Item a.docx b.docx", Dialect::PowerShell),
    ] {
        let input = serde_json::json!({ "command": command }).to_string();
        let mut out = BTreeSet::new();
        add_input(&mut out, &input, dialect, Some(""));
        assert_eq!(out.into_iter().collect::<Vec<_>>(), none(), "{command}");
    }
}

#[test]
fn powershell_move_item_and_rename_item_move_their_paths() {
    let lease = r"'Sharma v State 2024\O''Brien Lease.docx'";
    let script = format!(r"Move-Item -Path {lease} -Destination 'Archive\O''Brien Lease.docx'");
    assert_eq!(
        ps_moved(&script),
        ["Sharma v State 2024/O'Brien Lease.docx"]
    );
    assert_eq!(
        ps_changed(&script),
        [
            "Archive/O'Brien Lease.docx",
            "Sharma v State 2024/O'Brien Lease.docx"
        ]
    );
    assert_eq!(
        ps_moved("Move-Item -Path a.docx -Destination b.docx"),
        ["a.docx"]
    );
    assert_eq!(
        ps_changed("Move-Item -Path a.docx -Destination b.docx"),
        ["a.docx", "b.docx"]
    );
    // Positions, a named destination first, aliases, any case.
    assert_eq!(ps_moved("move-item a.docx b.docx"), ["a.docx"]);
    assert_eq!(ps_moved("mi -Destination b.docx -Force a.docx"), ["a.docx"]);
    assert_eq!(
        ps_changed("mv a.docx Archive\\"),
        ["Archive/a.docx", "a.docx"]
    );
    assert_eq!(
        ps_moved("MOVE -LiteralPath 'a b.docx' c.docx"),
        ["a b.docx"]
    );
    // A rename's new name stands beside its source.
    assert_eq!(ps_moved("Rename-Item a.docx b.docx"), ["a.docx"]);
    assert_eq!(
        ps_changed("Rename-Item a.docx b.docx"),
        ["a.docx", "b.docx"]
    );
    assert_eq!(
        ps_changed(r"Rename-Item -LiteralPath 'Drafts\Memo v1.docx' -NewName 'Memo v2.docx'"),
        ["Drafts/Memo v1.docx", "Drafts/Memo v2.docx"]
    );
    assert_eq!(
        ps_changed(r"ren Drafts\a.docx b.docx"),
        ["Drafts/a.docx", "Drafts/b.docx"]
    );
    // `cmd /c` from either shell.
    assert_eq!(ps_moved("cmd /c move /Y a.docx b.docx"), ["a.docx"]);
    assert_eq!(
        changed(r"cmd /c ren 'Drafts\a.docx' b.docx"),
        ["Drafts/a.docx", "Drafts/b.docx"]
    );
    assert_eq!(ps_deleted("cmd /c del /Q a.docx"), ["a.docx"]);
    // An unclear source moves nothing the reading can name; a move is no delete.
    assert_eq!(ps_moved("Move-Item $src b.docx"), none());
    assert_eq!(ps_deleted("Move-Item a.docx b.docx"), none());
}

#[test]
fn powershell_remove_item_and_del_delete_their_paths() {
    assert_eq!(
        ps_deleted(r"Remove-Item -Path 'C:\Matters\Acme\Old brief.docx' -Force"),
        ["C:/Matters/Acme/Old brief.docx"]
    );
    assert_eq!(
        ps_deleted(r#"Remove-Item -LiteralPath "Drafts\Memo v2.docx""#),
        ["Drafts/Memo v2.docx"]
    );
    assert_eq!(ps_deleted("del notes.txt"), ["notes.txt"]);
    assert_eq!(ps_deleted("rm -Recurse -Force Scans"), ["Scans"]);
    assert_eq!(
        ps_deleted(r"remove-item.exe -Filter *.tmp -Path C:\Matters\Acme"),
        ["C:/Matters/Acme"]
    );
    assert_eq!(ps_deleted("Get-Item notes.txt"), none());
}

/// The public readings take a shell call's input, as the after-turn detector and the guard do.
#[test]
fn the_input_readings_split_sources_from_destinations() {
    let input = r#"{"command":"mv \"Sharma v State 2024/Big.mp4\" Archive/","cwd":"Matters"}"#;
    let run = |add: fn(&mut BTreeSet<String>, &str, Dialect, Option<&str>)| {
        let mut out = BTreeSet::new();
        add(&mut out, input, Dialect::Posix, Some(""));
        out.into_iter().collect::<Vec<_>>()
    };
    assert_eq!(
        run(add_moved_input),
        ["Matters/Sharma v State 2024/Big.mp4"]
    );
    assert_eq!(
        run(add_changed_input),
        [
            "Matters/Archive/Big.mp4",
            "Matters/Sharma v State 2024/Big.mp4"
        ]
    );
    assert_eq!(run(add_deleted_input), none());
    assert_eq!(run(add_input), none());
}

#[test]
fn the_reader_gives_the_paths_index_by_index() {
    let posix = |program: &str, args: &[&str]| taken(program, args, Dialect::Posix);
    assert_eq!(posix("rm", &["-rf", "a", "b"]).deleted, [1, 2]);
    assert_eq!(posix("RM.EXE", &["a"]).deleted, [0]);
    assert_eq!(posix(r"C:\Windows\del", &["a"]).deleted, [0]);
    assert_eq!(
        taken(
            "Remove-Item",
            &["-Include", "*.tmp", "x"],
            Dialect::PowerShell
        )
        .deleted,
        [2]
    );
    assert_eq!(posix("cat", &["a"]), Taken::default());
    assert_eq!(
        posix("mv", &["a", "b", "d/"]),
        Taken {
            sources: vec![0, 1],
            to: Some(To::Folder(2)),
            ..Taken::default()
        }
    );
    assert_eq!(
        posix("git", &["mv", "-f", "a", "b"]),
        Taken {
            sources: vec![2],
            to: Some(To::Path(3)),
            ..Taken::default()
        }
    );
}
