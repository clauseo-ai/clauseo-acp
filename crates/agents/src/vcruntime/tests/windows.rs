//! The real `WinVerifyTrust` and version calls, against files every Windows CI runner has. A
//! missing file skips its test on a developer's machine, and fails it on CI (`CI` set), where
//! a skip would hide a check that never ran.

use std::path::{Path, PathBuf};

use super::{Layout, RG_IMPORTS, WIN_ARM64, X64, outcomes, pe_file};
use crate::vcruntime::pe::MACHINE_ARM64;
use crate::vcruntime::windows::{signer, version_info};
use crate::vcruntime::{Outcome, Refused, Signer, System, VENDOR, place_with};
use crate::write::TestDir;

/// `%SystemRoot%\System32\<name>`.
fn system32(name: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    PathBuf::from(root).join("System32").join(name)
}

/// Whether `path` is there to test with: a missing one fails on CI and skips elsewhere.
fn present(path: &Path) -> bool {
    if path.is_file() {
        return true;
    }
    let on_ci = std::env::var_os("CI").is_some_and(|value| !value.is_empty());
    assert!(!on_ci, "{} is missing on CI", path.display());
    eprintln!("skipped: no {}", path.display());
    false
}

/// The step with the runner's own `source` as the package's `vcruntime140.dll`.
fn step_with(source: &Path) -> (Layout, Vec<Outcome>) {
    let layout = Layout::new("codex-path", &RG_IMPORTS, &[]);
    layout.dir.file(
        &format!("{VENDOR}/codex-resources/voice/bin/vcruntime140.dll"),
        &std::fs::read(source).unwrap(),
    );
    let found = outcomes(place_with(WIN_ARM64, layout.root(), &System));
    (layout, found)
}

/// The VC++ runtime the Redistributable installs is embedded-signed by Microsoft (Windows'
/// own DLLs are mostly catalog-signed, which this check does not accept). The CI images have
/// the Redistributable.
#[test]
fn a_microsoft_signed_runtime_is_microsoft() {
    let dll = system32("vcruntime140.dll");
    if !present(&dll) {
        return;
    }
    assert_eq!(signer(&dll), Signer::Microsoft);
    let info = version_info(&dll).unwrap();
    assert_eq!(info.version[0], 14, "{info:?}");
    let original = info.original_filename.unwrap();
    assert!(
        original.eq_ignore_ascii_case("vcruntime140.dll"),
        "{original}"
    );
}

#[test]
fn an_unsigned_file_is_not_trusted() {
    let dir = TestDir::new("vcruntime-unsigned");
    let path = dir.file("unsigned.dll", &pe_file(MACHINE_ARM64, (14, 50), &[], &[]));
    assert!(matches!(signer(&path), Signer::NotTrusted(_)));
    assert_eq!(version_info(&path), None);
    assert!(matches!(
        signer(&dir.0.join("missing.dll")),
        Signer::NotTrusted(_)
    ));
}

/// Windows' own DLLs are catalog-signed, with no embedded signature: never `Microsoft` here,
/// and refused as a source.
#[test]
fn a_catalog_signed_system_dll_is_refused() {
    let dll = system32("kernel32.dll");
    if !present(&dll) {
        return;
    }
    assert_ne!(signer(&dll), Signer::Microsoft);
    let (layout, found) = step_with(&dll);
    assert!(
        matches!(found.as_slice(), [Outcome::Refused(..)]),
        "{found:?}"
    );
    assert_eq!(layout.names("codex-path"), ["rg.exe"]);
}

/// Another Microsoft-signed runtime DLL renamed: its `OriginalFilename` gives it away (on an
/// x64 runner its arch is refused first).
#[test]
fn a_renamed_microsoft_dll_is_refused() {
    let dll = system32("msvcp140.dll");
    if !present(&dll) {
        return;
    }
    let original = version_info(&dll).unwrap().original_filename.unwrap();
    assert!(original.eq_ignore_ascii_case("msvcp140.dll"), "{original}");
    let (layout, found) = step_with(&dll);
    let expected = if cfg!(target_arch = "x86_64") {
        Refused::WrongArch(X64)
    } else {
        Refused::WrongName(Some(original))
    };
    assert_eq!(
        found,
        [Outcome::Refused("vcruntime140.dll".to_owned(), expected)]
    );
    assert_eq!(layout.names("codex-path"), ["rg.exe"]);
}

/// A tampered copy of a Microsoft-signed DLL: its signature no longer matches.
#[test]
fn a_tampered_microsoft_dll_is_not_trusted() {
    let dll = system32("vcruntime140.dll");
    if !present(&dll) {
        return;
    }
    let mut bytes = std::fs::read(&dll).unwrap();
    // A byte in the code, past the headers: the Authenticode hash covers it.
    bytes[0x1000] ^= 0xFF;
    let dir = TestDir::new("vcruntime-tampered");
    let path = dir.file("vcruntime140.dll", &bytes);
    assert!(matches!(signer(&path), Signer::NotTrusted(_)));
}

/// Git for Windows' launcher is signed, by its maintainer: valid, but not Microsoft.
#[test]
fn a_file_signed_by_someone_else_is_other() {
    let git = PathBuf::from(r"C:\Program Files\Git\cmd\git.exe");
    if !present(&git) {
        return;
    }
    assert!(
        matches!(signer(&git), Signer::Other(_)),
        "{:?}",
        signer(&git)
    );
}

/// The whole step with the real checks, the runner's own runtime as the package's copy: an
/// x64 runner's is refused for its arch; an ARM64 runner's is placed when it is new enough.
#[test]
fn the_step_with_windows_checks() {
    let dll = system32("vcruntime140.dll");
    if !present(&dll) {
        return;
    }
    let (layout, found) = step_with(&dll);
    let dll_name = "vcruntime140.dll".to_owned();
    if cfg!(target_arch = "x86_64") {
        assert_eq!(found, [Outcome::Refused(dll_name, Refused::WrongArch(X64))]);
        assert_eq!(layout.names("codex-path"), ["rg.exe"]);
    } else {
        assert!(
            matches!(
                found.as_slice(),
                [Outcome::Placed(..) | Outcome::Refused(_, Refused::TooOld(..))]
            ),
            "{found:?}"
        );
    }
}
