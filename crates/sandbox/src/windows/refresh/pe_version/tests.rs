//! The `FILEVERSION` read from a PE image's bytes: a minimal image built here, and every way a
//! truncated or foreign image fails to `None`, never a panic.

use super::super::test_signer::image;
use super::*;

#[test]
fn the_file_version_is_read_from_the_version_resource() {
    let exe = image([1, 4, 2, 7], b"tail");
    assert_eq!(file_version(&exe), Some([1, 4, 2, 7]));
    assert_eq!(dotted([1, 4, 2, 7]), "1.4.2.7");
    assert_eq!(
        file_version(&image([65535, 0, 0, 1], b"")),
        Some([65535, 0, 0, 1])
    );
}

#[test]
fn a_foreign_or_broken_image_has_no_version() {
    let exe = image([1, 4, 0, 0], b"");
    assert_eq!(file_version(b""), None);
    assert_eq!(file_version(b"MZ the old setup exe"), None);
    assert_eq!(file_version(b"\x7fELF and so on"), None);
    // Every truncation of a good image: None, never a panic.
    for len in 0..exe.len() {
        assert_eq!(file_version(&exe[..len]), None, "{len}");
    }
    // A wrong signature, key or magic.
    for (at, byte) in [
        (0x200 + 0x58 + 40, 0u8),
        (0x200 + 0x58 + 6, b'X'),
        (0x58, 0x0C),
    ] {
        let mut broken = exe.clone();
        broken[at] = byte;
        assert_eq!(file_version(&broken), None, "{at:#x}");
    }
    // A resource tree whose subtable offset points past the image.
    let mut broken = exe;
    broken[0x200 + 0x14..0x200 + 0x18].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
    assert_eq!(file_version(&broken), None);
}

#[test]
fn version_not_older_compares_as_numbers() {
    for (recorded, candidate) in [
        ("1.4.0", "1.4.0.0"),
        ("1.4.0.0", "1.4.0"),
        ("v1.4.0-beta.2", "1.4.0.0"),
        ("1.4.0", "1.4.1.0"),
        ("1.4.0", "1.10.0.0"),
        ("1.9.9.9", "2.0.0.0"),
        ("0.1.0", "0.1.0.1"),
    ] {
        assert!(
            version_not_older(recorded, candidate),
            "{recorded} -> {candidate}"
        );
    }
    for (recorded, candidate) in [
        ("1.4.0", "1.3.9.0"),
        ("1.10.0", "1.9.0.0"),
        ("2.0.0.0", "1.99.99.99"),
        ("1.4.0.1", "1.4.0.0"),
        // Unreadable on either side: refused.
        ("", "1.4.0.0"),
        ("1.4.0", ""),
        ("1.4.0", "1.4.0.0.0"),
        ("one", "1.4.0.0"),
        ("1..4", "1.4.0.0"),
        ("1.4.0", "1.4.x.0"),
    ] {
        assert!(
            !version_not_older(recorded, candidate),
            "{recorded} -> {candidate}"
        );
    }
}
