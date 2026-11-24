//! The lock inside lock entries: specs, aliases, pins at every depth, look-alike keys.

use clauseo_engines::{Error, load_with_key};

use crate::fixtures::{
    ALIAS_AT_ENGINE, ALIAS_OK, ALIAS_WRONG_PATH, ALIAS_WRONG_VERSION, BAD_NAME, DOT_KEY,
    INSTALL_SCRIPT, KEY_A, NESTED_CODEX, OVERRIDE_DROPPED, OVERRIDE_OFF_PIN, SHORT_INTEGRITY,
    SPEC_ALIAS_GIT, SPEC_ALIAS_NAME, SPEC_ALIAS_OK, SPEC_BETA1, SPEC_DOT, SPEC_FILE, SPEC_GIT,
    SPEC_GITHUB, SPEC_LINK, SPEC_TAG, SPEC_TAR, SPEC_TGZ, SPEC_URL, SPEC_USER_REPO, SPEC_WORKSPACE,
    Signed, UNKNOWN_ENTRY_FIELD, UNNAMED_PLATFORM, UPPER_KEY, bundled, check, context, refusal,
};

fn each(cases: &[(&str, &Signed)], expected: &Error) {
    for (name, signed) in cases {
        assert_eq!(refusal(signed).as_ref(), Some(expected), "{name}");
    }
}

#[test]
fn a_spec_outside_the_registry_inside_an_entry_is_refused() {
    each(
        &[
            ("git+", &SPEC_GIT),
            ("github: (optionalDependencies)", &SPEC_GITHUB),
            ("user/repo", &SPEC_USER_REPO),
            ("https:", &SPEC_URL),
            ("file: (peerDependencies)", &SPEC_FILE),
            ("link:", &SPEC_LINK),
            ("workspace:", &SPEC_WORKSPACE),
            ("a bare word", &SPEC_TAG),
        ],
        &Error::LockSpec,
    );
}

#[test]
fn an_alias_or_a_bad_name_is_refused() {
    each(
        &[
            ("an alias at an engine's path", &ALIAS_AT_ENGINE),
            ("../", &BAD_NAME),
        ],
        &Error::LockEntry,
    );
}

#[test]
fn an_install_script_is_refused() {
    assert_eq!(refusal(&INSTALL_SCRIPT), Some(Error::LockEntry));
}

#[test]
fn a_pinned_package_off_its_pin_at_any_depth_is_refused() {
    each(
        &[
            ("an older codex nested under codex-acp", &NESTED_CODEX),
            (
                "the override at another version than the lock's",
                &OVERRIDE_OFF_PIN,
            ),
        ],
        &Error::LockMismatch,
    );
}

#[test]
fn a_look_alike_key_is_refused() {
    each(
        &[("upper case", &UPPER_KEY), ("trailing dot", &DOT_KEY)],
        &Error::LockPath,
    );
}

#[test]
fn an_integrity_of_the_wrong_length_is_refused() {
    assert_eq!(refusal(&SHORT_INTEGRITY), Some(Error::Integrity));
}

#[test]
fn npm_alias_specs_inside_entries_pass_only_with_a_valid_name_and_range() {
    assert!(check(&SPEC_ALIAS_OK).is_ok());
    each(
        &[
            ("git range", &SPEC_ALIAS_GIT),
            ("upper-case name", &SPEC_ALIAS_NAME),
        ],
        &Error::LockSpec,
    );
}

#[test]
fn a_spec_that_looks_like_a_path_or_a_tarball_is_refused_and_a_tag_with_a_digit_is_not() {
    each(
        &[
            (".1", &SPEC_DOT),
            ("1.tgz", &SPEC_TGZ),
            ("1.TAR", &SPEC_TAR),
        ],
        &Error::LockSpec,
    );
    assert!(check(&SPEC_BETA1).is_ok());
}

#[test]
fn a_codex_platform_path_must_be_the_pinned_alias() {
    assert!(check(&ALIAS_OK).is_ok());
    // No name: another package where codex's platform build goes.
    assert_eq!(refusal(&UNNAMED_PLATFORM), Some(Error::LockEntry));
    // The alias at another platform's version, and under a platform codex doesn't publish.
    assert_eq!(refusal(&ALIAS_WRONG_VERSION), Some(Error::LockMismatch));
    assert_eq!(refusal(&ALIAS_WRONG_PATH), Some(Error::LockEntry));
}

#[test]
fn codex_in_the_lock_with_no_override_is_refused() {
    // A bundle with no override, so the list may drop it; its lock still has codex, unpinned.
    let mut bundle = bundled();
    bundle.as_object_mut().unwrap().remove("overrides");
    assert_eq!(
        load_with_key(
            OVERRIDE_DROPPED.json,
            OVERRIDE_DROPPED.sig,
            &context(&bundle),
            KEY_A
        )
        .map(|_| ()),
        Err(Error::LockMismatch)
    );
}

#[test]
fn an_entry_field_outside_the_allowlist_is_refused() {
    assert_eq!(refusal(&UNKNOWN_ENTRY_FIELD), Some(Error::LockEntry));
}
