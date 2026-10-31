use super::hidden_attributes;

/// `FILE_ATTRIBUTE_READONLY`, `HIDDEN`, `DIRECTORY` and `ARCHIVE`.
const READONLY: u32 = 0x1;
const HIDDEN: u32 = 0x2;
const DIRECTORY: u32 = 0x10;
const ARCHIVE: u32 = 0x20;

#[test]
fn hiding_keeps_every_other_attribute() {
    assert_eq!(hidden_attributes(DIRECTORY), Some(DIRECTORY | HIDDEN));
    assert_eq!(
        hidden_attributes(DIRECTORY | READONLY | ARCHIVE),
        Some(DIRECTORY | READONLY | ARCHIVE | HIDDEN)
    );
    assert_eq!(hidden_attributes(DIRECTORY | HIDDEN), None);
}

/// Windows: a plain folder gets hidden and keeps read-only; a missing one is logged, not a panic.
#[cfg(windows)]
#[test]
fn hide_dir_sets_hidden_and_keeps_readonly() {
    use std::os::windows::fs::MetadataExt as _;

    let dir = crate::test_dir::TestDir::new("write-hide");
    let target = dir.path().join(".clauseo");
    super::create_dir(&target).unwrap();
    let mut permissions = std::fs::metadata(&target).unwrap().permissions();
    permissions.set_readonly(true);
    super::set_permissions(&target, permissions).unwrap();

    super::hide_dir(&target);
    let attributes = std::fs::symlink_metadata(&target)
        .unwrap()
        .file_attributes();
    assert_eq!(
        attributes & (HIDDEN | READONLY),
        HIDDEN | READONLY,
        "{attributes:#x}"
    );

    super::hide_dir(&dir.path().join("missing"));
    // Writable again, so the test folder can be removed.
    super::windows::set_attributes(&target, attributes & !READONLY).unwrap();
}
