//! The real Recycle Bin: a file goes there through `IFileOperation`, from a thread COM has not
//! seen and from one already in the multithreaded apartment, and the test purges its own
//! entries again by name. A path past 260 UTF-16 units is left to the fallback.
#![expect(
    unsafe_code,
    reason = "CoInitializeEx for the thread already in the MTA"
)]

use std::io::Write as _;
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

use super::{OsTrash, Trash, TrashError};
use crate::test_dir::TestDir;
use crate::write;

/// A file named `<stem>-<pid>.txt` in `dir`, holding `bytes`; a name of this run's own, so the
/// Recycle Bin entries the purge finds are this test's.
fn unique_file(dir: &Path, stem: &str, bytes: &[u8]) -> (PathBuf, String) {
    let name = format!("{stem}-{}.txt", std::process::id());
    let file = dir.join(&name);
    write::create_new(&file).unwrap().write_all(bytes).unwrap();
    (file, name)
}

/// Removes every entry of the Recycle Bin on `file`'s drive whose original name holds `name`:
/// each `$I` record (it holds the original path in UTF-16) and its `$R` file. Returns how many.
fn purge(file: &Path, name: &str) -> usize {
    let text = file.to_string_lossy();
    let root = PathBuf::from(text.get(..3).unwrap());
    let wanted: Vec<u16> = std::ffi::OsStr::new(name).encode_wide().collect();
    let mut purged = 0;
    let Ok(bins) = std::fs::read_dir(root.join("$Recycle.Bin")) else {
        return 0;
    };
    for bin in bins.flatten() {
        // Other users' bins refuse the listing.
        let Ok(entries) = std::fs::read_dir(bin.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let record = entry.path();
            let entry_name = entry.file_name().to_string_lossy().into_owned();
            let Some(rest) = entry_name.strip_prefix("$I") else {
                continue;
            };
            let Ok(bytes) = std::fs::read(&record) else {
                continue;
            };
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect();
            if units.windows(wanted.len()).any(|window| window == wanted) {
                let recycled = record.with_file_name(format!("$R{rest}"));
                drop(write::remove_file(&recycled));
                write::remove_file(&record).unwrap();
                purged += 1;
            }
        }
    }
    purged
}

/// Recycles a fresh file and checks it is in the Recycle Bin with its bytes, then purges it.
fn recycles(stem: &str) {
    let dir = TestDir::new(stem);
    let (file, name) = unique_file(dir.path(), stem, b"to the Recycle Bin");

    let went = OsTrash.trash(&file);

    let bytes = went
        .as_ref()
        .ok()
        .and_then(Option::as_ref)
        .map(std::fs::read);
    let purged = purge(&file, &name);
    let went = went
        .unwrap()
        .expect("the shell says where the bin keeps the file");
    assert!(!file.exists(), "gone from the folder");
    assert_eq!(bytes.unwrap().unwrap(), b"to the Recycle Bin");
    let in_bin = went.to_string_lossy().to_lowercase();
    assert!(in_bin.contains("$recycle.bin"), "{in_bin}");
    assert_eq!(
        purged, 1,
        "the test's entry was in the Recycle Bin, and is gone"
    );
}

#[test]
fn a_file_goes_to_the_recycle_bin_and_the_test_purges_it() {
    recycles("clauseo-recycle-test");
}

/// A thread already in the multithreaded apartment (`CoInitializeEx` answers
/// `RPC_E_CHANGED_MODE` to the trash's single-threaded ask) still recycles.
#[test]
fn a_thread_already_in_the_mta_still_recycles() {
    std::thread::spawn(|| {
        // SAFETY: no reserved pointer; balanced below on the same thread.
        let mta = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        assert!(mta.is_ok(), "{mta:?}");
        recycles("clauseo-recycle-mta");
        // SAFETY: balances the CoInitializeEx above.
        unsafe { CoUninitialize() };
    })
    .join()
    .unwrap();
}

/// The shell takes no `\\?\` path: a file past 260 UTF-16 units is left for the fallback,
/// untouched.
#[test]
fn a_path_past_260_units_is_left_to_the_fallback() {
    let dir = TestDir::new("trash-win-long");
    let mut folder = dir.path().to_path_buf();
    let mut n = 0;
    while folder.as_os_str().encode_wide().count() <= 280 {
        n += 1;
        folder.push(format!("{n:02}-folder-of-a-deeply-nested-client"));
    }
    write::create_dir_all(&folder).unwrap();
    let (file, _) = unique_file(&folder, "clauseo-recycle-long", b"stays");

    let err = OsTrash.trash(&file).unwrap_err();

    assert!(matches!(err, TrashError::NoTrash), "{err:?}");
    assert_eq!(std::fs::read(&file).unwrap(), b"stays");
}
