//! The `skills` keys (decision record 0040): `extra` and `allow_scripts` the lawyer's unless a
//! policy locks them, `curated` the firm's alone, `enabled` the lawyer's map of turned-on packs,
//! and the fail-closed rule for the three security keys.

use super::Effective;
use crate::error::Error;
use crate::policy::registry::{self, RegValue};
use crate::policy::{PolicyRead, PolicySource};
use crate::schema::{SkillsExtra, SkillsSettings};
use crate::testing::{FakeSource, MemFiles, abs, locations, policy, user};
use crate::warning::{Origin, Warning, WarningKind};

const SECURITY: [&str; 3] = ["skills.extra", "skills.allow_scripts", "skills.curated"];

/// A recorded hash: `sha256:` and 64 lowercase hex digits, from `digit`.
fn hash(digit: char) -> String {
    format!("sha256:{}", digit.to_string().repeat(64))
}

/// `settings.json` with every lawyer's `skills` key on, and a `curated` it may not set.
fn all_on() -> MemFiles {
    user(&format!(
        r#"{{"skills": {{"extra": "any", "allow_scripts": true, "curated": ["nda-kit"],
            "enabled": {{"nda-kit": "{}"}}}}}}"#,
        hash('a')
    ))
}

fn unreadable() -> FakeSource<impl Fn() -> PolicyRead> {
    FakeSource {
        name: "broken",
        read: || {
            PolicyRead::Unreadable(Error::Parse {
                path: abs("policy.json"),
                line: 1,
                column: 1,
                message: "expected value".to_owned(),
            })
        },
    }
}

/// Asserts the three security keys are locked off: no extra pack, no script, none curated.
fn locked_off(effective: &Effective, case: &str) {
    let skills = &effective.settings().skills;
    assert_eq!(skills.extra, SkillsExtra::Off, "{case}");
    assert!(!skills.allow_scripts, "{case}");
    assert!(skills.curated.is_empty(), "{case}");
    for key in SECURITY {
        assert!(effective.is_locked(key), "{case}: {key}");
    }
}

/// Each warning's key, in order.
fn warning_keys(warnings: &[Warning]) -> Vec<&str> {
    warnings
        .iter()
        .map(|warning| match &warning.kind {
            WarningKind::InvalidValue { key, .. } | WarningKind::UnknownKey { key } => key.as_str(),
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn skills_extra_defaults_off() {
    let effective = Effective::load(&locations(), &MemFiles::default(), &[]);
    assert_eq!(effective.settings().skills, SkillsSettings::default());
    assert_eq!(effective.settings().skills.extra, SkillsExtra::Off);
    assert!(!effective.settings().skills.allow_scripts);
    assert_eq!(effective.locked().count(), 0);
    // The lawyer's own `extra` and `allow_scripts` load, unlocked (0040, B-who).
    let effective = Effective::load(&locations(), &all_on(), &[]);
    assert_eq!(effective.settings().skills.extra, SkillsExtra::Any);
    assert!(effective.settings().skills.allow_scripts);
    assert_eq!(effective.locked().count(), 0);
    // A value of the lawyer's that does not type goes back to `off`, locking nothing.
    let files = user(r#"{"skills": {"extra": "all", "allow_scripts": "yes"}}"#);
    let effective = Effective::load(&locations(), &files, &[]);
    assert_eq!(effective.settings().skills, SkillsSettings::default());
    assert_eq!(effective.locked().count(), 0);
    assert_eq!(
        warning_keys(effective.warnings()),
        ["skills.extra", "skills.allow_scripts"]
    );
}

#[test]
fn an_unreadable_policy_source_locks_skills_extra_off_scripts_off_and_curated_empty() {
    let firm = policy("firm", r#"{"skills": {"curated": ["nda-kit"]}}"#);
    // A weaker unreadable source still locks what the stronger one left alone.
    let effective = Effective::load(&locations(), &all_on(), &[&unreadable()]);
    locked_off(&effective, "unreadable");
    // The lawyer's turned-on packs are not a security key: they stay, unlocked.
    assert_eq!(effective.settings().skills.enabled.len(), 1);
    assert!(!effective.is_locked("skills.enabled"));
    for key in SECURITY {
        assert!(
            effective.warnings().iter().any(|warning| matches!(
                &warning.kind,
                WarningKind::InvalidValue { key: k, message } if k == key && message.ends_with("locked off")
            )),
            "{key}"
        );
    }
    // A curated list a stronger source set stands.
    let effective = Effective::load(&locations(), &all_on(), &[&firm, &unreadable()]);
    assert_eq!(effective.settings().skills.curated, ["nda-kit"]);
    assert_eq!(effective.settings().skills.extra, SkillsExtra::Off);
    assert!(SECURITY.iter().all(|key| effective.is_locked(key)));
}

#[test]
fn a_malformed_policy_value_for_each_skills_key_fails_closed() {
    for (text, key) in [
        (r#"{"skills": {"extra": "all"}}"#, "skills.extra"),
        (r#"{"skills": {"extra": true}}"#, "skills.extra"),
        (
            r#"{"skills": {"allow_scripts": "yes"}}"#,
            "skills.allow_scripts",
        ),
        (
            r#"{"skills": {"allow_scripts": 1}}"#,
            "skills.allow_scripts",
        ),
        (r#"{"skills": {"curated": "nda-kit"}}"#, "skills.curated"),
        (
            r#"{"skills": {"curated": ["nda-kit", 7]}}"#,
            "skills.curated",
        ),
        // One entry that is no pack id: what the firm meant to admit is unknown.
        (
            r#"{"skills": {"curated": ["nda-kit", "clauseo"]}}"#,
            "skills.curated",
        ),
        (r#"{"skills": {"enabled": ["nda-kit"]}}"#, "skills.enabled"),
    ] {
        let firm = policy("firm", text);
        let effective = Effective::load(&locations(), &all_on(), &[&firm]);
        let skills = &effective.settings().skills;
        assert!(effective.is_locked(key), "{text}");
        match key {
            "skills.extra" => assert_eq!(skills.extra, SkillsExtra::Off, "{text}"),
            "skills.allow_scripts" => assert!(!skills.allow_scripts, "{text}"),
            "skills.curated" => assert!(skills.curated.is_empty(), "{text}"),
            _ => assert!(skills.enabled.is_empty(), "{text}"),
        }
        // Only that key is locked: the lawyer's others stand.
        assert_eq!(effective.locked().collect::<Vec<_>>(), [key], "{text}");
        assert_eq!(
            warning_keys(effective.warnings()).last(),
            Some(&key),
            "{text}"
        );
    }
}

#[test]
fn a_policy_extra_locks_and_set_is_refused_as_locked() {
    let firm = policy(
        "firm",
        r#"{"skills": {"extra": "curated", "allow_scripts": false, "curated": ["nda-kit", "tax", "nda-kit"]}}"#,
    );
    let effective = Effective::load(&locations(), &all_on(), &[&firm]);
    let skills = &effective.settings().skills;
    assert_eq!(skills.extra, SkillsExtra::Curated);
    assert!(!skills.allow_scripts);
    assert_eq!(skills.curated, ["nda-kit", "tax"]);
    assert!(SECURITY.iter().all(|key| effective.is_locked(key)));
    assert!(!effective.is_locked("skills.enabled"));
    // A policy `any` locks too: a policy value locks whatever it says.
    let firm = policy("firm", r#"{"skills": {"extra": "any"}}"#);
    let effective = Effective::load(&locations(), &MemFiles::default(), &[&firm]);
    assert_eq!(effective.settings().skills.extra, SkillsExtra::Any);
    assert!(effective.is_locked("skills.extra"));
    // The one `skills` write, of `skills.enabled`, is refused once a policy sets that key.
    let firm = policy("firm", r#"{"skills": {"enabled": {}}}"#);
    let files = all_on();
    let mut effective = Effective::load(&locations(), &files, &[&firm]);
    let before = files.text(locations().settings_file());
    assert!(matches!(
        effective.set_skill_pack(&files, "tax", Some(&hash('b'))),
        Err(Error::Locked { key }) if key == "skills.enabled"
    ));
    assert_eq!(files.text(locations().settings_file()), before);
}

#[test]
fn curated_in_settings_json_is_ignored_with_a_warning() {
    let effective = Effective::load(&locations(), &all_on(), &[]);
    // Never admits anything: `extra` is the lawyer's `any`, but `curated` stays empty.
    assert_eq!(effective.settings().skills.curated, Vec::<String>::new());
    assert!(!effective.is_locked("skills.curated"));
    match effective.warnings() {
        [
            Warning {
                origin: Origin::File { position, .. },
                kind: WarningKind::InvalidValue { key, message },
            },
        ] => {
            assert_eq!(key, "skills.curated");
            assert!(message.starts_with("only a policy can set"), "{message}");
            assert!(position.is_some());
        }
        other => panic!("{other:?}"),
    }
    // A lawyer's `curated` with `extra: curated` admits nothing either.
    let files = user(r#"{"skills": {"extra": "curated", "curated": ["nda-kit"]}}"#);
    let effective = Effective::load(&locations(), &files, &[]);
    assert_eq!(effective.settings().skills.extra, SkillsExtra::Curated);
    assert_eq!(effective.settings().skills.curated, Vec::<String>::new());
}

#[test]
fn curated_reads_a_reg_multi_sz() {
    let values = registry::parse(vec![
        (
            "Skills.Curated".to_owned(),
            RegValue::MultiSz(vec!["nda-kit".to_owned(), "tax".to_owned()]),
        ),
        (
            "skills.extra".to_owned(),
            RegValue::Sz("curated".to_owned()),
        ),
    ]);
    let source = FakeSource {
        name: registry::SOURCE,
        read: move || PolicyRead::Read(values.clone()),
    };
    assert_eq!(source.name(), "registry");
    let effective = Effective::load(&locations(), &all_on(), &[&source]);
    assert_eq!(effective.settings().skills.curated, ["nda-kit", "tax"]);
    assert_eq!(effective.settings().skills.extra, SkillsExtra::Curated);
    assert!(effective.is_locked("skills.curated"));
    // A `REG_SZ` for the list is the wrong type: it locks the list empty.
    let values = registry::parse(vec![(
        "skills.curated".to_owned(),
        RegValue::Sz("nda-kit".to_owned()),
    )]);
    let source = FakeSource {
        name: registry::SOURCE,
        read: move || PolicyRead::Read(values.clone()),
    };
    let effective = Effective::load(&locations(), &all_on(), &[&source]);
    assert_eq!(effective.settings().skills.curated, Vec::<String>::new());
    assert!(effective.is_locked("skills.curated"));
}

#[test]
fn set_skill_pack_records_and_removes_a_hash_and_a_fresh_load_agrees() {
    let files = user(r#"{"ui": {"theme": "dark"}, "skills": {"extra": "any"}}"#);
    let mut effective = Effective::load(&locations(), &files, &[]);
    effective
        .set_skill_pack(&files, "nda-kit", Some(&hash('a')))
        .unwrap();
    effective
        .set_skill_pack(&files, "tax", Some(&hash('b')))
        .unwrap();
    // Turning a pack on again overwrites its hash.
    effective
        .set_skill_pack(&files, "tax", Some(&hash('c')))
        .unwrap();
    let enabled = |effective: &Effective| effective.settings().skills.enabled.clone();
    assert_eq!(
        enabled(&effective).into_iter().collect::<Vec<_>>(),
        [
            ("nda-kit".to_owned(), hash('a')),
            ("tax".to_owned(), hash('c'))
        ]
    );
    let fresh = Effective::load(&locations(), &files, &[]);
    assert_eq!(enabled(&fresh), enabled(&effective));
    assert_eq!(fresh.warnings(), []);
    // The rest of the file stays.
    assert_eq!(fresh.settings().skills.extra, SkillsExtra::Any);
    assert_eq!(fresh.settings().ui.theme, crate::schema::Theme::Dark);

    effective.set_skill_pack(&files, "nda-kit", None).unwrap();
    assert_eq!(enabled(&effective).keys().collect::<Vec<_>>(), ["tax"]);
    let fresh = Effective::load(&locations(), &files, &[]);
    assert_eq!(enabled(&fresh), enabled(&effective));

    // An id that is no pack id, or a hash of another form, is refused, naming no value.
    let before = files.text(locations().settings_file());
    for (id, hash) in [
        ("clauseo", Some(hash('a'))),
        ("ghp_SECRET", Some(hash('a'))),
        ("ghp_SECRET", None),
        ("nda-kit", Some("sha256:ABC".to_owned())),
        ("nda-kit", Some("ghp_SECRET".to_owned())),
    ] {
        let error = effective
            .set_skill_pack(&files, id, hash.as_deref())
            .unwrap_err();
        assert!(
            matches!(&error, Error::Refused { key, .. } if key == "skills.enabled"),
            "{error:?}"
        );
        assert!(!error.to_string().contains("SECRET"), "{error}");
    }
    assert_eq!(files.text(locations().settings_file()), before);
    // A failed write changes nothing in memory.
    let broken = MemFiles {
        fail_writes: true,
        ..MemFiles::default()
    };
    assert!(matches!(
        effective.set_skill_pack(&broken, "law", Some(&hash('d'))),
        Err(Error::Io { .. })
    ));
    assert_eq!(enabled(&effective).keys().collect::<Vec<_>>(), ["tax"]);
}

#[test]
fn a_locked_enabled_refuses_set_skill_pack() {
    let firm = policy(
        "firm",
        format!(r#"{{"skills": {{"enabled": {{"tax": "{}"}}}}}}"#, hash('f')),
    );
    let files = all_on();
    let mut effective = Effective::load(&locations(), &files, &[&firm]);
    // The firm's map replaces the lawyer's whole map, and locks it.
    assert_eq!(
        effective
            .settings()
            .skills
            .enabled
            .keys()
            .collect::<Vec<_>>(),
        ["tax"]
    );
    assert!(effective.is_locked("skills.enabled"));
    let before = files.text(locations().settings_file());
    for hash in [Some(hash('b')), None] {
        assert!(matches!(
            effective.set_skill_pack(&files, "tax", hash.as_deref()),
            Err(Error::Locked { key }) if key == "skills.enabled"
        ));
    }
    assert_eq!(files.text(locations().settings_file()), before);
    assert_eq!(effective.settings().skills.enabled["tax"], hash('f'));
    // An unreadable source does not lock it: it is no security key.
    let mut effective = Effective::load(&locations(), &files, &[&unreadable()]);
    effective
        .set_skill_pack(&files, "tax", Some(&hash('b')))
        .unwrap();
}

#[test]
fn an_enabled_entry_with_a_bad_id_or_hash_is_dropped_with_a_warning() {
    let files = user(&format!(
        r#"{{"skills": {{"enabled": {{
            "nda-kit": "{good}",
            "tax": "{upper}",
            "law": "{short}",
            "deeds": 7,
            "clauseo": "{good}",
            "stetdoc": "{good}"
        }}}}}}"#,
        good = hash('a'),
        upper = hash('A'),
        short = &hash('a')[..60],
    ));
    let effective = Effective::load(&locations(), &files, &[]);
    assert_eq!(
        effective
            .settings()
            .skills
            .enabled
            .keys()
            .collect::<Vec<_>>(),
        ["nda-kit"]
    );
    let mut keys = warning_keys(effective.warnings());
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "skills.enabled.clauseo",
            "skills.enabled.deeds",
            "skills.enabled.law",
            "skills.enabled.stetdoc",
            "skills.enabled.tax"
        ]
    );
    for warning in effective.warnings() {
        assert!(!warning.to_string().contains(&hash('A')[7..]), "{warning}");
    }
    // The same entries in a policy are dropped too; the rest of its map stands, locked.
    let firm = policy("firm", files.text(locations().settings_file()).unwrap());
    let effective = Effective::load(&locations(), &MemFiles::default(), &[&firm]);
    assert_eq!(effective.settings().skills.enabled.len(), 1);
    assert!(effective.is_locked("skills.enabled"));
}

#[test]
fn a_bad_enabled_id_is_reported_by_place_without_echoing_text() {
    let files = user(&format!(
        r#"{{"skills": {{"enabled": {{
            "nda-kit": "{good}",
            "ghp_PastedToken123": "{good}",
            "Nda-Kit": "{good}",
            "nda kit=secret": "{good}",
            "nda--kit": "{good}"
        }}}}}}"#,
        good = hash('a'),
    ));
    let effective = Effective::load(&locations(), &files, &[]);
    assert_eq!(effective.settings().skills.enabled.len(), 1);
    let text: Vec<String> = effective
        .warnings()
        .iter()
        .map(ToString::to_string)
        .collect();
    for warning in &text {
        for pasted in ["ghp_", "PastedToken", "Nda-Kit", "secret", "nda kit"] {
            assert!(!warning.contains(pasted), "{warning}");
        }
    }
    let mut keys = warning_keys(effective.warnings());
    keys.sort_unstable();
    // Shown by place unless id-shaped: `nda--kit` is, so it reads as itself.
    assert_eq!(keys.len(), 4);
    assert!(keys.contains(&"skills.enabled.nda--kit"), "{keys:?}");
    assert_eq!(
        keys.iter()
            .filter(|key| key.starts_with("skills.enabled.<id "))
            .count(),
        3,
        "{keys:?}"
    );
}

#[test]
fn a_misspelt_skills_policy_key_locks_the_security_keys_off() {
    for text in [
        r#"{"skills": {"enbld": {"nda-kit": "x"}}}"#,
        r#"{"skills": {"extras": "any"}}"#,
        r#"{"skills": "any"}"#,
        r#"{"Skills": {"extra": "any"}}"#,
        r#"{"skills.extra": "any"}"#,
    ] {
        let firm = policy("firm", text);
        let effective = Effective::load(&locations(), &all_on(), &[&firm]);
        let skills = &effective.settings().skills;
        assert_eq!(skills.extra, SkillsExtra::Off, "{text}");
        assert!(effective.is_locked("skills.extra"), "{text}");
        if text.contains("extras") || text.contains("skills.extra\"") {
            continue;
        }
        locked_off(&effective, text);
    }
    // A misspelt key on the registry locks them too, with a warning naming the shape.
    let values = registry::parse(vec![("skills.allowscrips".to_owned(), RegValue::Dword(1))]);
    let source = FakeSource {
        name: registry::SOURCE,
        read: move || PolicyRead::Read(values.clone()),
    };
    let effective = Effective::load(&locations(), &all_on(), &[&source]);
    assert!(!effective.settings().skills.allow_scripts);
    assert!(effective.is_locked("skills.allow_scripts"));
    assert!(effective.warnings().iter().any(|warning| matches!(
        &warning.kind,
        WarningKind::InvalidValue { message, .. } if message.contains("`skills.allowscrips`")
    )));
}
