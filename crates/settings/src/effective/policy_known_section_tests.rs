//! The ways a firm's policy failed to lock in the orchestrator's review of #450, each its
//! repro: a security section placed inside a known section, which a known first segment hid,
//! and a non-ASCII key, which locked every security key at any depth. Each must fail closed
//! without locking more than it must.

use super::Effective;
use super::policy_gaps_tests::{assert_skills_misnamed, managed, registry_of, users_own};
use crate::lock_reason::LockCause;
use crate::policy::managed_prefs::PlistValue;
use crate::policy::registry::RegValue;
use crate::testing::{locations, policy};

// The orchestrator's review of #450: a name whose first segment was a known section was only
// that section's, so a security section misplaced inside `ui`, `matters` or `privacy` locked
// nothing, where main locked it.
#[test]
fn a_security_section_inside_a_known_section_locks() {
    let effective = Effective::load(
        &locations(),
        &users_own(),
        &[&policy("firm", r#"{"ui": {"tools": {"browser": true}}}"#)],
    );
    let settings = effective.settings();
    assert!(!settings.tools.web_search && !settings.tools.browser);
    assert!(effective.is_locked("tools.web_search") && effective.is_locked("tools.browser"));
    assert_eq!(
        effective.lock_reason("tools.browser").unwrap().cause,
        LockCause::MisnamedSection
    );
    assert!(!effective.is_locked("skills.extra") && !effective.is_locked("mcp.servers"));
    let firm = registry_of(vec![("ui.tools.browser", RegValue::Dword(1))]);
    let effective = Effective::load(&locations(), &users_own(), &[&firm]);
    assert!(!effective.settings().tools.browser && effective.is_locked("tools.browser"));
    let firm = managed(vec![(
        "ui",
        PlistValue::Dict(vec![("tools".to_owned(), PlistValue::Bool(true))]),
    )]);
    let effective = Effective::load(&locations(), &users_own(), &[&firm]);
    assert!(effective.is_locked("tools.web_search") && effective.is_locked("tools.browser"));
    assert!(!effective.settings().tools.browser);
    let effective = Effective::load(
        &locations(),
        &users_own(),
        &[&policy("firm", r#"{"matters": {"install_bundle": "x"}}"#)],
    );
    assert_eq!(effective.settings().install.bundle, None);
    assert!(effective.is_locked("install.bundle"));
    let effective = Effective::load(
        &locations(),
        &users_own(),
        &[&policy("firm", r#"{"privacy": {"mcp": 1}}"#)],
    );
    assert!(effective.is_locked("mcp.servers"));
    assert!(effective.settings().mcp.servers.is_empty());
    // A known section's real keys lock only themselves.
    let effective = Effective::load(
        &locations(),
        &users_own(),
        &[&policy(
            "firm",
            r#"{"ui": {"theme": "dark"}, "privacy": {"counts": false}}"#,
        )],
    );
    let mut locked: Vec<&str> = effective.locked().collect();
    locked.sort_unstable();
    assert_eq!(locked, ["privacy.counts", "ui.theme"]);
}

// The orchestrator's review of #450: a non-ASCII key at any depth locked every security key.
// Deep, it locks nothing; at the top level, or as a section name under `ui`, it still does.
#[test]
fn a_non_ascii_key_locks_only_at_a_section_place() {
    for text in [
        r#"{"_comment": {"note": {"café": 1}}}"#,
        r#"{"_comment": {"note": {"टिप्पणी": "x"}}}"#,
    ] {
        let effective = Effective::load(&locations(), &users_own(), &[&policy("firm", text)]);
        assert_eq!(effective.locked().count(), 0, "{text}");
        assert!(effective.settings().tools.web_search, "{text}");
        assert!(!effective.warnings().is_empty(), "{text}");
    }
    let effective = Effective::load(
        &locations(),
        &users_own(),
        &[&policy("firm", r#"{"टिप्पणी": "x"}"#)],
    );
    for key in crate::leaf::SECURITY_KEYS {
        assert!(effective.is_locked(key), "{key}");
    }
    assert_eq!(
        effective.lock_reason("tools.browser").unwrap().cause,
        LockCause::MisnamedSection
    );
    for text in [
        "{\"\u{455}k\u{456}lls\": {\"extra\": \"any\"}}",
        "{\"ui\": {\"\u{455}k\u{456}lls\": {\"extra\": \"any\"}}}",
    ] {
        let effective = Effective::load(&locations(), &users_own(), &[&policy("firm", text)]);
        assert_skills_misnamed(&effective, text);
    }
}
