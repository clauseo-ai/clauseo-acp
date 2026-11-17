//! Windows paths through the delete, move and write readings: UNC paths (`\\server\share\…`)
//! and their `\\?\UNC\` and `\\?\C:\` forms, and drive-relative paths (`C:a.docx`,
//! `C:..\b.docx`), joined to the folder the command runs in when it is on that drive. A
//! drive-relative path run anywhere else is no path here: the delete and move readings say so.

use super::super::{Dialect, Kind};
use super::deletes::reading;

const UNC: &str = "//server/share/Matters/a.docx";

/// What `script`, PowerShell run in `cwd`, gives through `kind`: its paths, and whether a delete
/// or a move in it names a target that is no path here.
fn ps(script: &str, kind: Kind, cwd: &str) -> (Vec<String>, bool) {
    let (paths, _, unclear) = reading(script, Dialect::PowerShell, kind, Some(cwd));
    (paths, unclear)
}

fn paths(list: &[&str]) -> (Vec<String>, bool) {
    (list.iter().map(|path| (*path).to_owned()).collect(), false)
}

fn unclear() -> (Vec<String>, bool) {
    (Vec::new(), true)
}

#[test]
fn a_unc_target_is_its_whole_path() {
    let unc = paths(&[UNC]);
    let delete = r"Remove-Item \\server\share\Matters\a.docx";
    assert_eq!(ps(delete, Kind::Deletes, ""), unc);
    assert_eq!(ps(delete, Kind::Deletes, r"C:\Work"), unc);
    let mv = r"Move-Item \\server\share\Matters\a.docx \\server\share\Matters\Archive\";
    assert_eq!(ps(mv, Kind::MovedFrom, ""), unc);
    assert_eq!(
        ps(mv, Kind::Changes, "").0,
        ["//server/share/Matters/Archive/a.docx", UNC]
    );
    let copy = r"cp Lease.docx \\server\share\Matters\a.docx";
    assert_eq!(ps(copy, Kind::Writes, ""), unc);
    let (posix, _, _) = reading(
        "cp Lease.docx //server/share/Matters/a.docx",
        Dialect::Posix,
        Kind::Writes,
        Some(""),
    );
    assert_eq!(posix, [UNC]);
    // Run in a folder on a share.
    assert_eq!(
        ps(
            "Remove-Item a.docx",
            Kind::Deletes,
            r"\\server\share\Matters"
        ),
        unc
    );
    assert_eq!(
        ps(
            r"Remove-Item ..\b.docx",
            Kind::Deletes,
            r"\\server\share\Matters"
        ),
        paths(&["//server/share/b.docx"])
    );
}

#[test]
fn a_win32_prefix_is_read_before_unc_or_a_drive() {
    let unc = paths(&[UNC]);
    // Quoted: a bare `?` is a wildcard.
    assert_eq!(
        ps(
            r"Remove-Item -LiteralPath '\\?\UNC\server\share\Matters\a.docx'",
            Kind::Deletes,
            ""
        ),
        unc
    );
    assert_eq!(
        ps(
            r"Remove-Item \\.\unc\server\share\Matters\a.docx",
            Kind::Deletes,
            ""
        ),
        unc
    );
    assert_eq!(
        ps(
            r"Move-Item -LiteralPath '\\?\UNC\server\share\Matters\a.docx' C:\Archive\",
            Kind::Changes,
            ""
        )
        .0,
        [UNC, "C:/Archive/a.docx"]
    );
    assert_eq!(
        ps(
            r"Remove-Item -LiteralPath '\\?\C:\Matters\a.docx'",
            Kind::Deletes,
            ""
        ),
        paths(&["C:/Matters/a.docx"])
    );
    assert_eq!(
        ps(
            r"cp Lease.docx '\\?\UNC\server\share\Matters\a.docx'",
            Kind::Writes,
            ""
        ),
        unc
    );
    assert_eq!(
        ps(
            r"Remove-Item \\?\UNC\server\share\a.docx",
            Kind::Deletes,
            ""
        ),
        unclear()
    );
    // A device or volume path is no path here.
    assert_eq!(
        ps(r"Remove-Item \\.\PhysicalDrive0", Kind::Deletes, ""),
        unclear()
    );
    assert_eq!(
        ps(r"Remove-Item '\\?\Volume{1b3}\a.docx'", Kind::Deletes, ""),
        unclear()
    );
    assert_eq!(ps(r"echo x > \\.\nul", Kind::Writes, ""), paths(&[]));
}

#[test]
fn a_drive_relative_target_joins_the_folder_on_its_drive() {
    let deal = r"C:\Matters\Deal";
    assert_eq!(
        ps("Remove-Item C:a.docx", Kind::Deletes, deal),
        paths(&["C:/Matters/Deal/a.docx"])
    );
    assert_eq!(
        ps(r"Remove-Item c:..\b.docx", Kind::Deletes, deal),
        paths(&["C:/Matters/b.docx"])
    );
    assert_eq!(
        ps("Remove-Item C:a.docx", Kind::Deletes, "c:/Matters/Deal"),
        paths(&["c:/Matters/Deal/a.docx"])
    );
    assert_eq!(
        ps(
            r"Set-Location C:\Matters; Remove-Item C:a.docx",
            Kind::Deletes,
            r"D:\Work"
        ),
        paths(&["C:/Matters/a.docx"])
    );
    assert_eq!(
        ps(r"Move-Item C:a.docx C:Archive\", Kind::Changes, deal).0,
        ["C:/Matters/Deal/Archive/a.docx", "C:/Matters/Deal/a.docx"]
    );
    assert_eq!(
        ps("cp Lease.docx C:b.docx", Kind::Writes, deal),
        paths(&["C:/Matters/Deal/b.docx"])
    );
    // A rename's new name stands beside its drive-relative source.
    assert_eq!(
        ps("Rename-Item C:a.docx b.docx", Kind::Changes, deal).0,
        ["C:/Matters/Deal/a.docx", "C:/Matters/Deal/b.docx"]
    );
}

#[test]
fn a_drive_relative_target_off_its_drive_is_no_path_here() {
    // The folder the command runs in is on another drive, relative, or unknown.
    for cwd in [r"D:\Work", "", "Deal"] {
        assert_eq!(
            ps("Remove-Item C:a.docx", Kind::Deletes, cwd),
            unclear(),
            "{cwd}"
        );
        assert_eq!(
            ps(r"Remove-Item C:..\b.docx", Kind::Deletes, cwd),
            unclear()
        );
        assert_eq!(
            ps("Move-Item C:a.docx Archive", Kind::MovedFrom, cwd),
            unclear()
        );
        assert_eq!(ps("cp Lease.docx C:b.docx", Kind::Writes, cwd), paths(&[]));
    }
    let (none, _, flagged) = reading("rm C:a.docx", Dialect::Posix, Kind::Deletes, None);
    assert!(none.is_empty() && flagged);
}
