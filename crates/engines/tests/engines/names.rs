//! Official names only: the two engines, the codex override, a v3 lock from the registry.

use clauseo_engines::Error;

use crate::fixtures::{
    ABSOLUTE_KEY, BACKSLASH_KEY, DEP_BUILD, DEP_GIT, DEP_NPM_ALIAS, DOTDOT_KEY, EXTRA_DEP,
    EXTRA_OVERRIDE, IN_BUNDLE, INSTALLED_MISMATCH, LINK, LOCK_V2, MISSING_DEP, MISSING_INTEGRITY,
    MISSING_RESOLVED, NON_NPM_RESOLVED, OTHER_RESOLVED, OVERRIDE_ALIAS, OVERRIDE_DROPPED,
    OVERRIDE_GIT, OVERRIDE_OBJECT, OVERRIDE_REF, PACKAGE_JSON_ARRAY, ROOT_MISMATCH, SCRIPTS,
    SHA1_INTEGRITY, Signed, TWO_INTEGRITY, WORKSPACES, refusal,
};

fn each(cases: &[(&str, &Signed)], expected: &Error) {
    for (name, signed) in cases {
        assert_eq!(refusal(signed).as_ref(), Some(expected), "{name}");
    }
}

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

#[test]
fn a_lock_key_outside_node_modules_is_refused() {
    each(
        &[
            ("../", &DOTDOT_KEY),
            ("absolute", &ABSOLUTE_KEY),
            ("backslash", &BACKSLASH_KEY),
        ],
        &Error::LockPath,
    );
}

#[test]
fn a_link_or_bundled_entry_is_refused() {
    each(
        &[("link", &LINK), ("inBundle", &IN_BUNDLE)],
        &Error::LockEntry,
    );
}

#[test]
fn a_missing_or_foreign_resolved_is_refused() {
    each(
        &[
            ("missing", &MISSING_RESOLVED),
            ("another package", &OTHER_RESOLVED),
            ("another host", &NON_NPM_RESOLVED),
        ],
        &Error::Resolved,
    );
}

#[test]
fn an_integrity_of_more_than_one_token_is_refused() {
    assert_eq!(refusal(&TWO_INTEGRITY), Some(Error::Integrity));
}

#[test]
fn an_override_that_is_not_one_version_is_refused() {
    each(
        &[
            ("npm: alias", &OVERRIDE_ALIAS),
            ("git", &OVERRIDE_GIT),
            ("$ reference", &OVERRIDE_REF),
        ],
        &Error::Version,
    );
}

#[test]
fn a_lock_that_disagrees_with_the_pins_is_refused() {
    each(
        &[
            ("root dependencies", &ROOT_MISMATCH),
            ("installed version", &INSTALLED_MISMATCH),
        ],
        &Error::LockMismatch,
    );
}

#[test]
fn a_package_json_that_is_not_a_pin_file_is_refused() {
    each(
        &[
            ("an array", &PACKAGE_JSON_ARRAY),
            ("workspaces", &WORKSPACES),
        ],
        &Error::PackageJsonField,
    );
}

#[test]
fn an_override_that_is_not_a_string_or_is_dropped_is_refused() {
    each(
        &[
            ("an object", &OVERRIDE_OBJECT),
            ("dropped while the bundle pins it", &OVERRIDE_DROPPED),
        ],
        &Error::Overrides,
    );
}

#[test]
fn a_dependency_that_is_not_one_plain_version_is_refused() {
    each(
        &[
            ("npm: alias", &DEP_NPM_ALIAS),
            ("git+", &DEP_GIT),
            ("build metadata", &DEP_BUILD),
        ],
        &Error::Version,
    );
}
