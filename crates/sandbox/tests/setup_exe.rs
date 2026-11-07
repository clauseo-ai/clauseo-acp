//! The setup exe as built (S15 `PR4h`). On Windows (msvc) its embedded manifest asks for an
//! administrator (D-B14): the linker writes the manifest's XML into the exe's `RT_MANIFEST`
//! resource as it is, so the exe's bytes hold it. Off Windows the exe exits `Usage` (1) with
//! one line. Its PE load configuration keeps direct static imports to System32
//! (`DependentLoadFlags` = `LOAD_LIBRARY_SEARCH_SYSTEM32`, `/DEPENDENTLOADFLAG:0x800`), and
//! the imports whose own imports aren't `KnownDLLs` are delay-loaded (`build.rs`, ADR 0049).
//! Its `VERSIONINFO` names it, and its one `RT_MANIFEST` is the linker's, `setup.manifest`'s
//! supported Windows versions merged in. Its `DllCharacteristics` keep ASLR (high-entropy) and DEP.

// Used by the msvc tests and the off-Windows one; windows-gnu builds neither.
#[cfg(any(not(windows), target_env = "msvc"))]
const EXE: &str = env!("CARGO_BIN_EXE_clauseo-sandbox-setup");

#[cfg(all(windows, target_env = "msvc"))]
#[test]
fn the_embedded_manifest_requires_an_administrator() {
    let bytes = std::fs::read(EXE).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    let at = text
        .find("requestedExecutionLevel")
        .expect("the exe embeds a manifest with a requested execution level");
    let element: String = text[at..].chars().take(120).collect();
    assert!(
        element.contains("level='requireAdministrator'")
            || element.contains("level=\"requireAdministrator\""),
        "{element}"
    );
    assert!(
        element.contains("uiAccess='false'") || element.contains("uiAccess=\"false\""),
        "{element}"
    );
    assert!(
        !text.contains("asInvoker"),
        "the manifest never asks to run as the invoker"
    );
}

/// Reads the exe's PE image without FFI.
#[cfg(all(windows, target_env = "msvc"))]
#[cfg(test)]
mod pe {
    /// `bytes[at..at + N]`; `None` past the end.
    fn le<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
        bytes.get(at..at + N).and_then(|part| part.try_into().ok())
    }

    /// The exe's PE image as bytes, read without FFI.
    pub(crate) struct Pe {
        bytes: Vec<u8>,
        /// The PE32+ optional header's offset.
        optional: usize,
        /// The section table's offset, and its number of sections.
        table: usize,
        sections: usize,
    }

    impl Pe {
        pub(crate) fn read() -> Self {
            let bytes = std::fs::read(super::EXE).unwrap();
            let mut pe = Self {
                bytes,
                optional: 0,
                table: 0,
                sections: 0,
            };
            let header = pe.u32_at(0x3C);
            assert_eq!(le::<4>(&pe.bytes, header), Some(*b"PE\0\0"));
            pe.sections = pe.u16_at(header + 6);
            pe.optional = header + 24;
            pe.table = pe.optional + pe.u16_at(header + 20);
            assert_eq!(pe.u16_at(pe.optional), 0x20B, "a PE32+ image");
            pe
        }

        pub(crate) fn u16_at(&self, at: usize) -> usize {
            usize::from(u16::from_le_bytes(le::<2>(&self.bytes, at).unwrap()))
        }

        pub(crate) fn u32_at(&self, at: usize) -> usize {
            usize::try_from(u32::from_le_bytes(le::<4>(&self.bytes, at).unwrap())).unwrap()
        }

        /// The optional header's `DllCharacteristics`, 70 bytes into it.
        pub(crate) fn dll_characteristics(&self) -> usize {
            self.u16_at(self.optional + 70)
        }

        /// Data directory `index`'s RVA: they start 112 bytes into a PE32+ optional header, eight
        /// bytes each.
        pub(crate) fn directory(&self, index: usize) -> usize {
            self.u32_at(self.optional + 112 + index * 8)
        }

        /// Data directory `index`'s size in bytes, the four bytes after its RVA.
        pub(crate) fn directory_size(&self, index: usize) -> usize {
            self.u32_at(self.optional + 112 + index * 8 + 4)
        }

        /// The `Type` of each entry in the debug directory (data directory 6): 28-byte
        /// `IMAGE_DEBUG_DIRECTORY` entries, the type 12 bytes into each. Empty when absent.
        pub(crate) fn debug_types(&self) -> Vec<usize> {
            let rva = self.directory(6);
            if rva == 0 {
                return Vec::new();
            }
            let table = self.offset(rva);
            (0..self.directory_size(6) / 28)
                .map(|n| self.u32_at(table + n * 28 + 12))
                .collect()
        }

        /// The image's bytes.
        pub(crate) fn bytes(&self) -> &[u8] {
            &self.bytes
        }

        /// The RVA to a file offset, through the section that holds it.
        pub(crate) fn offset(&self, rva: usize) -> usize {
            (0..self.sections)
                .map(|n| self.table + n * 40)
                .find_map(|at| {
                    let (virtual_size, start) = (self.u32_at(at + 8), self.u32_at(at + 12));
                    let raw = self.u32_at(at + 20);
                    (start..start + virtual_size)
                        .contains(&rva)
                        .then(|| rva - start + raw)
                })
                .expect("a section holds the RVA")
        }

        /// The NUL-terminated ASCII name at `rva`, lowercased.
        pub(crate) fn name_at(&self, rva: usize) -> String {
            let at = self.offset(rva);
            let end = self.bytes[at..]
                .iter()
                .position(|&byte| byte == 0)
                .expect("a NUL-terminated name");
            String::from_utf8_lossy(&self.bytes[at..at + end]).to_ascii_lowercase()
        }

        /// The DLL names of the table of `size`-byte descriptors that data directory `index`
        /// points at, each name's RVA `name` bytes into its descriptor; the table ends at a
        /// descriptor whose name RVA is 0. None when the directory is absent.
        pub(crate) fn dll_names(&self, index: usize, size: usize, name: usize) -> Vec<String> {
            let rva = self.directory(index);
            if rva == 0 {
                return Vec::new();
            }
            let table = self.offset(rva);
            let mut names = Vec::new();
            let mut at = table;
            loop {
                let name_rva = self.u32_at(at + name);
                if name_rva == 0 {
                    return names;
                }
                names.push(self.name_at(name_rva));
                at += size;
            }
        }

        /// The data of every resource of type `id` (`RT_VERSION` 16, `RT_MANIFEST` 24), under
        /// each of its names and languages. The resource directory is data directory 2: a tree
        /// of `IMAGE_RESOURCE_DIRECTORY` tables (16 bytes, then 8-byte entries), each entry's
        /// offset from the tree's start, its top bit set for a subtable; a leaf is an
        /// `IMAGE_RESOURCE_DATA_ENTRY`, its data's RVA and size.
        pub(crate) fn resources(&self, id: usize) -> Vec<Vec<u8>> {
            let rva = self.directory(2);
            if rva == 0 {
                return Vec::new();
            }
            let root = self.offset(rva);
            let entries = |table: usize| {
                let count = self.u16_at(table + 12) + self.u16_at(table + 14);
                (0..count).map(move |n| (self.u32_at(table + 16 + n * 8), table + 16 + n * 8))
            };
            let below = |entry: usize| self.u32_at(entry + 4);
            let mut leaves = Vec::new();
            let mut tables: Vec<usize> = entries(root)
                .filter(|&(name, _)| name == id)
                .map(|(_, entry)| below(entry))
                .collect();
            while let Some(next) = tables.pop() {
                if next & 0x8000_0000 == 0 {
                    leaves.push(root + next);
                    continue;
                }
                tables.extend(entries(root + (next & 0x7FFF_FFFF)).map(|(_, e)| below(e)));
            }
            leaves
                .into_iter()
                .map(|leaf| {
                    let at = self.offset(self.u32_at(leaf));
                    self.bytes[at..at + self.u32_at(leaf + 4)].to_vec()
                })
                .collect()
        }
    }
}

#[cfg(all(windows, target_env = "msvc"))]
use pe::Pe;

#[cfg(all(windows, target_env = "msvc"))]
#[test]
fn static_imports_load_from_system32_only() {
    let pe = Pe::read();
    // LOAD_CONFIG is the eleventh data directory (index 10).
    let load_config_rva = pe.directory(10);
    assert_ne!(load_config_rva, 0, "the exe has a load configuration");
    let offset = pe.offset(load_config_rva);
    // IMAGE_LOAD_CONFIG_DIRECTORY64: its Size first, DependentLoadFlags (a u16) at 0x4E.
    assert!(
        pe.u32_at(offset) > 0x4E + 2,
        "the load configuration reaches DependentLoadFlags"
    );
    assert_eq!(
        pe.u16_at(offset + 0x4E),
        0x800,
        "LOAD_LIBRARY_SEARCH_SYSTEM32"
    );
}

#[cfg(all(windows, target_env = "msvc"))]
#[test]
fn the_exe_has_aslr_with_high_entropy_and_dep() {
    let pe = Pe::read();
    let flags = pe.dll_characteristics();
    for (flag, name) in [
        (0x20, "HIGH_ENTROPY_VA"),
        (0x40, "DYNAMIC_BASE"),
        (0x100, "NX_COMPAT"),
    ] {
        assert_eq!(flags & flag, flag, "{name} is off: {flags:#x}");
    }
}

/// The debug-directory entry types the exe may carry without a PDB (`IMAGE_DEBUG_TYPE_*`).
/// `/Brepro` always adds a `REPRO` (16) entry holding the image's hash, `/DEBUG:NONE` or not.
/// The linker may also add, without `/DEBUG`, `VC_FEATURE` (12) and `POGO` (13) from MSVC-built
/// objects such as the CRT's, `ILTCG` (14) for LTCG objects, and `EX_DLLCHARACTERISTICS` (20)
/// for `/CETCOMPAT`. None names a file or holds a path. `CODEVIEW` (2) is what names the PDB.
#[cfg(all(windows, target_env = "msvc"))]
const DEBUG_TYPES_WITHOUT_A_PDB: [usize; 5] = [12, 13, 14, 16, 20];

/// No PDB, so no `CODEVIEW` entry: its GUID would hash rustc's random temp paths (`build.rs`).
#[cfg(all(windows, target_env = "msvc"))]
#[test]
fn the_exe_names_no_pdb() {
    let pe = Pe::read();
    let types = pe.debug_types();
    assert!(
        !types.contains(&2),
        "a CodeView entry: /DEBUG:NONE didn't reach the link: {types:?}"
    );
    assert!(
        types
            .iter()
            .all(|kind| DEBUG_TYPES_WITHOUT_A_PDB.contains(kind)),
        "a debug entry of a type not vetted as PDB-free: {types:?}"
    );
    let text = String::from_utf8_lossy(pe.bytes()).to_ascii_lowercase();
    for pdb in ["clauseo-sandbox-setup.pdb", "clauseo_sandbox_setup.pdb"] {
        assert!(!text.contains(pdb), "the exe names {pdb}");
    }
}

/// The imports that load after `main` has kept the DLL search to System32 (`build.rs`).
#[cfg(all(windows, target_env = "msvc"))]
const DELAYED: [&str; 2] = ["netapi32.dll", "userenv.dll"];

/// The normal imports the loader may resolve before `main`, when `DependentLoadFlags` covers
/// only the import itself and not what it imports. Each is a `KnownDLL` on Windows 10 and 11
/// (always mapped from System32, with its own imports), or imports nothing but `KnownDLLs` and
/// API sets. `ntdll.dll` is in every process before the exe's imports; `kernelbase.dll` comes
/// with kernel32; `bcryptprimitives.dll`, `crypt32.dll` and `vcruntime140.dll` import only
/// `KnownDLLs` and API sets (the VM row's Process Monitor trace showed none of their imports
/// searched beside the exe). API sets (`api-ms-win-*`, the UCRT's `api-ms-win-crt-*`
/// included) resolve through the system's API set schema, never a folder search.
#[cfg(all(windows, target_env = "msvc"))]
const SAFE_BEFORE_MAIN: [&str; 22] = [
    // KnownDLLs (HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\KnownDLLs).
    "advapi32.dll",
    "combase.dll",
    "gdi32.dll",
    "kernel32.dll",
    "msvcrt.dll",
    "nsi.dll",
    "ole32.dll",
    "oleaut32.dll",
    "psapi.dll",
    "rpcrt4.dll",
    "sechost.dll",
    "setupapi.dll",
    "shcore.dll",
    "shell32.dll",
    "shlwapi.dll",
    "user32.dll",
    "ws2_32.dll",
    // Not KnownDLLs, but nothing they import is searched beside the exe.
    "ntdll.dll",
    "kernelbase.dll",
    "bcryptprimitives.dll",
    "crypt32.dll",
    "vcruntime140.dll",
];

#[cfg(all(windows, target_env = "msvc"))]
#[test]
fn imports_that_import_beyond_the_known_dlls_are_delay_loaded() {
    let pe = Pe::read();
    // IMAGE_DIRECTORY_ENTRY_IMPORT (index 1): 20-byte descriptors, the name's RVA at 12.
    let imports = pe.dll_names(1, 20, 12);
    // IMAGE_DIRECTORY_ENTRY_DELAY_IMPORT (index 13): 32-byte descriptors, the name's RVA at 4.
    let delayed = pe.dll_names(13, 32, 4);
    assert!(!imports.is_empty(), "the exe has an import directory");
    for dll in DELAYED {
        assert!(
            !imports.iter().any(|import| import == dll),
            "{dll} is a normal import, loaded before main with its own imports searched \
             beside the exe: {imports:?}"
        );
        assert!(
            delayed.iter().any(|import| import == dll),
            "{dll} isn't delay-loaded (LNK4199 if the linker saw no import of it): {delayed:?}"
        );
    }
    let unvetted: Vec<&String> = imports
        .iter()
        .filter(|import| {
            !import.starts_with("api-ms-win-") && !SAFE_BEFORE_MAIN.contains(&import.as_str())
        })
        .collect();
    assert!(
        unvetted.is_empty(),
        "new import(s) {unvetted:?}: a DLL loaded before main whose own imports aren't all \
         KnownDLLs would be searched for beside the elevated exe. Check its imports; delay-load \
         it in build.rs, or add it to SAFE_BEFORE_MAIN if they are all KnownDLLs."
    );
}

/// The value of `key` in a `VERSIONINFO`'s UTF-16 strings: the key, its NUL, padding NULs to a
/// 4-byte boundary, then the value up to its NUL.
#[cfg(all(windows, target_env = "msvc"))]
fn version_string(version: &[u8], key: &str) -> Option<String> {
    let units: Vec<u16> = version
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .collect();
    let needle: Vec<u16> = key.encode_utf16().chain([0]).collect();
    let at = units.windows(needle.len()).position(|w| w == needle)? + needle.len();
    let value: Vec<u16> = units[at..]
        .iter()
        .skip_while(|&&unit| unit == 0)
        .take_while(|&&unit| unit != 0)
        .copied()
        .collect();
    String::from_utf16(&value).ok()
}

#[cfg(all(windows, target_env = "msvc"))]
#[test]
fn the_exe_carries_version_information_and_one_manifest() {
    let pe = Pe::read();
    let versions = pe.resources(16);
    assert_eq!(versions.len(), 1, "one VERSIONINFO (build.rs)");
    let version = &versions[0];
    assert_eq!(
        version_string(version, "FileDescription").as_deref(),
        Some("Clauseo sandbox set-up")
    );
    assert_eq!(
        version_string(version, "ProductName").as_deref(),
        Some("Clauseo")
    );
    assert_eq!(
        version_string(version, "OriginalFilename").as_deref(),
        Some("clauseo-sandbox-setup.exe")
    );
    let manifests = pe.resources(24);
    assert_eq!(
        manifests.len(),
        1,
        "the linker's manifest only, none from the .rc"
    );
    let manifest = String::from_utf8_lossy(&manifests[0]);
    assert!(
        manifest.contains("level='requireAdministrator'")
            || manifest.contains("level=\"requireAdministrator\""),
        "{manifest}"
    );
    for (os, id) in SUPPORTED_OS {
        assert!(
            manifest.contains(id),
            "setup.manifest's {os} supportedOS: {manifest}"
        );
    }
}

/// The `supportedOS` IDs `setup.manifest` declares: Windows 10 and 11, 8.1, 8 and 7.
const SUPPORTED_OS: [(&str, &str); 4] = [
    (
        "Windows 10 and 11",
        "{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}",
    ),
    ("Windows 8.1", "{1f676c76-80e1-4239-95bb-83d0f6d0da78}"),
    ("Windows 8", "{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}"),
    ("Windows 7", "{35138b9a-5d96-4fbd-8e2d-a2440225f93a}"),
];

/// The manifest input itself, on every platform: the msvc test above sees it only once linked.
#[test]
fn the_manifest_declares_windows_7_to_11() {
    let manifest = include_str!("../setup.manifest");
    for (os, id) in SUPPORTED_OS {
        let line = format!("<supportedOS Id=\"{id}\"/>");
        assert!(manifest.contains(&line), "{os}: {manifest}");
    }
}

#[cfg(not(windows))]
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test runs the built exe itself"
)]
fn off_windows_the_exe_exits_usage_with_one_line() {
    let output = std::process::Command::new(EXE)
        .arg("--set-up")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(stderr.contains("Windows"), "{stderr}");
}
