//! The fake Windows checks and the fake install folder the tests share.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use super::{RG_IMPORTS, RG_LINKER, SHIPPED, pe_file};
use crate::vcruntime::pe::MACHINE_ARM64;
use crate::vcruntime::{Inspect, Signer, VENDOR, VersionInfo, rel};
use crate::write::TestDir;

/// The `OriginalFilename` a fake version resource gives.
#[derive(Clone)]
pub(super) enum Original {
    /// The DLL's own name, read from the copy's `clauseo-tmp.<dll>` name.
    OwnName,
    /// This one, or none.
    Given(Option<&'static str>),
}

/// The fake Windows checks: one answer each, and every path they were asked about.
pub(super) struct Fake {
    pub(super) signer: Signer,
    pub(super) version: Option<[u16; 4]>,
    pub(super) original: Original,
    /// A file written here as the signer is asked: one appearing after the look beside rg.exe.
    pub(super) plant: Option<PathBuf>,
    pub(super) asked: RefCell<Vec<PathBuf>>,
}

impl Fake {
    pub(super) fn new(signer: Signer, version: Option<[u16; 4]>) -> Fake {
        Fake {
            signer,
            version,
            original: Original::OwnName,
            plant: None,
            asked: RefCell::new(Vec::new()),
        }
    }

    /// Microsoft-signed, the shipped version.
    pub(super) fn good() -> Fake {
        Fake::new(Signer::Microsoft, Some(SHIPPED))
    }
}

impl Inspect for Fake {
    fn signer(&self, path: &Path) -> Signer {
        self.asked.borrow_mut().push(path.to_path_buf());
        if let Some(plant) = &self.plant {
            crate::write::write_atomic(plant, b"appeared meanwhile").unwrap();
        }
        self.signer.clone()
    }

    fn version_info(&self, path: &Path) -> Option<VersionInfo> {
        self.asked.borrow_mut().push(path.to_path_buf());
        let original_filename = match &self.original {
            Original::OwnName => path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix("clauseo-tmp."))
                .map(str::to_owned),
            Original::Given(name) => name.map(str::to_owned),
        };
        Some(VersionInfo {
            version: self.version?,
            original_filename,
        })
    }
}

/// An install folder holding Codex's ARM64 layout.
pub(super) struct Layout {
    pub(super) dir: TestDir,
}

impl Layout {
    /// `rg.exe` with `rg_imports` in `rg_dir`, and the voice host's DLLs (name, machine).
    pub(super) fn new(rg_dir: &str, rg_imports: &[&str], sources: &[(&str, u16)]) -> Layout {
        let dir = TestDir::new("vcruntime");
        let vendor = VENDOR;
        dir.file(
            &format!("{vendor}/{rg_dir}/rg.exe"),
            &pe_file(MACHINE_ARM64, RG_LINKER, rg_imports, &[]),
        );
        for (name, machine) in sources {
            dir.file(
                &format!("{vendor}/codex-resources/voice/bin/{name}"),
                &pe_file(*machine, (14, 50), &["KERNEL32.dll"], &[]),
            );
        }
        Layout { dir }
    }

    /// The usual layout: rg in `codex-path`, an ARM64 vcruntime140.dll in the voice host.
    pub(super) fn usual() -> Layout {
        Layout::new(
            "codex-path",
            &RG_IMPORTS,
            &[("vcruntime140.dll", MACHINE_ARM64)],
        )
    }

    pub(super) fn root(&self) -> &Path {
        &self.dir.0
    }

    pub(super) fn vendor(&self) -> PathBuf {
        rel(self.root(), VENDOR)
    }

    pub(super) fn rg_dir(&self, name: &str) -> PathBuf {
        self.vendor().join(name)
    }

    pub(super) fn source(&self, dll: &str) -> PathBuf {
        self.vendor().join("codex-resources/voice/bin").join(dll)
    }

    /// The file names in `rg_dir`, sorted.
    pub(super) fn names(&self, rg_dir: &str) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.rg_dir(rg_dir))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}
