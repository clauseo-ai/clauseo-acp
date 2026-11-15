//! The Windows handle calls: errors read with the right kind, and a move never replaces.

use std::io;

use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS};

use super::{io_error, move_new_into, open_folder};

/// The review's M2: both "exists" errors read as `AlreadyExists`, which the download mover
/// retries under the next name; another error keeps its own kind.
#[test]
fn an_existing_name_reads_as_already_exists() {
    for code in [ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS] {
        let error = windows::core::Error::from(code.to_hresult());
        assert_eq!(
            io_error(&error).kind(),
            io::ErrorKind::AlreadyExists,
            "{code:?}"
        );
    }
    let denied = windows::core::Error::from(ERROR_ACCESS_DENIED.to_hresult());
    assert_eq!(io_error(&denied).kind(), io::ErrorKind::PermissionDenied);
}

/// A real move onto a file that exists fails with `AlreadyExists` and changes neither file.
#[test]
fn a_move_onto_a_file_fails_with_already_exists() {
    let dir = std::env::temp_dir().join(format!("clauseo-mcp-move-{}", std::process::id()));
    if dir.exists() {
        crate::write::remove_dir_all(&dir).unwrap();
    }
    let into = dir.join("into");
    crate::write::create_dir_all(&into).unwrap();
    let from = dir.join("from.txt");
    crate::write::write(&from, "download").unwrap();
    crate::write::write(&into.join("to.txt"), "kept").unwrap();
    let folder = open_folder(&into).unwrap();
    let error = move_new_into(&from, &folder, "to.txt".as_ref()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists, "{error}");
    assert_eq!(
        std::fs::read_to_string(into.join("to.txt")).unwrap(),
        "kept"
    );
    assert_eq!(std::fs::read_to_string(&from).unwrap(), "download");
    move_new_into(&from, &folder, "new.txt".as_ref()).unwrap();
    assert_eq!(
        std::fs::read_to_string(into.join("new.txt")).unwrap(),
        "download"
    );
    drop(folder);
    crate::write::remove_dir_all(&dir).unwrap();
}
