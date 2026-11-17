//! What a delete or a move takes (`shell_writes::deletes`), through each reading: the paths a
//! delete names ([`add_deleted_input`]), a move's sources alone ([`add_moved_input`]), and both
//! sides with every write ([`add_changed_input`]); [`add_input`] keeps neither. POSIX and
//! PowerShell, quoted paths, spaces, apostrophes and Windows paths; an option is no path, nor is
//! the value of one that takes a pattern or a setting.

use std::collections::BTreeSet;

use super::super::deletes::{Act, Arg, Taken, To, taken};
use super::super::{
    Dialect, Kind, Out, add_changed_input, add_deleted_input, add_input, add_moved_input,
    add_script_as,
};

/// What `script`, run in `cwd`, gives through the reading `kind`: its paths, those matched only
/// whole, and whether a delete or a move in it names a target that is no path here.
pub(super) fn reading(
    script: &str,
    dialect: Dialect,
    kind: Kind,
    cwd: Option<&str>,
) -> (Vec<String>, Vec<String>, bool) {
    let (mut paths, mut exact) = (BTreeSet::new(), BTreeSet::new());
    let mut out = Out::new(&mut paths, Some(&mut exact));
    add_script_as(&mut out, cwd, script, dialect, kind);
    let unclear = out.unclear;
    (
        paths.into_iter().collect(),
        exact.into_iter().collect(),
        unclear,
    )
}

fn read(script: &str, dialect: Dialect, kind: Kind) -> Vec<String> {
    reading(script, dialect, kind, Some("")).0
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
    // `b.docx` may be the new name or a folder `a.docx` goes in.
    assert_eq!(
        changed("mv a.docx b.docx"),
        ["a.docx", "b.docx", "b.docx/a.docx"]
    );
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
        [
            "New name.docx",
            "New name.docx/Old name.docx",
            "Old name.docx"
        ]
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
            "Archive/O'Brien Lease.docx/O'Brien Lease.docx",
            "Sharma v State 2024/O'Brien Lease.docx"
        ]
    );
    assert_eq!(
        ps_moved("Move-Item -Path a.docx -Destination b.docx"),
        ["a.docx"]
    );
    assert_eq!(
        ps_changed("Move-Item -Path a.docx -Destination b.docx"),
        ["a.docx", "b.docx", "b.docx/a.docx"]
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
    let list = |set: BTreeSet<String>| set.into_iter().collect::<Vec<_>>();
    let mut moved = BTreeSet::new();
    assert!(!add_moved_input(
        &mut moved,
        input,
        Dialect::Posix,
        Some("")
    ));
    assert_eq!(list(moved), ["Matters/Sharma v State 2024/Big.mp4"]);
    let (mut changed, mut exact) = (BTreeSet::new(), BTreeSet::new());
    add_changed_input(&mut changed, &mut exact, input, Dialect::Posix, Some(""));
    assert_eq!(
        list(changed),
        [
            "Matters/Archive/Big.mp4",
            "Matters/Sharma v State 2024/Big.mp4"
        ]
    );
    assert_eq!(list(exact), none());
    let mut deleted = BTreeSet::new();
    assert!(!add_deleted_input(
        &mut deleted,
        input,
        Dialect::Posix,
        Some("")
    ));
    assert_eq!(list(deleted), none());
    let mut written = BTreeSet::new();
    add_input(&mut written, input, Dialect::Posix, Some(""));
    assert_eq!(list(written), none());
    // A delete whose target is a glob says so.
    let glob = r#"{"command":"Remove-Item *.docx"}"#;
    let mut deleted = BTreeSet::new();
    assert!(add_deleted_input(
        &mut deleted,
        glob,
        Dialect::PowerShell,
        Some("")
    ));
    assert_eq!(list(deleted), none());
}

#[test]
fn the_reader_gives_the_paths_place_by_place() {
    let posix = |program: &str, args: &[&str]| taken(program, args, Dialect::Posix);
    let whole = |places: &[usize]| places.iter().map(|at| Arg::whole(*at)).collect::<Vec<_>>();
    assert_eq!(posix("rm", &["-rf", "a", "b"]).deleted, whole(&[1, 2]));
    assert_eq!(posix("RM.EXE", &["a"]).deleted, whole(&[0]));
    assert_eq!(posix(r"C:\Windows\del", &["a"]).deleted, whole(&[0]));
    assert_eq!(
        taken(
            "Remove-Item",
            &["-Include", "*.tmp", "x"],
            Dialect::PowerShell
        )
        .deleted,
        whole(&[2])
    );
    assert_eq!(
        taken("Remove-Item", &["-Path:a.docx"], Dialect::PowerShell).deleted,
        [Arg { at: 0, from: 6 }]
    );
    assert_eq!(posix("cat", &["a"]), Taken::default());
    assert_eq!(
        posix("mv", &["a", "b", "d/"]),
        Taken {
            act: Some(Act::Move),
            sources: whole(&[0, 1]),
            to: Some(To::Folder(Arg::whole(2))),
            ..Taken::default()
        }
    );
    assert_eq!(
        posix("git", &["mv", "-f", "a", "b"]),
        Taken {
            act: Some(Act::Move),
            sources: whole(&[2]),
            to: Some(To::Path(Arg::whole(3))),
            ..Taken::default()
        }
    );
    assert_eq!(
        posix("mv", &["-tArchive", "a"]).to,
        Some(To::Folder(Arg { at: 0, from: 2 }))
    );
    assert_eq!(
        posix("mv", &["--target-directory=Archive", "a"]).to,
        Some(To::Folder(Arg { at: 0, from: 19 }))
    );
    // A parameter with no value after it; a cmdlet that names no path.
    assert!(taken("Remove-Item", &["-Path"], Dialect::PowerShell).unclear);
    assert!(posix("mv", &["a", "-t"]).unclear);
    assert!(taken("Remove-Item", &["-Recurse"], Dialect::PowerShell).input);
    assert!(!taken("Remove-Item", &["a"], Dialect::PowerShell).input);
}
