//! The `FILEVERSION` read from a PE image's bytes: a minimal image built here, and every way a
//! truncated or foreign image fails to `None`, never a panic.

use super::super::test_signer::{image, image_for, image_with_load_config};
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
fn the_machine_is_read_from_the_coff_header() {
    assert_eq!(machine(&image([1, 4, 0, 0], b"")), Some(MACHINE_X64));
    let arm = image_for(MACHINE_ARM64, [1, 4, 0, 0], b"");
    assert_eq!(machine(&arm), Some(MACHINE_ARM64));
    assert_eq!(machine(b"MZ not a PE"), None);
}

#[test]
fn a_hybrid_image_is_told_apart_from_a_plain_one() {
    let version = [1, 4, 0, 0];
    // No load config, or one with no CHPE metadata: the header's machine.
    assert_eq!(plain_machine(&image(version, b"")), Some(MACHINE_X64));
    for machine in [MACHINE_X64, MACHINE_ARM64, MACHINE_X86] {
        let plain = image_with_load_config(machine, version, b"", 0);
        assert_eq!(plain_machine(&plain), Some(machine), "{machine:#06x}");
    }
    // CHPE metadata: ARM64EC when headed x64, ARM64X when headed ARM64. The header alone says
    // otherwise, which is why the move across CPU types reads plain_machine.
    let ec = image_with_load_config(MACHINE_X64, version, b"", 0x1_4000_2000);
    assert_eq!(plain_machine(&ec), Some(MACHINE_ARM64EC));
    assert_eq!(machine(&ec), Some(MACHINE_X64));
    let x = image_with_load_config(MACHINE_ARM64, version, b"", 0x2000);
    assert_eq!(plain_machine(&x), Some(MACHINE_ARM64X));
    assert_eq!(machine(&x), Some(MACHINE_ARM64));
    // The file version still reads with a load config beside it.
    assert_eq!(file_version(&x), Some(version));
    // Every truncation: no panic, and none cut before the CHPE field's end passes for plain ARM64.
    let field_end = 0x200 + 184 + 208;
    for len in 0..x.len() {
        let read = plain_machine(&x[..len]);
        if len < field_end {
            assert_eq!(read, None, "{len}");
        }
    }
    // A load config RVA no section maps: unreadable, so None.
    let mut stray = x;
    stray[0x58 + 112 + 80..0x58 + 112 + 84].copy_from_slice(&0x9000u32.to_le_bytes());
    assert_eq!(plain_machine(&stray), None);
}
