//! The delete and move forms through each reading, with the place a command runs in: PowerShell
//! arrays, `-Name:value`, the common parameters, `mv`'s target folder in each spelling, every
//! program name, `Set-Location`, `.` and `..`, and pipelines.

use super::super::{Dialect, Kind};
use super::deletes::reading;

fn none() -> Vec<String> {
    Vec::new()
}

/// The paths a delete names in PowerShell, and whether one is no path here.
fn ps_deleted(script: &str) -> (Vec<String>, bool) {
    let (paths, _, unclear) = reading(script, Dialect::PowerShell, Kind::Deletes, Some(""));
    (paths, unclear)
}

fn ps_moved(script: &str) -> (Vec<String>, bool) {
    let (paths, _, unclear) = reading(script, Dialect::PowerShell, Kind::MovedFrom, Some(""));
    (paths, unclear)
}

fn ps_changed(script: &str) -> Vec<String> {
    reading(script, Dialect::PowerShell, Kind::Changes, Some("")).0
}

fn deleted_in(script: &str, cwd: Option<&str>) -> (Vec<String>, bool) {
    let (paths, _, unclear) = reading(script, Dialect::Posix, Kind::Deletes, cwd);
    (paths, unclear)
}

fn changed(script: &str) -> Vec<String> {
    reading(script, Dialect::Posix, Kind::Changes, Some("")).0
}

fn read(paths: &[&str]) -> (Vec<String>, bool) {
    (paths.iter().map(|path| (*path).to_owned()).collect(), false)
}

#[test]
fn powershell_arrays_name_each_element() {
    let both = read(&["a.docx", "b.docx"]);
    assert_eq!(ps_deleted("Remove-Item a.docx, b.docx"), both);
    assert_eq!(ps_deleted("Remove-Item a.docx,b.docx"), both);
    assert_eq!(ps_deleted("Remove-Item -Path a.docx,  b.docx -Force"), both);
    assert_eq!(
        ps_deleted(r"Remove-Item 'Old brief.docx','Drafts\Memo v2.docx'"),
        read(&["Drafts/Memo v2.docx", "Old brief.docx"])
    );
    assert_eq!(ps_deleted("cmd /c del a.docx,b.docx"), both);
    assert_eq!(ps_moved("Move-Item a.docx,b.docx Archive"), both);
    // Several sources go in the destination folder.
    assert_eq!(
        ps_changed("Move-Item a.docx,b.docx Archive"),
        ["Archive/a.docx", "Archive/b.docx", "a.docx", "b.docx"]
    );
    // An element that is no path here.
    assert_eq!(ps_deleted("Remove-Item a.docx, $other"), (none(), true));
    assert_eq!(ps_deleted("Remove-Item a.docx,*.tmp"), (none(), true));
    assert_eq!(ps_deleted(r"Remove-Item a.docx,~\b.docx"), (none(), true));
}

#[test]
fn cmdlet_parameters_take_their_value_after_a_colon() {
    let a = read(&["a.docx"]);
    assert_eq!(ps_deleted("Remove-Item -Path:a.docx"), a);
    assert_eq!(ps_deleted("Remove-Item -Path: a.docx"), a);
    assert_eq!(ps_deleted("Remove-Item -path:a.docx -Force:$true"), a);
    assert_eq!(
        ps_deleted(r"Remove-Item -LiteralPath:'Drafts\Memo v2.docx'"),
        read(&["Drafts/Memo v2.docx"])
    );
    assert_eq!(
        ps_deleted("Remove-Item -Path:a.docx,b.docx"),
        read(&["a.docx", "b.docx"])
    );
    assert_eq!(ps_moved(r"Move-Item -Path:a.docx -Destination:Archive\"), a);
    assert_eq!(
        ps_changed(r"Move-Item -Path:a.docx -Destination:Archive\"),
        ["Archive/a.docx", "a.docx"]
    );
    assert_eq!(
        ps_changed(r"Rename-Item -Path:'Drafts\a.docx' -NewName:b.docx"),
        ["Drafts/a.docx", "Drafts/b.docx"]
    );
    // A parameter with no value, or one that is no path here.
    assert_eq!(ps_deleted("Remove-Item -Path"), (none(), true));
    assert_eq!(ps_deleted("Remove-Item -Path:$f"), (none(), true));
    assert_eq!(ps_moved("Move-Item -LiteralPath"), (none(), true));
}

#[test]
fn common_parameters_take_values_that_are_no_paths() {
    let a = read(&["a.docx"]);
    assert_eq!(
        ps_deleted("Remove-Item a.docx -ErrorAction SilentlyContinue"),
        a
    );
    assert_eq!(
        ps_deleted("Remove-Item -ea 0 a.docx -WarningAction Ignore -ev err -ov out"),
        a
    );
    assert_eq!(
        ps_deleted(
            "del a.docx -InformationAction Continue -OutBuffer 5 -PipelineVariable p \
             -ProgressAction SilentlyContinue -ErrorVariable e -WarningVariable w \
             -InformationVariable i -OutVariable o -wa 0 -infa 0 -wv w -iv i -ob 1 -pv p \
             -proga 0 -Verbose -Debug -WhatIf -Confirm"
        ),
        a
    );
    // A unique prefix of a name, any case.
    assert_eq!(
        ps_deleted("Remove-Item -ErrorA Stop -literalp a.docx -VERB"),
        a
    );
    assert_eq!(
        ps_deleted("Remove-Item -Recurse:$true Scans"),
        read(&["Scans"])
    );
    assert_eq!(ps_moved("Move-Item -ErrorAction Stop a.docx b.docx"), a);
    assert_eq!(
        ps_changed("Rename-Item a.docx b.docx -ErrorAction Stop"),
        ["a.docx", "b.docx"]
    );
}

#[test]
fn cmdlet_parameters_bind_by_prefix_in_any_case() {
    let a = read(&["a.docx"]);
    assert_eq!(ps_deleted("Remove-Item -Pa a.docx"), a);
    assert_eq!(ps_deleted("remove-item -PATH a.docx"), a);
    assert_eq!(ps_deleted("REMOVE-ITEM -l a.docx -r -forc"), a);
    assert_eq!(ps_deleted("Remove-Item -LP a.docx -EA 0"), a);
    assert_eq!(ps_moved(r"Move-Item -Des Archive\ -Pat a.docx"), a);
    assert_eq!(
        ps_changed(r"move-item -DESTINATION Archive\ -literalpath a.docx"),
        ["Archive/a.docx", "a.docx"]
    );
    assert_eq!(
        ps_changed("Rename-Item -LITERALPATH a.docx -newn b.docx"),
        ["a.docx", "b.docx"]
    );
}

#[test]
fn posix_mv_reads_its_target_folder_in_every_spelling() {
    let into = ["Archive/a", "Archive/b", "a", "b"];
    assert_eq!(changed("mv -vt Archive a b"), into);
    assert_eq!(changed("mv -tArchive a b"), into);
    assert_eq!(changed("mv -vtArchive a b"), into);
    assert_eq!(changed("mv --target-directory Archive a b"), into);
    assert_eq!(changed("mv --target-directory=Archive a b"), into);
    assert_eq!(changed("mv a --target Archive b"), into);
    // `-T`: the destination is the source's new path.
    assert_eq!(changed("mv -T Drafts Old"), ["Drafts", "Old"]);
    // After `--` every word is a path.
    assert_eq!(changed("mv -- -t x"), ["-t", "x", "x/-t"]);
    assert_eq!(changed("mv -fS .bak a b"), ["a", "b", "b/a"]);
    let (_, _, unclear) = reading("mv a -t", Dialect::Posix, Kind::MovedFrom, Some(""));
    assert!(unclear);
}

#[test]
fn every_program_name_deletes_or_moves() {
    let a = read(&["a.docx"]);
    assert_eq!(deleted_in("rmdir Empty", Some("")), read(&["Empty"]));
    assert_eq!(ps_deleted("erase a.docx"), a);
    assert_eq!(ps_deleted("ri a.docx"), a);
    assert_eq!(ps_deleted("rd Scans -Recurse"), read(&["Scans"]));
    assert_eq!(ps_deleted("rmdir Scans"), read(&["Scans"]));
    assert_eq!(ps_deleted("cmd /c rd /s /q Scans"), read(&["Scans"]));
    assert_eq!(ps_changed("rni a.docx b.docx"), ["a.docx", "b.docx"]);
    assert_eq!(ps_moved(r"cmd /k move a.docx Archive\"), a);
    assert_eq!(
        ps_changed(r"cmd /k move a.docx Archive\"),
        ["Archive/a.docx", "a.docx"]
    );
    assert_eq!(
        ps_changed(r"cmd /c rename Drafts\a.docx b.docx"),
        ["Drafts/a.docx", "Drafts/b.docx"]
    );
}

#[test]
fn the_folder_a_command_runs_in_places_its_paths() {
    assert_eq!(
        ps_deleted("Set-Location Drafts; Remove-Item a.docx"),
        read(&["Drafts/a.docx"])
    );
    assert_eq!(
        ps_deleted(r"Set-Location Drafts; Remove-Item ..\a.docx"),
        read(&["a.docx"])
    );
    // `.` and `..` from an absolute start.
    let m = read(&["/m"]);
    assert_eq!(deleted_in("rm -rf .", Some("/m")), m);
    assert_eq!(deleted_in("rm -rf ./", Some("/m")), m);
    assert_eq!(deleted_in("cd Sub && rm -rf ..", Some("/m")), m);
    assert_eq!(deleted_in("rm -rf ..", Some("/m/Sub")), m);
    // The matter's folder itself, or above it, from the matter's folder; an unknown start.
    assert_eq!(deleted_in("rm -rf .", Some("")), (none(), true));
    assert_eq!(deleted_in("cd Sub && rm -rf ..", Some("")), (none(), true));
    assert_eq!(deleted_in("rm -rf ../x", Some("")), (none(), true));
    assert_eq!(deleted_in("rm a.txt", None), (none(), true));
    assert_eq!(deleted_in("rm /m/a.txt", None), read(&["/m/a.txt"]));
}

#[test]
fn a_delete_in_a_pipeline_names_its_paths() {
    assert_eq!(
        ps_deleted("Remove-Item a.docx | Out-Null"),
        read(&["a.docx"])
    );
    assert_eq!(
        deleted_in("rm a.txt | tee log.txt", Some("")),
        read(&["a.txt"])
    );
    // A cmdlet on a pipeline's input that names no path.
    assert_eq!(
        ps_deleted("Get-ChildItem *.tmp | Remove-Item"),
        (none(), true)
    );
    assert_eq!(
        ps_deleted("Get-ChildItem *.tmp | Remove-Item -Force -ErrorAction Stop"),
        (none(), true)
    );
    assert_eq!(
        ps_moved("Get-Item a.docx | Move-Item -Destination Archive"),
        (none(), true)
    );
    assert_eq!(ps_deleted("Remove-Item -Recurse"), (none(), false));
}

#[test]
fn a_move_destination_without_a_separator_is_matched_whole() {
    let changed_exact = |script: &str, dialect| {
        let (paths, exact, _) = reading(script, dialect, Kind::Changes, Some(""));
        (paths, exact)
    };
    let archive = (
        vec![
            "Archive".to_owned(),
            "Archive/a.docx".to_owned(),
            "a.docx".to_owned(),
        ],
        vec!["Archive".to_owned()],
    );
    assert_eq!(changed_exact("mv a.docx Archive", Dialect::Posix), archive);
    assert_eq!(
        changed_exact("Move-Item a.docx Archive", Dialect::PowerShell),
        archive
    );
    assert_eq!(
        changed_exact("cmd /c move a.docx Archive", Dialect::PowerShell),
        archive
    );
    assert_eq!(
        changed_exact("mv a.docx Archive/", Dialect::Posix),
        (
            vec!["Archive/a.docx".to_owned(), "a.docx".to_owned()],
            none()
        )
    );
    assert_eq!(
        changed_exact("rm -r Folder", Dialect::Posix),
        (vec!["Folder".to_owned()], none())
    );
    assert_eq!(
        changed_exact("echo x > notes.md", Dialect::Posix),
        (vec!["notes.md".to_owned()], vec!["notes.md".to_owned()])
    );
}
