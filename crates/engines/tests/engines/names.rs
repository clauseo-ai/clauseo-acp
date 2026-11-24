//! Official names only: the two engines, the codex override, a v3 lock from the registry.

use clauseo_engines::Error;

use crate::fixtures::{
    EXTRA_DEP, EXTRA_OVERRIDE, LOCK_V2, MISSING_DEP, MISSING_INTEGRITY, NON_NPM_RESOLVED, SCRIPTS,
    SHA1_INTEGRITY, refusal,
};

#[test]
fn an_extra_dependency_is_refused() {
    assert_eq!(refusal(&EXTRA_DEP), Some(Error::Dependencies));
}

#[test]
fn a_missing_dependency_is_refused() {
    assert_eq!(refusal(&MISSING_DEP), Some(Error::Dependencies));
}

#[test]
fn an_override_other_than_codex_is_refused() {
    assert_eq!(refusal(&EXTRA_OVERRIDE), Some(Error::Overrides));
}

#[test]
fn install_scripts_in_package_json_are_refused() {
    assert_eq!(refusal(&SCRIPTS), Some(Error::PackageJsonField));
}

#[test]
fn a_lock_that_is_not_version_3_is_refused() {
    assert_eq!(refusal(&LOCK_V2), Some(Error::Lockfile));
}

#[test]
fn a_package_resolved_outside_the_npm_registry_is_refused() {
    assert_eq!(refusal(&NON_NPM_RESOLVED), Some(Error::Resolved));
}

#[test]
fn a_package_without_integrity_is_refused() {
    assert_eq!(refusal(&MISSING_INTEGRITY), Some(Error::Integrity));
}

#[test]
fn a_package_with_a_weaker_integrity_is_refused() {
    assert_eq!(refusal(&SHA1_INTEGRITY), Some(Error::Integrity));
}
