//! The lock inside lock entries: specs, aliases, pins at every depth, look-alike keys.

use clauseo_engines::Error;

use crate::fixtures::{
    ALIAS_AT_ENGINE, BAD_NAME, DOT_KEY, INSTALL_SCRIPT, NESTED_CODEX, OVERRIDE_OFF_PIN,
    SHORT_INTEGRITY, SPEC_FILE, SPEC_GIT, SPEC_GITHUB, SPEC_LINK, SPEC_TAG, SPEC_URL,
    SPEC_USER_REPO, SPEC_WORKSPACE, Signed, UPPER_KEY, refusal,
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
