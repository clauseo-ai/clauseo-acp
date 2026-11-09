//! A minimal, bounds-checked reader of a PE file's headers: its machine, its linker's version and
//! the names of the DLLs it imports and delay-imports. Every read goes through `get`, so a
//! truncated or hostile file is an error, never a panic or an out-of-bounds read. Nothing else of
//! the file is interpreted, and nothing in it runs.

use std::fmt;

/// `IMAGE_FILE_MACHINE_ARM64`.
pub(crate) const MACHINE_ARM64: u16 = 0xAA64;
/// The import table's index among the optional header's data directories.
const IMPORT_DIR: usize = 1;
/// The delay-import table's index.
const DELAY_IMPORT_DIR: usize = 13;
/// Far more descriptors than any real file has; a table with no terminator stops here.
const MAX_DESCRIPTORS: usize = 4096;
/// The longest DLL name read.
const MAX_NAME: usize = 512;
/// More sections than any real file has (the format's own limit is 96 for the loader).
const MAX_SECTIONS: usize = 96;

/// What [`parse`] read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeInfo {
    /// The COFF header's machine (`0xAA64` for ARM64).
    pub(crate) machine: u16,
    /// The optional header's linker version, major then minor (MSVC 14.44 is `(14, 44)`).
    pub(crate) linker: (u8, u8),
    /// The imported DLLs' names, as the file spells them.
    pub(crate) imports: Vec<String>,
    /// The delay-imported DLLs' names.
    pub(crate) delay_imports: Vec<String>,
}

impl PeInfo {
    /// Every DLL the file loads, imported or delay-imported.
    pub(crate) fn all_imports(&self) -> impl Iterator<Item = &str> {
        self.imports
            .iter()
            .chain(&self.delay_imports)
            .map(String::as_str)
    }
}

/// Why a file is not a PE file this reader understands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NotPe(pub(crate) &'static str);

impl fmt::Display for NotPe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a PE file: {}", self.0)
    }
}

/// One section: its virtual address and size, and where its bytes are in the file.
struct Section {
    va: u32,
    virtual_size: u32,
    raw_ptr: u32,
    raw_size: u32,
}

/// Reads `bytes` as a PE file.
pub(crate) fn parse(bytes: &[u8]) -> Result<PeInfo, NotPe> {
    if get(bytes, 0, 2) != Some(b"MZ".as_slice()) {
        return Err(NotPe("no MZ header"));
    }
    let pe = to_usize(u32_at(bytes, 0x3c).ok_or(NotPe("no PE offset"))?);
    if get(bytes, pe, 4) != Some(b"PE\0\0".as_slice()) {
        return Err(NotPe("no PE signature"));
    }
    let coff = pe.checked_add(4).ok_or(NotPe("bad PE offset"))?;
    let machine = u16_at(bytes, coff).ok_or(NotPe("no COFF header"))?;
    let sections = usize::from(u16_at(bytes, coff + 2).ok_or(NotPe("no COFF header"))?);
    let opt_size = usize::from(u16_at(bytes, coff + 16).ok_or(NotPe("no COFF header"))?);
    let opt = coff + 20;
    let magic = u16_at(bytes, opt).ok_or(NotPe("no optional header"))?;
    let (base_at, count_at, dirs_at, pe32_plus) = match magic {
        0x10b => (opt + 28, opt + 92, opt + 96, false),
        0x20b => (opt + 24, opt + 108, opt + 112, true),
        _ => return Err(NotPe("unknown optional header")),
    };
    let linker = match get(bytes, opt + 2, 2) {
        Some(&[major, minor]) => (major, minor),
        _ => return Err(NotPe("no linker version")),
    };
    let image_base = if pe32_plus {
        u64_at(bytes, base_at)
    } else {
        u32_at(bytes, base_at).map(u64::from)
    }
    .ok_or(NotPe("no image base"))?;
    let dir_count = to_usize(u32_at(bytes, count_at).ok_or(NotPe("no directory count"))?);
    if dirs_at.saturating_add(dir_count.saturating_mul(8)) > opt.saturating_add(opt_size) {
        return Err(NotPe("directories past the optional header"));
    }
    if sections > MAX_SECTIONS {
        return Err(NotPe("too many sections"));
    }
    let table = opt + opt_size;
    let mut sects = Vec::with_capacity(sections);
    for index in 0..sections {
        let at = table + index * 40;
        let field = |offset: usize| u32_at(bytes, at + offset).ok_or(NotPe("truncated sections"));
        sects.push(Section {
            virtual_size: field(8)?,
            va: field(12)?,
            raw_size: field(16)?,
            raw_ptr: field(20)?,
        });
    }
    let dir = |index: usize| -> Result<Option<u32>, NotPe> {
        if index >= dir_count {
            return Ok(None);
        }
        let rva = u32_at(bytes, dirs_at + index * 8).ok_or(NotPe("truncated directories"))?;
        Ok((rva != 0).then_some(rva))
    };
    let reader = Reader {
        bytes,
        sections: &sects,
    };
    let imports = match dir(IMPORT_DIR)? {
        Some(rva) => reader.names(rva, 20, 12, |_, name| Some(name))?,
        None => Vec::new(),
    };
    let delay_imports = match dir(DELAY_IMPORT_DIR)? {
        // `Attributes` bit 0 set: the name is an RVA; clear (Visual C++ 6): a VA.
        Some(rva) => reader.names(rva, 32, 4, |attrs, name| {
            if attrs & 1 == 1 {
                Some(name)
            } else {
                u32::try_from(u64::from(name).checked_sub(image_base)?).ok()
            }
        })?,
        None => Vec::new(),
    };
    Ok(PeInfo {
        machine,
        linker,
        imports,
        delay_imports,
    })
}

/// Reads through the section table.
struct Reader<'a> {
    bytes: &'a [u8],
    sections: &'a [Section],
}

impl Reader<'_> {
    /// The file offset of `rva`, when a section's file bytes hold it.
    fn offset(&self, rva: u32) -> Option<usize> {
        self.sections.iter().find_map(|section| {
            let within = rva.checked_sub(section.va)?;
            let span = section.virtual_size.max(section.raw_size);
            (within < span && within < section.raw_size)
                .then(|| to_usize(section.raw_ptr).checked_add(to_usize(within)))
                .flatten()
        })
    }

    /// The DLL names of the descriptor table at `rva`: descriptors of `size` bytes, the name's
    /// RVA at `name_at` in each (mapped by `name_rva` from the descriptor's first word and the
    /// field), up to the first descriptor whose name field is zero.
    fn names(
        &self,
        rva: u32,
        size: usize,
        name_at: usize,
        name_rva: impl Fn(u32, u32) -> Option<u32>,
    ) -> Result<Vec<String>, NotPe> {
        let start = self
            .offset(rva)
            .ok_or(NotPe("import table outside the file"))?;
        let mut names = Vec::new();
        for index in 0..MAX_DESCRIPTORS {
            let at = start + index * size;
            let first = u32_at(self.bytes, at).ok_or(NotPe("truncated import table"))?;
            let field = u32_at(self.bytes, at + name_at).ok_or(NotPe("truncated import table"))?;
            if field == 0 {
                return Ok(names);
            }
            let name = name_rva(first, field)
                .and_then(|rva| self.offset(rva))
                .ok_or(NotPe("import name outside the file"))?;
            names.push(self.c_string(name)?);
        }
        Err(NotPe("import table has no end"))
    }

    /// The NUL-terminated ASCII string at file offset `at`.
    fn c_string(&self, at: usize) -> Result<String, NotPe> {
        let rest = self.bytes.get(at..).ok_or(NotPe("name outside the file"))?;
        let rest = rest.get(..MAX_NAME.min(rest.len())).unwrap_or_default();
        let end = rest
            .iter()
            .position(|&byte| byte == 0)
            .ok_or(NotPe("unterminated name"))?;
        let name = rest.get(..end).unwrap_or_default();
        if !name.is_ascii() {
            return Err(NotPe("non-ASCII name"));
        }
        Ok(String::from_utf8_lossy(name).into_owned())
    }
}

/// `len` bytes at `at`, when the file has them.
fn get(bytes: &[u8], at: usize, len: usize) -> Option<&[u8]> {
    bytes.get(at..at.checked_add(len)?)
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    get(bytes, at, 2)?.try_into().ok().map(u16::from_le_bytes)
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    get(bytes, at, 4)?.try_into().ok().map(u32::from_le_bytes)
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    get(bytes, at, 8)?.try_into().ok().map(u64::from_le_bytes)
}

/// `value` as a `usize`; every target this crate builds for has 32 bits or more.
fn to_usize(value: u32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}
