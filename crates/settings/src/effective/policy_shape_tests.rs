//! A policy whose shape this version cannot read fails closed: a misspelt section locks that
//! section's security keys off, and a `policy.json` that holds a key twice is dropped whole, so
//! every security key locks off. Both locks say why (`Effective::lock_reasons`), naming the
//! source by kind and never by path. The lawyer's own file only warns.

use super::Effective;
use crate::lock_reason::{LockCause, LockReason, SourceKind};
use crate::policy::PolicyRead;
use crate::policy::registry::{self, RegValue};
use crate::schema::SkillsExtra;
use crate::testing::{FakeSource, abs_json, locations, policy, user};
use crate::warning::{Warning, WarningKind};

/// `settings.json` with the lawyer's skills, tools and install keys on or set.
fn users_own() -> crate::testing::MemFiles {
    user(&format!(
        r#"{{"skills": {{"extra": "any", "allow_scripts": true}},
            "tools": {{"web_search": true, "browser": true}},
            "install": {{"bundle": "{}"}}}}"#,
        abs_json("bundle")
    ))
}

#[test]
fn a_misspelt_skills_section_locks_its_security_keys_off() {
    for section in ["skill", "skils", "Skills", "SKILL"] {
        let text = format!(r#"{{"{section}": {{"extra": "off", "allow_scripts": false}}}}"#);
        let effective = Effective::load(&locations(), &users_own(), &[&policy("firm", text)]);
        let skills = &effective.settings().skills;
        assert_eq!(skills.extra, SkillsExtra::Off, "{section}");
        assert!(!skills.allow_scripts, "{section}");
        for key in ["skills.extra", "skills.allow_scripts", "skills.curated"] {
            assert!(effective.is_locked(key), "{section}: {key}");
        }
        // Other sections stay the lawyer's.
        assert!(effective.settings().tools.web_search && !effective.is_locked("tools.browser"));
    }
}

#[test]
fn a_misspelt_tools_or_install_section_locks_its_security_keys_off() {
    let tool = policy("firm", r#"{"tool": {"web_search": true, "browser": true}}"#);
    let effective = Effective::load(&locations(), &users_own(), &[&tool]);
    assert!(!effective.settings().tools.web_search && !effective.settings().tools.browser);
    assert!(effective.is_locked("tools.web_search") && effective.is_locked("tools.browser"));
    assert!(!effective.is_locked("skills.extra"));

    let instal = policy(
        "firm",
        r#"{"instal": {"registry": "https://npm.firm.example"}}"#,
    );
    let effective = Effective::load(&locations(), &users_own(), &[&instal]);
    assert_eq!(effective.settings().install.bundle, None);
    assert!(effective.is_locked("install.bundle") && effective.is_locked("install.registry"));
    assert!(effective.settings().tools.web_search);
}

// A name far from every security section, or a known section's own key, locks nothing.
#[test]
fn a_name_outside_the_security_sections_only_warns() {
    for text in [
        r#"{"colour": {"theme": "x"}}"#,
        r#"{"colour": {"extra": "off"}}"#,
        r#"{"ui": {"colour": "off"}}"#,
    ] {
        let effective = Effective::load(&locations(), &users_own(), &[&policy("firm", text)]);
        assert_eq!(effective.locked().count(), 0, "{text}");
        assert!(effective.settings().tools.web_search, "{text}");
        assert_eq!(
            effective.settings().skills.extra,
            SkillsExtra::Any,
            "{text}"
        );
        assert!(!effective.warnings().is_empty(), "{text}");
    }
    // A registry value far from every security section only warns too.
    let values = registry::parse(vec![("foo.colour_note".to_owned(), RegValue::Dword(1))]);
    let source = FakeSource {
        name: registry::SOURCE,
        read: move || PolicyRead::Read(values.clone()),
    };
    let effective = Effective::load(&locations(), &users_own(), &[&source]);
    assert_eq!(effective.locked().count(), 0);
}

// A known section is matched exactly first, so a real section close to a security one (`ui`,
// two edits from `mcp`'s length, `plans` and `agents`) is read as itself and locks nothing
// beyond its own keys.
#[test]
fn a_known_section_near_a_security_one_is_read_normally() {
    let firm = policy(
        "firm",
        r#"{"ui": {"theme": "dark"}, "agents": {}, "matters": {}}"#,
    );
    let effective = Effective::load(&locations(), &users_own(), &[&firm]);
    assert_eq!(effective.locked().collect::<Vec<_>>(), ["ui.theme"]);
    assert_eq!(effective.warnings(), []);
    assert!(effective.settings().tools.web_search);
}

#[test]
fn a_misspelt_section_in_the_users_own_file_only_warns() {
    let files = user(r#"{"skill": {"extra": "any"}, "skills": {"allow_scripts": true}}"#);
    let effective = Effective::load(&locations(), &files, &[]);
    assert_eq!(effective.locked().count(), 0);
    assert!(effective.settings().skills.allow_scripts);
    assert!(matches!(
        effective.warnings(),
        [Warning { kind: WarningKind::UnknownKey { key }, .. }] if key == "skill"
    ));
}

#[test]
fn a_misspelt_section_lock_says_why_by_source_kind_and_plain_name() {
    let firm = policy("/etc/clauseo/policy.json", r#"{"skill": {"extra": "off"}}"#);
    let effective = Effective::load(&locations(), &users_own(), &[&firm]);
    let reason = LockReason {
        keys: ["skills.extra", "skills.allow_scripts", "skills.curated"]
            .map(str::to_owned)
            .to_vec(),
        cause: LockCause::MisnamedSection,
        source: SourceKind::PolicyJson,
        section: Some("skill".to_owned()),
        place: 1,
    };
    // One reason for the name, not one per key.
    assert_eq!(effective.lock_reasons(), std::slice::from_ref(&reason));
    assert_eq!(effective.lock_reason("skills.curated"), Some(&reason));
    assert_eq!(effective.lock_reason("tools.browser"), None);
    // A name that is no plain word is given by its place, never echoed.
    let firm = policy(
        "firm",
        r#"{"ui": {}, "Skills=ghp_Secret": {"extra": "off"}}"#,
    );
    let effective = Effective::load(&locations(), &users_own(), &[&firm]);
    let reason = effective.lock_reason("skills.extra").unwrap();
    // Its place among the file's top-level entries.
    assert_eq!((reason.section.as_deref(), reason.place), (None, 2));
    assert_eq!(reason.cause, LockCause::MisnamedSection);
    assert!(!format!("{:?}", effective.lock_reasons()).contains("ghp_"));
    // The registry, by its kind.
    let values = registry::parse(vec![("tool.browser".to_owned(), RegValue::Dword(1))]);
    let source = FakeSource {
        name: registry::SOURCE,
        read: move || PolicyRead::Read(values.clone()),
    };
    let effective = Effective::load(&locations(), &users_own(), &[&source]);
    let reason = effective.lock_reason("tools.browser").unwrap();
    assert_eq!(reason.source, SourceKind::Registry);
    assert_eq!(reason.section.as_deref(), Some("tool"));
    assert_eq!(SourceKind::Registry.name(), "registry");
    assert_eq!(LockCause::MisnamedSection.code(), "misnamed_section");
}

#[test]
fn a_malformed_or_unreadable_policy_lock_has_its_cause_and_a_plain_set_has_none() {
    let firm = policy(
        "firm",
        r#"{"ui": {"theme": "dark"}, "skills": {"extra": "all"}}"#,
    );
    let effective = Effective::load(&locations(), &users_own(), &[&firm]);
    assert_eq!(effective.lock_reason("ui.theme"), None);
    assert_eq!(
        effective.lock_reasons(),
        [LockReason {
            keys: vec!["skills.extra".to_owned()],
            cause: LockCause::MalformedValue,
            source: SourceKind::PolicyJson,
            section: Some("skills".to_owned()),
            place: 2,
        }]
    );
    let effective = Effective::load(&locations(), &users_own(), &[&broken()]);
    match effective.lock_reasons() {
        [reason] => {
            assert_eq!(reason.cause, LockCause::SourceUnreadable);
            assert_eq!((reason.section.as_deref(), reason.place), (None, 0));
            assert_eq!(reason.keys.len(), crate::leaf::SECURITY_KEYS.len());
        }
        other => panic!("{other:?}"),
    }
}

/// A policy source that cannot be read.
fn broken() -> FakeSource<impl Fn() -> PolicyRead> {
    FakeSource {
        name: "broken",
        read: || {
            PolicyRead::Unreadable(crate::error::Error::Parse {
                path: crate::testing::abs("policy.json"),
                line: 1,
                column: 1,
                message: "expected value".to_owned(),
            })
        },
    }
}

#[test]
fn a_duplicate_section_in_a_policy_locks_every_security_key_off() {
    let firm = policy(
        "/etc/clauseo/policy.json",
        r#"{"skills": {"extra": "off"},
            "skills": {"enabled": {}}}"#,
    );
    let effective = Effective::load(&locations(), &users_own(), &[&firm]);
    assert_eq!(effective.settings().skills.extra, SkillsExtra::Off);
    assert!(effective.is_locked("skills.extra"));
    assert!(!effective.settings().tools.web_search && effective.is_locked("tools.web_search"));
    assert_eq!(effective.policy().errors().len(), 1);
    // One reason, at the second copy's line; never the key, nor the path.
    match effective.lock_reasons() {
        [reason] => {
            assert_eq!(reason.cause, LockCause::DuplicateKey);
            assert_eq!(reason.source, SourceKind::PolicyJson);
            assert_eq!((reason.section.as_deref(), reason.place), (None, 2));
            assert!(reason.keys.iter().any(|key| key == "skills.extra"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_nested_duplicate_in_a_policy_is_caught() {
    let firm = policy(
        "firm",
        r#"{"skills": {"enabled": {"nda-kit": "x", "nda-kit": "y"}}, "ui": {"theme": "dark"}}"#,
    );
    let effective = Effective::load(&locations(), &users_own(), &[&firm]);
    assert_eq!(effective.settings().skills.extra, SkillsExtra::Off);
    assert!(effective.is_locked("skills.extra"));
    // Dropped whole: its other values do not apply.
    assert!(!effective.is_locked("ui.theme"));
    assert!(effective.lock_reason("tools.browser").is_some());
}

#[test]
fn a_duplicate_in_the_users_own_file_warns_and_the_last_wins() {
    let files = user(r#"{"skills": {"extra": "off", "extra": "any"}}"#);
    let effective = Effective::load(&locations(), &files, &[]);
    assert_eq!(effective.settings().skills.extra, SkillsExtra::Any);
    assert_eq!(effective.locked().count(), 0);
    assert!(matches!(
        effective.warnings(),
        [Warning {
            kind: WarningKind::DuplicateKey,
            ..
        }]
    ));
    // Nested too.
    let files = user(r#"{"skills": {"enabled": {"a1": "x", "a1": "y"}}}"#);
    let effective = Effective::load(&locations(), &files, &[]);
    assert!(
        effective
            .warnings()
            .iter()
            .any(|warning| warning.kind == WarningKind::DuplicateKey)
    );
}

#[test]
fn a_prefixed_policy_name_still_locks_its_section() {
    for text in [
        r#"{"clauseo.skills.extra": "off"}"#,
        r#"{"x.skils.extra": "off"}"#,
        r#"{"clauseo.skills_extra": "off"}"#,
        r#"{"clauseo.skills-extra": "off"}"#,
    ] {
        let effective = Effective::load(&locations(), &users_own(), &[&policy("firm", text)]);
        assert_eq!(
            effective.settings().skills.extra,
            SkillsExtra::Off,
            "{text}"
        );
        assert!(effective.is_locked("skills.extra"), "{text}");
        let reason = effective.lock_reason("skills.extra").unwrap();
        assert_eq!(reason.cause, LockCause::MisnamedSection, "{text}");
        // Only the key the name could stand for.
        assert!(effective.settings().skills.allow_scripts, "{text}");
    }
    // The registry, whose value names compare without case.
    let values = registry::parse(vec![(
        "Clauseo.skills.allow_scripts".to_owned(),
        RegValue::Dword(0),
    )]);
    let source = FakeSource {
        name: registry::SOURCE,
        read: move || PolicyRead::Read(values.clone()),
    };
    let effective = Effective::load(&locations(), &users_own(), &[&source]);
    assert!(!effective.settings().skills.allow_scripts);
    assert!(effective.is_locked("skills.allow_scripts"));
    let reason = effective.lock_reason("skills.allow_scripts").unwrap();
    assert_eq!(
        (reason.cause, reason.source, reason.section.as_deref()),
        (LockCause::MisnamedSection, SourceKind::Registry, None)
    );
    let values = registry::parse(vec![(
        "Clauseo.skills_allow_scripts".to_owned(),
        RegValue::Dword(0),
    )]);
    let source = FakeSource {
        name: registry::SOURCE,
        read: move || PolicyRead::Read(values.clone()),
    };
    let effective = Effective::load(&locations(), &users_own(), &[&source]);
    assert!(!effective.settings().skills.allow_scripts);
    assert!(effective.is_locked("skills.allow_scripts"));
}

// A deliberate over-lock, failing closed: a prefixed name that holds a security section's name
// locks that section, though `tools_note` may mean nothing to Clauseo.
#[test]
fn a_prefixed_name_holding_a_section_name_locks_it_deliberately() {
    // A name nested in an unknown section counts as a later segment too.
    for text in [
        r#"{"foo.tools_note": "x"}"#,
        r#"{"foo": {"tools_note": "x"}}"#,
    ] {
        let effective = Effective::load(&locations(), &users_own(), &[&policy("firm", text)]);
        assert!(!effective.settings().tools.web_search, "{text}");
        assert!(effective.is_locked("tools.web_search") && effective.is_locked("tools.browser"));
        assert!(!effective.is_locked("skills.extra"), "{text}");
    }
    let values = registry::parse(vec![("foo.tools_note".to_owned(), RegValue::Dword(1))]);
    let source = FakeSource {
        name: registry::SOURCE,
        read: move || PolicyRead::Read(values.clone()),
    };
    let effective = Effective::load(&locations(), &users_own(), &[&source]);
    assert!(effective.is_locked("tools.browser"));
}

#[test]
fn a_section_nested_under_an_unknown_wrapper_locks_it() {
    for text in [
        r#"{"clauseo": {"skills": {"extra": "off"}}}"#,
        r#"{"a": {"b": {"skills": {"extra": "off"}}}}"#,
        r#"{"firm": {"policy": {"Skills": {}}}}"#,
    ] {
        let effective = Effective::load(&locations(), &users_own(), &[&policy("firm", text)]);
        assert_eq!(
            effective.settings().skills.extra,
            SkillsExtra::Off,
            "{text}"
        );
        assert!(effective.is_locked("skills.extra"), "{text}");
        let reason = effective.lock_reason("skills.extra").unwrap();
        assert_eq!(reason.cause, LockCause::MisnamedSection, "{text}");
        // Other sections stay the lawyer's.
        assert!(!effective.is_locked("tools.browser"), "{text}");
    }
    // A far-off nested name only warns.
    let firm = policy("firm", r#"{"colour": {"theme": "x"}}"#);
    let effective = Effective::load(&locations(), &users_own(), &[&firm]);
    assert_eq!(effective.locked().count(), 0);
    assert_ne!(effective.warnings(), []);
    // The lawyer's own file only warns.
    let files = user(r#"{"clauseo": {"skills": {"extra": "off"}}, "skills": {"extra": "any"}}"#);
    let effective = Effective::load(&locations(), &files, &[]);
    assert_eq!(effective.locked().count(), 0);
    assert_eq!(effective.settings().skills.extra, SkillsExtra::Any);
}
