//! The chain's top: the volume root and any folder between it and `%ProgramFiles%` are checked
//! too, the root to [`root_safe`]'s narrower rights.

use std::path::{Path, PathBuf};

use super::fake::{AUTHENTICATED_USERS, Fake, descriptor, locked_dacl, volume_root_dacl};
use super::*;

const LAWYER: &str = "S-1-5-21-1-2-3-1001";

fn root() -> PathBuf {
    PathBuf::from(r"C:\")
}

/// A stock root with `extra` added.
fn root_with(extra: (u8, u8, u32, &str)) -> Vec<u8> {
    descriptor(
        TRUSTED_INSTALLER,
        Some(&[
            (0, 0x3, FULL_ACCESS, SYSTEM),
            (0, 0x3, FULL_ACCESS, ADMINISTRATORS),
            (0, 0x3, READ_EXECUTE, USERS),
            extra,
        ]),
        false,
    )
}

#[test]
fn the_folders_above_program_files_run_from_the_volume_root_down() {
    let above_of = |text: &str| above(Path::new(text));
    assert_eq!(above_of(r"C:\Program Files"), Some(vec![root()]));
    assert_eq!(above_of(r"\\?\C:\Program Files\"), Some(vec![root()]));
    assert_eq!(
        above_of(r"D:\Apps\Program Files"),
        Some(vec![PathBuf::from(r"D:\"), PathBuf::from(r"D:\Apps")])
    );
    for refused in [
        r"C:\",
        r"Program Files",
        r"\\server\share\Program Files",
        r"\\?\UNC\server\share\Program Files",
        r"C:\Apps\..\Program Files",
        r"C:\Apps\\Program Files",
        r"CC:\Program Files",
    ] {
        assert_eq!(above_of(refused), None, "{refused}");
    }
}

#[test]
fn the_stock_volume_root_passes_and_the_root_is_read() {
    assert!(root_safe(&volume_root_dacl()));
    // Authenticated Users' "add a folder" at the root would fail the rule the levels below
    // keep: the root has its own.
    assert!(!level_safe(&volume_root_dacl()));
    let fake = Fake::ready();
    assert!(check(&fake).is_ok());
    let asked = fake.asked.borrow();
    assert!(
        asked.iter().any(|path| same_path(path, &root())),
        "{asked:?}"
    );
}

#[test]
fn a_right_at_the_root_that_could_replace_program_files_is_unsafe() {
    for right in [
        DELETE_CHILD,
        WRITE_DAC,
        WRITE_OWNER,
        GENERIC_ALL,
        MAXIMUM_ALLOWED,
    ] {
        for sid in [USERS, AUTHENTICATED_USERS, LAWYER] {
            let mut fake = Fake::ready();
            fake.set(&root(), Found::Folder, root_with((0, 0, right, sid)));
            assert_eq!(
                check(&fake),
                Err(Blocked::AdminCopyUnsafe),
                "{right:#x} {sid}"
            );
        }
    }
    // Adding a file or folder, writing attributes, or deleting the root itself replace nothing.
    for right in [
        WRITE_DATA,
        APPEND_DATA,
        WRITE_ATTRIBUTES,
        DELETE,
        GENERIC_WRITE,
    ] {
        let mut fake = Fake::ready();
        fake.set(&root(), Found::Folder, root_with((0, 0, right, USERS)));
        assert!(check(&fake).is_ok(), "{right:#x}");
    }
}

#[test]
fn an_untrusted_root_owner_a_null_dacl_a_linked_or_unread_root_is_unsafe() {
    let unsafe_roots = [
        (Found::Folder, Some(descriptor(LAWYER, Some(&[]), false))),
        (Found::Folder, Some(descriptor(ADMINISTRATORS, None, false))),
        (Found::Link, Some(volume_root_dacl())),
        (Found::Folder, None),
    ];
    for (found, security) in unsafe_roots {
        let mut fake = Fake::ready();
        match security {
            Some(bytes) => fake.set(&root(), found, bytes),
            None => {
                fake.paths.remove(r"c:\");
            }
        }
        assert_eq!(check(&fake), Err(Blocked::AdminCopyUnsafe), "{found:?}");
    }
}

/// [`Fake::ready`] with Program Files at `program_files` and its volume root at `root`, both as
/// the fake keys them (lowercase, `\` throughout).
fn moved(program_files: &str, root: &str) -> Fake {
    let mut fake = Fake::ready();
    let paths = std::mem::take(&mut fake.paths);
    for (path, entry) in paths {
        let path = match path.strip_prefix(r"c:\program files") {
            Some(rest) => format!("{program_files}{rest}"),
            None => root.to_owned(),
        };
        fake.paths.insert(path, entry);
    }
    fake.program_files = PathBuf::from(program_files);
    fake
}

#[test]
fn a_folder_between_the_root_and_program_files_keeps_the_full_rule() {
    let mut fake = moved(r"d:\apps\program files", r"d:\");
    let apps = Path::new(r"D:\Apps");
    fake.set(apps, Found::Folder, locked_dacl(true));
    assert!(check(&fake).is_ok());
    // Users may add a file there: fine at a root, unsafe below it.
    fake.set(apps, Found::Folder, root_with((0, 0, APPEND_DATA, USERS)));
    assert_eq!(check(&fake), Err(Blocked::AdminCopyUnsafe));
    fake.paths.remove(r"d:\apps");
    assert_eq!(check(&fake), Err(Blocked::AdminCopyUnsafe), "a level gone");
}

#[test]
fn program_files_off_a_drive_letter_is_unsafe() {
    // Every level is there and locked, but nothing says what is above a share.
    let fake = moved(r"\\server\share\program files", r"\\server\share");
    assert_eq!(check(&fake), Err(Blocked::AdminCopyUnsafe));
}

/// The uninstaller's hook checks the copy's chain itself before it elevates the copy
/// (`crates/desktop/windows/hooks.nsh`): with these rights and these owners.
#[test]
fn the_uninstall_hook_counts_these_rights_and_trusts_these_owners() {
    let hook = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../desktop/windows/hooks.nsh"),
    )
    .unwrap();
    let value = |name: &str| -> String {
        let prefix = format!("!define {name} ");
        hook.lines()
            .find_map(|line| line.strip_prefix(&prefix))
            .unwrap_or_else(|| panic!("{name}"))
            .to_owned()
    };
    let rights =
        |name: &str| u32::from_str_radix(value(name).trim_start_matches("0x"), 16).unwrap();
    assert_eq!(rights("CLAUSEO_CHANGE_RIGHTS"), CHANGE_RIGHTS);
    assert_eq!(rights("CLAUSEO_ROOT_CHANGE_RIGHTS"), ROOT_CHANGE_RIGHTS);
    assert_eq!(
        value("CLAUSEO_TRUSTED_INSTALLER"),
        format!("\"{TRUSTED_INSTALLER}\"")
    );
    for sid in [SYSTEM, ADMINISTRATORS] {
        assert!(hook.contains(&format!("$R8 == \"{sid}\"")), "{sid}");
    }
}
