//! The trust roots file from a fake `security` (`tls_roots.rs`).

use std::path::PathBuf;

use super::{ROOTS_FILE, SYSTEM_KEYCHAIN, SYSTEM_ROOTS, write_roots};
use crate::write;

const ROOT: &str = "-----BEGIN CERTIFICATE-----\nR46\n-----END CERTIFICATE-----\n";
const FIRM: &str = "-----BEGIN CERTIFICATE-----\nFIRM\n-----END CERTIFICATE-----\n";

/// A fresh data folder under the temp folder for `test`.
fn data(test: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("clauseo-tls-{test}-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    write::create_dir_all(&root).unwrap();
    root
}

/// The System Roots, then the admin's roots, in `<data>/tls/roots.pem`, rewritten each launch.
#[test]
fn the_roots_file_holds_the_system_roots_then_the_admins() {
    let data = data("both");
    let export = |keychain: &str| match keychain {
        SYSTEM_ROOTS => Some(ROOT.to_owned()),
        SYSTEM_KEYCHAIN => Some(FIRM.to_owned()),
        other => panic!("{other}"),
    };
    let file = write_roots(&data, &export).unwrap();
    assert_eq!(file, data.join(ROOTS_FILE[0]).join(ROOTS_FILE[1]));
    assert!(file.ends_with("tls/roots.pem"));
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        format!("{ROOT}{FIRM}")
    );
    // The next launch rewrites it; a System keychain with no certificate (`security` exits 44)
    // leaves the System Roots alone.
    let roots_only = |keychain: &str| (keychain == SYSTEM_ROOTS).then(|| ROOT.to_owned());
    assert_eq!(write_roots(&data, &roots_only), Some(file.clone()));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), ROOT);
    write::remove_dir_all(&data).unwrap();
}

/// No System Roots (`security` failed, timed out or printed no certificate): no file, so no
/// variable is set and curl keeps its own bundle.
#[test]
fn no_system_roots_gives_no_file() {
    let data = data("none");
    for export in [
        &(|_: &str| None) as &dyn Fn(&str) -> Option<String>,
        &|_: &str| Some(String::new()),
        &|keychain: &str| (keychain == SYSTEM_KEYCHAIN).then(|| FIRM.to_owned()),
    ] {
        assert_eq!(write_roots(&data, export), None);
    }
    assert!(!data.join(ROOTS_FILE[0]).exists());
    write::remove_dir_all(&data).unwrap();
}

/// The real `security` off macOS is not there: none, never a panic.
#[cfg(not(target_os = "macos"))]
#[test]
fn a_missing_security_tool_exports_nothing() {
    let clock: std::sync::Arc<dyn crate::clock::Clock> =
        std::sync::Arc::new(crate::clock::SystemClock);
    assert_eq!(super::security(clock)(SYSTEM_ROOTS), None);
}
