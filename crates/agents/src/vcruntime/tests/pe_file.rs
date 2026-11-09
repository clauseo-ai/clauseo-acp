//! A minimal PE file for the tests, and the PE reader's own tests.

use super::{RG_IMPORTS, RG_LINKER};
use crate::vcruntime::pe::{self, MACHINE_ARM64};

/// A minimal PE32+ file: `machine`, `linker`, and import tables naming `imports` and
/// `delay_imports`. Enough for `pe::parse` and for Windows' calls to find no signature and no
/// version resource.
pub(super) fn pe_file(
    machine: u16,
    linker: (u8, u8),
    imports: &[&str],
    delay_imports: &[&str],
) -> Vec<u8> {
    const SECTION_RVA: u32 = 0x1000;
    const SECTION_RAW: usize = 0x200;
    let mut section = Vec::new();
    let import_size = (imports.len() + 1) * 20;
    let delay_size = if delay_imports.is_empty() {
        0
    } else {
        (delay_imports.len() + 1) * 32
    };
    let mut names_at = import_size + delay_size;
    let mut name_rvas = Vec::new();
    let mut names = Vec::new();
    for name in imports.iter().chain(delay_imports) {
        name_rvas.push(SECTION_RVA + u32::try_from(names_at).unwrap());
        names.extend_from_slice(name.as_bytes());
        names.push(0);
        names_at += name.len() + 1;
    }
    for rva in &name_rvas[..imports.len()] {
        let mut desc = [0_u8; 20];
        desc[12..16].copy_from_slice(&rva.to_le_bytes());
        section.extend_from_slice(&desc);
    }
    section.extend_from_slice(&[0; 20]);
    if !delay_imports.is_empty() {
        for rva in &name_rvas[imports.len()..] {
            let mut desc = [0_u8; 32];
            desc[0..4].copy_from_slice(&1_u32.to_le_bytes());
            desc[4..8].copy_from_slice(&rva.to_le_bytes());
            section.extend_from_slice(&desc);
        }
        section.extend_from_slice(&[0; 32]);
    }
    section.extend_from_slice(&names);
    while section.len() % 0x200 != 0 {
        section.push(0);
    }

    let mut file = vec![0_u8; SECTION_RAW];
    file[0..2].copy_from_slice(b"MZ");
    file[0x3c..0x40].copy_from_slice(&0x40_u32.to_le_bytes());
    file[0x40..0x44].copy_from_slice(b"PE\0\0");
    let coff = 0x44;
    file[coff..coff + 2].copy_from_slice(&machine.to_le_bytes());
    file[coff + 2..coff + 4].copy_from_slice(&1_u16.to_le_bytes());
    file[coff + 16..coff + 18].copy_from_slice(&240_u16.to_le_bytes());
    file[coff + 18..coff + 20].copy_from_slice(&0x22_u16.to_le_bytes());
    let opt = coff + 20;
    file[opt..opt + 2].copy_from_slice(&0x20b_u16.to_le_bytes());
    file[opt + 2] = linker.0;
    file[opt + 3] = linker.1;
    file[opt + 24..opt + 32].copy_from_slice(&0x1_4000_0000_u64.to_le_bytes());
    file[opt + 108..opt + 112].copy_from_slice(&16_u32.to_le_bytes());
    let dirs = opt + 112;
    file[dirs + 8..dirs + 12].copy_from_slice(&SECTION_RVA.to_le_bytes());
    file[dirs + 12..dirs + 16].copy_from_slice(&u32::try_from(import_size).unwrap().to_le_bytes());
    if delay_size > 0 {
        let delay_rva = SECTION_RVA + u32::try_from(import_size).unwrap();
        file[dirs + 104..dirs + 108].copy_from_slice(&delay_rva.to_le_bytes());
        file[dirs + 108..dirs + 112]
            .copy_from_slice(&u32::try_from(delay_size).unwrap().to_le_bytes());
    }
    let sect = opt + 240;
    file[sect..sect + 6].copy_from_slice(b".idata");
    let len = u32::try_from(section.len()).unwrap();
    file[sect + 8..sect + 12].copy_from_slice(&len.to_le_bytes());
    file[sect + 12..sect + 16].copy_from_slice(&SECTION_RVA.to_le_bytes());
    file[sect + 16..sect + 20].copy_from_slice(&len.to_le_bytes());
    file[sect + 20..sect + 24].copy_from_slice(&u32::try_from(SECTION_RAW).unwrap().to_le_bytes());
    file.extend_from_slice(&section);
    file
}

#[test]
fn pe_reader_reads_imports_and_refuses_bad_files() {
    let bytes = pe_file(MACHINE_ARM64, RG_LINKER, &RG_IMPORTS, &["delayed.dll"]);
    let info = pe::parse(&bytes).unwrap();
    assert_eq!(info.machine, MACHINE_ARM64);
    assert_eq!(info.linker, RG_LINKER);
    assert_eq!(info.imports, RG_IMPORTS);
    assert_eq!(info.delay_imports, ["delayed.dll"]);
    // Every truncation before the last name's end is an error, never a panic.
    let last = bytes.iter().rposition(|&byte| byte != 0).unwrap();
    for len in 0..=last {
        let cut = &bytes[..len];
        if let Ok(info) = pe::parse(cut) {
            panic!("a file cut at {len} read as {info:?}");
        }
    }
    assert!(pe::parse(b"MZ").is_err());
    let mut bad = bytes.clone();
    bad[0x3c..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(pe::parse(&bad).is_err());
    // An import table with no terminator, whose names run off the section, is an error.
    let mut endless = bytes;
    let end = endless.len();
    endless[0x200..end].fill(0xFF);
    assert!(pe::parse(&endless).is_err());
}

/// Where `pe_file` puts things: the COFF header's section count, the PE32+ image base, and the
/// section's bytes (its first descriptor).
const SECTIONS_AT: usize = 0x44 + 2;
const IMAGE_BASE_AT: usize = 0x58 + 24;
const SECTION_AT: usize = 0x200;

#[test]
fn a_non_ascii_import_name_is_refused() {
    let mut bytes = pe_file(MACHINE_ARM64, RG_LINKER, &["VCRUNTIME140.dll"], &[]);
    let at = bytes
        .windows(16)
        .position(|window| window == b"VCRUNTIME140.dll")
        .unwrap();
    bytes[at] = 0xC3;
    assert_eq!(pe::parse(&bytes), Err(pe::NotPe("non-ASCII name")));
}

#[test]
fn more_than_96_sections_is_refused() {
    let mut bytes = pe_file(MACHINE_ARM64, RG_LINKER, &["VCRUNTIME140.dll"], &[]);
    bytes[SECTIONS_AT..SECTIONS_AT + 2].copy_from_slice(&97_u16.to_le_bytes());
    assert_eq!(pe::parse(&bytes), Err(pe::NotPe("too many sections")));
    bytes[SECTIONS_AT..SECTIONS_AT + 2].copy_from_slice(&u16::MAX.to_le_bytes());
    assert_eq!(pe::parse(&bytes), Err(pe::NotPe("too many sections")));
}

/// Visual C++ 6's delay-import descriptors (`Attributes` bit 0 clear) hold the name as a VA:
/// the image base is subtracted; a VA below the image base is an error, never a wrap.
#[test]
fn the_old_delay_import_form_reads_its_name_as_a_va() {
    let base: u32 = 0x0001_0000;
    let mut bytes = pe_file(MACHINE_ARM64, RG_LINKER, &[], &["old.dll"]);
    // No imports: the import table is its terminator alone (20 bytes), then the delay table.
    let desc = SECTION_AT + 20;
    let rva = u32::from_le_bytes(bytes[desc + 4..desc + 8].try_into().unwrap());
    bytes[desc..desc + 4].copy_from_slice(&0_u32.to_le_bytes());
    let mut below = bytes.clone();
    // Below the image base (0x1_4000_0000 here): refused.
    assert_eq!(
        pe::parse(&below),
        Err(pe::NotPe("import name outside the file"))
    );
    bytes[IMAGE_BASE_AT..IMAGE_BASE_AT + 8].copy_from_slice(&u64::from(base).to_le_bytes());
    bytes[desc + 4..desc + 8].copy_from_slice(&(rva + base).to_le_bytes());
    assert_eq!(pe::parse(&bytes).unwrap().delay_imports, ["old.dll"]);
    // The same VA with the old image base set back: below it, refused.
    below[desc + 4..desc + 8].copy_from_slice(&(rva + base).to_le_bytes());
    assert!(pe::parse(&below).is_err());
}
