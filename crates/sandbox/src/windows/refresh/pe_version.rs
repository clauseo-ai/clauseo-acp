//! The file version a PE image's `VERSIONINFO` carries: the numeric `FILEVERSION` of its
//! `VS_FIXEDFILEINFO`, read from the bytes alone, with no Windows call (and so no new DLL in
//! the setup exe), and the image's `Machine` (its CPU type). The refresh reads both from the
//! staged copy once its signature has verified (`version.rs` compares the version).
//!
//! The walk: the DOS header's `e_lfanew`, the PE signature, the optional header (PE32 or PE32+),
//! data directory 2 (resources), the section that maps it, the resource tree's `RT_VERSION` (16)
//! entry down to its first name and language, that leaf's data, then `VS_VERSIONINFO`: its key
//! `VS_VERSION_INFO` and, at the next 4-byte boundary, `VS_FIXEDFILEINFO` (signature
//! `0xFEEF04BD`). Every read is bounds-checked; anything malformed is `None`.

/// `RT_VERSION`.
const RT_VERSION: u32 = 16;
/// `VS_FIXEDFILEINFO`'s signature.
const FIXED_SIGNATURE: u32 = 0xFEEF_04BD;
/// The deepest the resource tree is walked (type, name, language).
const LEVELS: usize = 3;

/// `IMAGE_FILE_MACHINE_AMD64`.
pub const MACHINE_X64: u16 = 0x8664;
/// `IMAGE_FILE_MACHINE_ARM64`.
pub const MACHINE_ARM64: u16 = 0xAA64;
/// `IMAGE_FILE_MACHINE_I386`.
pub const MACHINE_X86: u16 = 0x014C;

/// The image's COFF `Machine`, or `None` when it isn't a PE image.
pub fn machine(image: &[u8]) -> Option<u16> {
    Pe::parse(image).map(|pe| pe.machine)
}

/// The image's `FILEVERSION` as its four parts, or `None` when it has no readable one.
pub fn file_version(image: &[u8]) -> Option<[u16; 4]> {
    let pe = Pe::parse(image)?;
    let data = pe.version_resource()?;
    fixed_version(data)
}

/// `[1, 4, 0, 0]` as `1.4.0.0`.
pub fn dotted(version: [u16; 4]) -> String {
    let [a, b, c, d] = version;
    format!("{a}.{b}.{c}.{d}")
}

/// `N` little-endian bytes of `bytes` at `at`.
fn le<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
    bytes.get(at..at.checked_add(N)?)?.try_into().ok()
}

/// A `u16` at `at`.
fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    le::<2>(bytes, at).map(u16::from_le_bytes)
}

/// A `u32` at `at`.
fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    le::<4>(bytes, at).map(u32::from_le_bytes)
}

/// A `u32` at `at`, as a `usize`.
fn usize_at(bytes: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32_at(bytes, at)?).ok()
}

/// The parts of the image the walk needs.
struct Pe<'a> {
    bytes: &'a [u8],
    /// The COFF header's `Machine`.
    machine: u16,
    /// The resource directory's RVA (0 when absent).
    resources: usize,
    /// The section table's offset, and its number of sections.
    table: usize,
    sections: usize,
}

impl<'a> Pe<'a> {
    /// The headers of `bytes`, or `None` when it isn't a PE image.
    fn parse(bytes: &'a [u8]) -> Option<Pe<'a>> {
        if le::<2>(bytes, 0)? != *b"MZ" {
            return None;
        }
        let header = usize_at(bytes, 0x3C)?;
        if le::<4>(bytes, header)? != *b"PE\0\0" {
            return None;
        }
        let machine = u16_at(bytes, header.checked_add(4)?)?;
        let sections = usize::from(u16_at(bytes, header.checked_add(6)?)?);
        let optional = header.checked_add(24)?;
        let optional_size = usize::from(u16_at(bytes, header.checked_add(20)?)?);
        // Data directories start 96 bytes into a PE32 optional header, 112 into a PE32+ one;
        // the resource directory is the third, 8 bytes each.
        let directories = match u16_at(bytes, optional)? {
            0x10B => optional.checked_add(96)?,
            0x20B => optional.checked_add(112)?,
            _ => return None,
        };
        let count = usize_at(bytes, directories.checked_sub(4)?)?;
        let resources = if count > 2 {
            usize_at(bytes, directories.checked_add(16)?)?
        } else {
            0
        };
        Some(Pe {
            bytes,
            machine,
            resources,
            table: optional.checked_add(optional_size)?,
            sections,
        })
    }

    /// The file offset of `rva`, through the section that maps it.
    fn offset(&self, rva: usize) -> Option<usize> {
        (0..self.sections).find_map(|n| {
            let at = self.table.checked_add(n.checked_mul(40)?)?;
            let size = usize_at(self.bytes, at.checked_add(8)?)?;
            let start = usize_at(self.bytes, at.checked_add(12)?)?;
            let raw = usize_at(self.bytes, at.checked_add(20)?)?;
            let inside = rva >= start && rva - start < size;
            inside.then(|| raw.checked_add(rva - start)).flatten()
        })
    }

    /// The first `RT_VERSION` resource's data.
    fn version_resource(&self) -> Option<&'a [u8]> {
        if self.resources == 0 {
            return None;
        }
        let root = self.offset(self.resources)?;
        // The type level picks RT_VERSION; the name and language levels take their first entry.
        let mut table = root;
        let mut entry = None;
        for level in 0..LEVELS {
            let named = usize::from(u16_at(self.bytes, table.checked_add(12)?)?);
            let ids = usize::from(u16_at(self.bytes, table.checked_add(14)?)?);
            let first = table.checked_add(16)?;
            let pick = (0..named + ids).find_map(|n| {
                let at = first.checked_add(n.checked_mul(8)?)?;
                let id = u32_at(self.bytes, at)?;
                (level != 0 || id == RT_VERSION).then_some(at)
            })?;
            let below = u32_at(self.bytes, pick.checked_add(4)?)?;
            let below_at = root.checked_add(usize::try_from(below & 0x7FFF_FFFF).ok()?)?;
            if below & 0x8000_0000 == 0 {
                entry = Some(below_at);
                break;
            }
            table = below_at;
        }
        let leaf = entry?;
        let at = self.offset(usize_at(self.bytes, leaf)?)?;
        let size = usize_at(self.bytes, leaf.checked_add(4)?)?;
        self.bytes.get(at..at.checked_add(size)?)
    }
}

/// The `FILEVERSION` of the `VS_VERSIONINFO` block `data`.
fn fixed_version(data: &[u8]) -> Option<[u16; 4]> {
    let key: Vec<u8> = "VS_VERSION_INFO\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    if data.get(6..6 + key.len())? != key.as_slice() {
        return None;
    }
    let value_length = usize::from(u16_at(data, 2)?);
    let fixed = (6 + key.len()).next_multiple_of(4);
    if value_length < 52 || u32_at(data, fixed)? != FIXED_SIGNATURE {
        return None;
    }
    let ms = u32_at(data, fixed + 8)?;
    let ls = u32_at(data, fixed + 12)?;
    let part = |word: u32, high: bool| {
        let [a, b, c, d] = word.to_be_bytes();
        if high {
            u16::from_be_bytes([a, b])
        } else {
            u16::from_be_bytes([c, d])
        }
    };
    Some([
        part(ms, true),
        part(ms, false),
        part(ls, true),
        part(ls, false),
    ])
}

#[cfg(test)]
mod tests;
