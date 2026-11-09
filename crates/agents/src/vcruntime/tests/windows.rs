//! The real `WinVerifyTrust` and version calls, against files every Windows CI runner has.

use std::path::PathBuf;

use super::{Layout, RG_IMPORTS, WIN_ARM64, X64, outcomes, pe_file};
use crate::vcruntime::pe::MACHINE_ARM64;
use crate::vcruntime::windows::{file_version, signer};
use crate::vcruntime::{Outcome, Refused, Signer, System, VENDOR, place_with};
use crate::write::TestDir;

/// `%SystemRoot%\System32\<name>`.
fn system32(name: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    PathBuf::from(root).join("System32").join(name)
}

/// The VC++ runtime the Redistributable installs is embedded-signed by Microsoft (Windows'
/// own DLLs are mostly catalog-signed, which this check does not accept). The CI images
/// have the Redistributable; a machine without it skips.
#[test]
fn a_microsoft_signed_runtime_is_microsoft() {
    let dll = system32("vcruntime140.dll");
    if !dll.is_file() {
        eprintln!("skipped: no {}", dll.display());
        return;
    }
    assert_eq!(signer(&dll), Signer::Microsoft);
    let version = file_version(&dll).unwrap();
    assert_eq!(version[0], 14, "{version:?}");
}

#[test]
fn an_unsigned_file_is_not_trusted() {
    let dir = TestDir::new("vcruntime-unsigned");
    let path = dir.file("unsigned.dll", &pe_file(MACHINE_ARM64, (14, 50), &[], &[]));
    assert!(matches!(signer(&path), Signer::NotTrusted(_)));
    assert_eq!(file_version(&path), None);
    assert!(matches!(
        signer(&dir.0.join("missing.dll")),
        Signer::NotTrusted(_)
    ));
}

/// A tampered copy of a Microsoft-signed DLL: its signature no longer matches.
#[test]
fn a_tampered_microsoft_dll_is_not_trusted() {
    let dll = system32("vcruntime140.dll");
    if !dll.is_file() {
        eprintln!("skipped: no {}", dll.display());
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
    if !git.is_file() {
        eprintln!("skipped: no {}", git.display());
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
    if !dll.is_file() {
        eprintln!("skipped: no {}", dll.display());
        return;
    }
    let layout = Layout::new("codex-path", &RG_IMPORTS, &[]);
    layout.dir.file(
        &format!("{VENDOR}/codex-resources/voice/bin/vcruntime140.dll"),
        &std::fs::read(&dll).unwrap(),
    );
    let found = outcomes(place_with(WIN_ARM64, layout.root(), &System));
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
