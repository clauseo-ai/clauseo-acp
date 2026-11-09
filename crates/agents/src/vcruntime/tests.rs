use std::cell::RefCell;
use std::path::{Path, PathBuf};

use pe_file::pe_file;

/// The PE files the tests use, and the PE reader's own tests.
mod pe_file;
/// Windows only: the real `WinVerifyTrust` and version calls.
#[cfg(windows)]
mod windows;

use super::pe::{self, MACHINE_ARM64};
use super::{
    Inspect, Outcome, PLACED_DLLS, Refused, Signer, VENDOR, check, is_forbidden, missing_beside,
    place_with, rel,
};
use crate::platform::{Arch, Os, Platform};
use crate::write::TestDir;

const X64: u16 = 0x8664;
const WIN_ARM64: Platform = Platform::new(Os::Win32, Arch::Arm64);
/// The version the pinned package's copy has.
const SHIPPED: [u16; 4] = [14, 50, 35719, 0];
/// ripgrep 15.2.0's linker version.
const RG_LINKER: (u8, u8) = (14, 44);

/// ripgrep 15.2.0's ARM64 imports, as its `rg.exe` lists them.
const RG_IMPORTS: [&str; 7] = [
    "kernel32.dll",
    "ntdll.dll",
    "bcryptprimitives.dll",
    "USERENV.dll",
    "VCRUNTIME140.dll",
    "api-ms-win-crt-runtime-l1-1-0.dll",
    "api-ms-win-crt-heap-l1-1-0.dll",
];

/// The fake Windows checks: one answer each, and every path they were asked about.
struct Fake {
    signer: Signer,
    version: Option<[u16; 4]>,
    asked: RefCell<Vec<PathBuf>>,
}

impl Fake {
    fn new(signer: Signer, version: Option<[u16; 4]>) -> Fake {
        Fake {
            signer,
            version,
            asked: RefCell::new(Vec::new()),
        }
    }

    /// Microsoft-signed, the shipped version.
    fn good() -> Fake {
        Fake::new(Signer::Microsoft, Some(SHIPPED))
    }
}

impl Inspect for Fake {
    fn signer(&self, path: &Path) -> Signer {
        self.asked.borrow_mut().push(path.to_path_buf());
        self.signer.clone()
    }

    fn file_version(&self, path: &Path) -> Option<[u16; 4]> {
        self.asked.borrow_mut().push(path.to_path_buf());
        self.version
    }
}

/// An install folder holding Codex's ARM64 layout.
struct Layout {
    dir: TestDir,
}

impl Layout {
    /// `rg.exe` with `rg_imports` in `rg_dir`, and the voice host's DLLs (name, machine).
    fn new(rg_dir: &str, rg_imports: &[&str], sources: &[(&str, u16)]) -> Layout {
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
    fn usual() -> Layout {
        Layout::new(
            "codex-path",
            &RG_IMPORTS,
            &[("vcruntime140.dll", MACHINE_ARM64)],
        )
    }

    fn root(&self) -> &Path {
        &self.dir.0
    }

    fn vendor(&self) -> PathBuf {
        rel(self.root(), VENDOR)
    }

    fn rg_dir(&self, name: &str) -> PathBuf {
        self.vendor().join(name)
    }

    fn source(&self, dll: &str) -> PathBuf {
        self.vendor().join("codex-resources/voice/bin").join(dll)
    }

    /// The file names in `rg_dir`, sorted.
    fn names(&self, rg_dir: &str) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.rg_dir(rg_dir))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

fn outcomes(found: Vec<(PathBuf, Outcome)>) -> Vec<Outcome> {
    found.into_iter().map(|(_, outcome)| outcome).collect()
}

#[test]
fn places_a_checked_copy_beside_rg() {
    let layout = Layout::usual();
    let rg_before = std::fs::read(layout.rg_dir("codex-path").join("rg.exe")).unwrap();
    let fake = Fake::good();
    let found = place_with(WIN_ARM64, layout.root(), &fake);
    assert_eq!(
        found,
        vec![(
            layout.rg_dir("codex-path"),
            Outcome::Placed("vcruntime140.dll".to_owned(), SHIPPED)
        )]
    );
    let placed = layout.rg_dir("codex-path").join("vcruntime140.dll");
    assert_eq!(
        std::fs::read(&placed).unwrap(),
        std::fs::read(layout.source("vcruntime140.dll")).unwrap()
    );
    // A copy, not a hard link: a Codex update may replace the source.
    assert!(!crate::same_file(&placed, &layout.source("vcruntime140.dll")).unwrap());
    // The checks ran on the copy beside rg.exe, never on the source.
    let copy = layout
        .rg_dir("codex-path")
        .join("clauseo-tmp.vcruntime140.dll");
    let asked = fake.asked.borrow();
    assert!(!asked.is_empty());
    assert!(asked.iter().all(|path| *path == copy), "{asked:?}");
    // rg.exe is never changed, and nothing else is left beside it.
    assert_eq!(
        std::fs::read(layout.rg_dir("codex-path").join("rg.exe")).unwrap(),
        rg_before
    );
    assert_eq!(layout.names("codex-path"), ["rg.exe", "vcruntime140.dll"]);
}

/// Stage F's `pe-crt-imports.mjs --app-local-ok` rule: after the step, every VC++ runtime DLL
/// rg.exe imports has a same-named file beside it.
#[test]
fn every_forbidden_import_of_rg_is_beside_it_after_the_step() {
    let layout = Layout::new(
        "codex-path",
        &RG_IMPORTS,
        &[
            ("vcruntime140.dll", MACHINE_ARM64),
            ("vcruntime140_1.dll", MACHINE_ARM64),
        ],
    );
    let rg_dir = layout.rg_dir("codex-path");
    assert_eq!(missing_beside(&rg_dir).unwrap(), ["vcruntime140.dll"]);
    place_with(WIN_ARM64, layout.root(), &Fake::good());
    assert_eq!(missing_beside(&rg_dir).unwrap(), Vec::<String>::new());

    // Imported and delay-imported, both runtime DLLs: both placed.
    let both = TestDir::new("vcruntime-both");
    both.file(
        &format!("{VENDOR}/codex-path/rg.exe"),
        &pe_file(
            MACHINE_ARM64,
            RG_LINKER,
            &["VCRUNTIME140.dll"],
            &["vcruntime140_1.dll"],
        ),
    );
    for dll in PLACED_DLLS {
        both.file(
            &format!("{VENDOR}/codex-resources/voice/bin/{dll}"),
            &pe_file(MACHINE_ARM64, (14, 50), &[], &[]),
        );
    }
    let rg_dir = rel(&both.0, VENDOR).join("codex-path");
    assert_eq!(missing_beside(&rg_dir).unwrap().len(), 2);
    let found = outcomes(place_with(WIN_ARM64, &both.0, &Fake::good()));
    assert_eq!(
        found,
        [
            Outcome::Placed("vcruntime140.dll".to_owned(), SHIPPED),
            Outcome::Placed("vcruntime140_1.dll".to_owned(), SHIPPED),
        ]
    );
    assert_eq!(missing_beside(&rg_dir).unwrap(), Vec::<String>::new());
}

#[test]
fn a_source_of_the_wrong_arch_is_refused_and_removed() {
    let layout = Layout::new("codex-path", &RG_IMPORTS, &[("vcruntime140.dll", X64)]);
    let found = outcomes(place_with(WIN_ARM64, layout.root(), &Fake::good()));
    assert_eq!(
        found,
        [Outcome::Refused(
            "vcruntime140.dll".to_owned(),
            Refused::WrongArch(X64)
        )]
    );
    assert_eq!(layout.names("codex-path"), ["rg.exe"]);
    assert_eq!(
        missing_beside(&layout.rg_dir("codex-path")).unwrap(),
        ["vcruntime140.dll"]
    );
}

#[test]
fn an_unsigned_source_is_refused() {
    let layout = Layout::usual();
    let fake = Fake::new(
        Signer::NotTrusted("WinVerifyTrust 0x800B0100".to_owned()),
        Some(SHIPPED),
    );
    let found = outcomes(place_with(WIN_ARM64, layout.root(), &fake));
    assert_eq!(
        found,
        [Outcome::Refused(
            "vcruntime140.dll".to_owned(),
            Refused::NotTrusted("WinVerifyTrust 0x800B0100".to_owned())
        )]
    );
    assert_eq!(layout.names("codex-path"), ["rg.exe"]);
}

#[test]
fn a_source_signed_by_someone_else_is_refused() {
    let layout = Layout::usual();
    let fake = Fake::new(
        Signer::Other("organisation \"Contoso\"".to_owned()),
        Some(SHIPPED),
    );
    let found = outcomes(place_with(WIN_ARM64, layout.root(), &fake));
    assert!(matches!(
        found.as_slice(),
        [Outcome::Refused(_, Refused::NotMicrosoft(who))] if who.contains("Contoso")
    ));
    assert_eq!(layout.names("codex-path"), ["rg.exe"]);
}

#[test]
fn an_old_or_foreign_version_is_refused() {
    for (version, refused) in [
        (
            Some([14, 42, 34438, 0]),
            Refused::TooOld([14, 42, 34438, 0], (14, 44)),
        ),
        (Some([15, 0, 0, 0]), Refused::NotRuntime([15, 0, 0, 0])),
        (
            Some([10, 0, 26100, 1]),
            Refused::NotRuntime([10, 0, 26100, 1]),
        ),
        (None, Refused::NoVersion),
    ] {
        let layout = Layout::usual();
        let fake = Fake::new(Signer::Microsoft, version);
        let found = outcomes(place_with(WIN_ARM64, layout.root(), &fake));
        assert_eq!(
            found,
            [Outcome::Refused("vcruntime140.dll".to_owned(), refused)]
        );
        assert_eq!(layout.names("codex-path"), ["rg.exe"]);
    }
    // The same toolset as rg.exe's is new enough.
    let layout = Layout::usual();
    let fake = Fake::new(Signer::Microsoft, Some([14, 44, 35211, 0]));
    assert_eq!(
        outcomes(place_with(WIN_ARM64, layout.root(), &fake)),
        [Outcome::Placed(
            "vcruntime140.dll".to_owned(),
            [14, 44, 35211, 0]
        )]
    );
}

#[test]
fn a_dll_already_beside_rg_is_left_alone() {
    let layout = Layout::usual();
    let existing = layout.dir.file(
        &format!("{VENDOR}/codex-path/VCRUNTIME140.DLL"),
        b"someone else's copy",
    );
    let fake = Fake::good();
    let found = outcomes(place_with(WIN_ARM64, layout.root(), &fake));
    assert_eq!(
        found,
        [Outcome::AlreadyBeside("vcruntime140.dll".to_owned())]
    );
    assert_eq!(std::fs::read(existing).unwrap(), b"someone else's copy");
    assert_eq!(layout.names("codex-path"), ["VCRUNTIME140.DLL", "rg.exe"]);
    assert!(fake.asked.borrow().is_empty());
}

#[test]
fn an_rg_that_imports_no_runtime_gets_nothing() {
    let layout = Layout::new(
        "codex-path",
        &[
            "kernel32.dll",
            "api-ms-win-crt-runtime-l1-1-0.dll",
            "msvcp_win.dll",
        ],
        &[("vcruntime140.dll", MACHINE_ARM64)],
    );
    let fake = Fake::good();
    let found = outcomes(place_with(WIN_ARM64, layout.root(), &fake));
    assert_eq!(found, [Outcome::NotNeeded]);
    assert_eq!(layout.names("codex-path"), ["rg.exe"]);
    assert!(fake.asked.borrow().is_empty());
}

#[test]
fn other_hosts_are_untouched() {
    for platform in [
        Platform::new(Os::Win32, Arch::X64),
        Platform::new(Os::Darwin, Arch::Arm64),
        Platform::new(Os::Darwin, Arch::X64),
        Platform::new(Os::Linux, Arch::Arm64),
    ] {
        let layout = Layout::usual();
        let fake = Fake::good();
        assert_eq!(place_with(platform, layout.root(), &fake), Vec::new());
        assert_eq!(layout.names("codex-path"), ["rg.exe"]);
        assert!(fake.asked.borrow().is_empty());
    }
}

#[test]
fn an_install_without_codex_is_untouched() {
    let dir = TestDir::new("vcruntime-none");
    dir.file("node_modules/other/index.js", b"module.exports = 1;\n");
    assert_eq!(place_with(WIN_ARM64, &dir.0, &Fake::good()), Vec::new());
}

/// Smart App Control's step (PR #812) renames `codex-path` to `codex-path.sac-off`, the whole
/// folder: the DLL moves with rg.exe, and when it runs first, the DLL is placed in the new name.
#[test]
fn rg_moved_off_path_by_smart_app_control_still_gets_the_dll() {
    let layout = Layout::new(
        "codex-path.sac-off",
        &RG_IMPORTS,
        &[("vcruntime140.dll", MACHINE_ARM64)],
    );
    let found = place_with(WIN_ARM64, layout.root(), &Fake::good());
    assert_eq!(
        found,
        [(
            layout.rg_dir("codex-path.sac-off"),
            Outcome::Placed("vcruntime140.dll".to_owned(), SHIPPED)
        )]
    );
    assert!(!layout.rg_dir("codex-path").exists());
    assert_eq!(
        missing_beside(&layout.rg_dir("codex-path.sac-off")).unwrap(),
        Vec::<String>::new()
    );

    // Placed first, then moved aside as `sac::settle` does: still beside rg.exe.
    let layout = Layout::usual();
    place_with(WIN_ARM64, layout.root(), &Fake::good());
    crate::write::rename(
        &layout.rg_dir("codex-path"),
        &layout.rg_dir("codex-path.sac-off"),
    )
    .unwrap();
    assert_eq!(
        missing_beside(&layout.rg_dir("codex-path.sac-off")).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn a_missing_second_dll_is_refused_alone() {
    let layout = Layout::new(
        "codex-path",
        &["VCRUNTIME140.dll", "VCRUNTIME140_1.dll", "MSVCP140.dll"],
        &[("vcruntime140.dll", MACHINE_ARM64)],
    );
    let found = outcomes(place_with(WIN_ARM64, layout.root(), &Fake::good()));
    assert_eq!(
        found,
        [
            Outcome::Unsupported("msvcp140.dll".to_owned()),
            Outcome::Placed("vcruntime140.dll".to_owned(), SHIPPED),
            Outcome::Refused("vcruntime140_1.dll".to_owned(), Refused::Missing),
        ]
    );
    assert!(found[0].warns() && !found[1].warns() && found[2].warns());
}

#[test]
fn an_unreadable_rg_is_reported() {
    let layout = Layout::usual();
    layout
        .dir
        .file(&format!("{VENDOR}/codex-path/rg.exe"), b"not a program");
    let found = outcomes(place_with(WIN_ARM64, layout.root(), &Fake::good()));
    assert!(matches!(found.as_slice(), [Outcome::RgUnreadable(_)]));
    assert_eq!(layout.names("codex-path"), ["rg.exe"]);
}

#[test]
fn check_runs_in_order() {
    let arm64 = pe::parse(&pe_file(MACHINE_ARM64, (14, 50), &[], &[])).unwrap();
    let x64 = pe::parse(&pe_file(X64, (14, 50), &[], &[])).unwrap();
    let untrusted = Signer::NotTrusted("no".to_owned());
    assert_eq!(
        check(&x64, &untrusted, None, RG_LINKER),
        Err(Refused::WrongArch(X64))
    );
    assert_eq!(
        check(&arm64, &untrusted, Some(SHIPPED), RG_LINKER),
        Err(Refused::NotTrusted("no".to_owned()))
    );
    assert_eq!(
        check(&arm64, &Signer::Microsoft, Some(SHIPPED), (15, 0)),
        Err(Refused::UnknownToolset((15, 0)))
    );
    assert_eq!(
        check(&arm64, &Signer::Microsoft, Some(SHIPPED), RG_LINKER),
        Ok(SHIPPED)
    );
}

#[test]
fn forbidden_imports_follow_stage_f_rules() {
    for dll in [
        "VCRUNTIME140.dll",
        "vcruntime140_1.dll",
        "vcruntime140d.dll",
        "MSVCP140.dll",
        "msvcp140_atomic_wait.dll",
        "msvcr120.dll",
        "vccorlib140.dll",
        "concrt140.dll",
        "vcomp140.dll",
        "vcamp140.dll",
        "mfc140u.dll",
        "mfcm140.dll",
        "ucrtbased.dll",
    ] {
        assert!(is_forbidden(dll), "{dll}");
    }
    for dll in [
        "api-ms-win-crt-runtime-l1-1-0.dll",
        "ucrtbase.dll",
        "msvcrt.dll",
        "msvcp_win.dll",
        "mfcore.dll",
        "kernel32.dll",
        "vcruntime.dll",
    ] {
        assert!(!is_forbidden(dll), "{dll}");
    }
}

#[test]
fn log_lines_name_the_dll_and_the_reason() {
    use super::describe;
    let line = describe(&Outcome::Refused(
        "vcruntime140.dll".to_owned(),
        Refused::TooOld([14, 42, 34438, 0], (14, 44)),
    ));
    assert!(
        line.contains("vcruntime140.dll")
            && line.contains("14.42.34438.0")
            && line.contains("14.44")
    );
    let line = describe(&Outcome::Placed("vcruntime140.dll".to_owned(), SHIPPED));
    assert!(line.contains("14.50.35719.0"));
}
