//! The ways a firm's policy failed to lock in the host's review of #450, each its repro: a
//! duplicate the duplicate check could not reach, a security section nested where the misnamed
//! pass did not look, a name that differs from a section by characters the pass did not count,
//! or by separators it did not split on. Each must fail closed.

use super::Effective;
use crate::lock_reason::LockCause;
use crate::policy::PolicyRead;
use crate::policy::managed_prefs::{self, PlistValue};
use crate::policy::registry::{self, RegValue};
use crate::schema::SkillsExtra;
use crate::testing::{FakeSource, MemFiles, abs_json, locations, policy, user};
use std::path::Path;

/// `settings.json` with the lawyer's skills, tools and install keys on or set.
pub(super) fn users_own() -> crate::testing::MemFiles {
    user(&format!(
        r#"{{"skills": {{"extra": "any", "allow_scripts": true}},
            "tools": {{"web_search": true, "browser": true}},
            "install": {{"bundle": "{}"}}}}"#,
        abs_json("bundle")
    ))
}

/// Asserts that `text`, as a `policy.json`, is dropped as malformed: every security key locked
/// off, for the reason that the source cannot be read.
fn assert_malformed(text: &str) {
    let effective = Effective::load(&locations(), &users_own(), &[&policy("firm", text)]);
    let shown = text.get(..60).unwrap_or(text);
    assert_eq!(
        effective.settings().skills.extra,
        SkillsExtra::Off,
        "{shown}"
    );
    assert!(!effective.settings().tools.web_search, "{shown}");
    for key in crate::leaf::SECURITY_KEYS {
        assert!(effective.is_locked(key), "{shown}: {key}");
    }
    assert_eq!(effective.policy().errors().len(), 1, "{shown}");
    match effective.lock_reasons() {
        [reason] => assert_eq!(reason.cause, LockCause::SourceUnreadable, "{shown}"),
        other => panic!("{shown}: {other:?}"),
    }
}

// The duplicate check walks the file with a stricter reader than the one that reads it: a value
// it cannot take before the duplicate stopped it short, and the last copy won.
#[test]
fn a_value_the_duplicate_check_cannot_walk_makes_the_policy_malformed() {
    assert_malformed(r#"{"ui":{"note":1e400},"skills":{"extra":"off","extra":"any"}}"#);
    let deep = format!(
        r#"{{"ui":{{"note":{}{}}},"skills":{{"extra":"off","extra":"any"}}}}"#,
        "[".repeat(200),
        "]".repeat(200)
    );
    assert_malformed(&deep);
}

// Nested names were walked through objects only, and only when the wrapper read as a value.
#[test]
fn a_section_nested_under_an_array_or_an_unreadable_wrapper_locks() {
    let effective = Effective::load(
        &locations(),
        &users_own(),
        &[&policy(
            "firm",
            r#"{"clauseo":[{"skills":{"extra":"any"}}]}"#,
        )],
    );
    assert_eq!(effective.settings().skills.extra, SkillsExtra::Off);
    assert!(effective.is_locked("skills.extra") && effective.is_locked("skills.curated"));
    let reason = effective.lock_reason("skills.extra").unwrap();
    assert_eq!(reason.cause, LockCause::MisnamedSection);
    assert!(!effective.is_locked("tools.browser"));
    assert_malformed(r#"{"clauseo":{"skills":{"extra":"off"},"n":1e400}}"#);
    let pad = format!(
        r#"{{"clauseo":{}{{"skills":{{"extra":"off"}}}}{}}}"#,
        "[".repeat(200),
        "]".repeat(200)
    );
    assert_malformed(&pad);
}

/// The registry holding `values`, read through the real registry parser.
pub(super) fn registry_of(values: Vec<(&str, RegValue)>) -> FakeSource<impl Fn() -> PolicyRead> {
    let values = registry::parse(
        values
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
    );
    FakeSource {
        name: registry::SOURCE,
        read: move || PolicyRead::Read(values.clone()),
    }
}

/// Asserts that the skills keys are locked off, as a misnamed section, by `effective`.
pub(super) fn assert_skills_misnamed(effective: &Effective, shown: &str) {
    assert_eq!(
        effective.settings().skills.extra,
        SkillsExtra::Off,
        "{shown}"
    );
    assert!(!effective.settings().skills.allow_scripts, "{shown}");
    for key in ["skills.extra", "skills.allow_scripts", "skills.curated"] {
        assert!(effective.is_locked(key), "{shown}: {key}");
    }
    let reason = effective.lock_reason("skills.extra").unwrap();
    assert_eq!(reason.cause, LockCause::MisnamedSection, "{shown}");
}

// The distance counted bytes: a zero-width space or two Cyrillic letters put `skills` out of
// reach.
#[test]
fn a_section_name_with_invisible_or_look_alike_characters_locks() {
    for text in [
        r#"{"skil\u200bls": {"extra": "any", "allow_scripts": true}}"#,
        "{\"\u{455}k\u{456}lls\": {\"extra\": \"any\"}}",
    ] {
        let effective = Effective::load(&locations(), &users_own(), &[&policy("firm", text)]);
        assert_skills_misnamed(&effective, text);
        // Never echoed: it is no plain word.
        let reason = effective.lock_reason("skills.extra").unwrap();
        assert_eq!(reason.section, None, "{text}");
    }
    let firm = registry_of(vec![("skil\u{200b}ls.allow_scripts", RegValue::Dword(1))]);
    let effective = Effective::load(&locations(), &users_own(), &[&firm]);
    assert!(!effective.settings().skills.allow_scripts);
    let reason = effective.lock_reason("skills.allow_scripts").unwrap();
    assert_eq!(reason.cause, LockCause::MisnamedSection);
}

// Segments were split only at `.`: `skill_extra` was one word, far from `skills`.
#[test]
fn a_section_joined_to_its_key_by_another_separator_locks() {
    for text in [
        r#"{"skill_extra": "any"}"#,
        r#"{"skill-extra": "any"}"#,
        r#"{"skill allow_scripts": true}"#,
    ] {
        let effective = Effective::load(&locations(), &users_own(), &[&policy("firm", text)]);
        assert_skills_misnamed(&effective, text);
        assert!(!effective.is_locked("tools.browser"), "{text}");
    }
    let effective = Effective::load(
        &locations(),
        &users_own(),
        &[&policy("firm", r#"{"tool_browser": true}"#)],
    );
    assert!(!effective.settings().tools.browser && effective.is_locked("tools.browser"));
    assert!(!effective.is_locked("skills.extra"));
    let firm = registry_of(vec![("tool_browser", RegValue::Dword(1))]);
    let effective = Effective::load(&locations(), &users_own(), &[&firm]);
    assert!(!effective.settings().tools.browser && effective.is_locked("tools.browser"));
}

// Two edits from `mcp` reached `app` and `tmp`; a swap was two edits, so `mpc` was as far.
#[test]
fn a_short_section_locks_one_edit_away_only() {
    for text in [r#"{"app": {"x": 1}}"#, r#"{"tmp": {"servers": {}}}"#] {
        let effective = Effective::load(&locations(), &users_own(), &[&policy("firm", text)]);
        assert_eq!(effective.locked().count(), 0, "{text}");
    }
    let effective = Effective::load(
        &locations(),
        &users_own(),
        &[&policy("firm", r#"{"mpc": {"servers": {}}}"#)],
    );
    assert!(effective.is_locked("mcp.servers"));
    let reason = effective.lock_reason("mcp.servers").unwrap();
    assert_eq!(reason.cause, LockCause::MisnamedSection);
}

// A place was counted among the values, the warnings or the nested names, whichever list the
// name was found in: here 1, among the warnings.
#[test]
fn a_lock_reason_place_counts_the_source_entries_in_document_order() {
    let text = r#"{"ui":{"theme":5},"matters":{},"Skills=x":{"extra":"off"}}"#;
    let effective = Effective::load(&locations(), &users_own(), &[&policy("firm", text)]);
    let reason = effective.lock_reason("skills.extra").unwrap();
    assert_eq!(
        (reason.cause, reason.place),
        (LockCause::MisnamedSection, 3)
    );
    // A malformed value takes its section's place.
    let text = r#"{"matters":{},"x":1,"ui":{"density":"compact","theme":5}}"#;
    let effective = Effective::load(&locations(), &users_own(), &[&policy("firm", text)]);
    let reason = effective.lock_reason("ui.theme").unwrap();
    assert_eq!((reason.cause, reason.place), (LockCause::MalformedValue, 3));
    // The registry's entries are its values.
    let firm = registry_of(vec![
        ("ui.theme", RegValue::Sz("dark".to_owned())),
        ("colour", RegValue::Dword(1)),
        ("tool.browser", RegValue::Dword(1)),
        ("skills.allow_scripts", RegValue::Sz("yes".to_owned())),
    ]);
    let effective = Effective::load(&locations(), &users_own(), &[&firm]);
    let places: Vec<(LockCause, u32)> = effective
        .lock_reasons()
        .iter()
        .map(|reason| (reason.cause, reason.place))
        .collect();
    assert_eq!(
        places,
        [
            (LockCause::MisnamedSection, 3),
            (LockCause::MalformedValue, 4)
        ]
    );
}

// A registry `REG_SZ` holding JSON kept the last copy of a duplicate key, silently.
#[test]
fn a_registry_json_value_with_a_duplicate_key_locks_its_section() {
    let servers = r#"{"a": {"command": "/a"}, "a": {"command": "/b"}}"#;
    let firm = registry_of(vec![
        ("ui.theme", RegValue::Sz("dark".to_owned())),
        ("mcp.servers", RegValue::Sz(servers.to_owned())),
    ]);
    let effective = Effective::load(&locations(), &users_own(), &[&firm]);
    assert!(effective.settings().mcp.servers.is_empty());
    assert!(effective.is_locked("mcp.servers") && effective.is_locked("ui.theme"));
    assert!(!effective.is_locked("skills.extra") && effective.settings().tools.browser);
    let reason = effective.lock_reason("mcp.servers").unwrap();
    assert_eq!(
        (
            reason.cause,
            reason.source,
            reason.section.as_deref(),
            reason.place
        ),
        (
            LockCause::DuplicateKey,
            crate::SourceKind::Registry,
            Some("mcp"),
            2
        )
    );
    assert_eq!(effective.lock_reasons().len(), 1);
    // Nested, in a plan: no plan stands.
    let plan = r#"{"endpoint": "https://a.example", "proxy": {"url": "x", "url": "y"}}"#;
    let firm = registry_of(vec![("plans.claude-acp", RegValue::Sz(plan.to_owned()))]);
    let effective = Effective::load(&locations(), &users_own(), &[&firm]);
    assert!(effective.is_locked("plans."));
    assert_eq!(
        effective.lock_reason("plans.").map(|reason| reason.cause),
        Some(LockCause::DuplicateKey)
    );
    assert!(!effective.is_locked("mcp.servers") && !effective.is_locked("skills.extra"));
}

/// Managed preferences forcing `forced`, read through the real parser, as the shim hands them
/// over once it has asked for every name the domain lists.
pub(super) fn managed(forced: Vec<(&str, PlistValue)>) -> FakeSource<impl Fn() -> PolicyRead> {
    let values: Vec<(String, PlistValue, bool)> = forced
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value, true))
        .collect();
    FakeSource {
        name: managed_prefs::SOURCE,
        read: move || managed_prefs::parse(values.clone()),
    }
}

fn text(value: &str) -> PlistValue {
    PlistValue::String(value.to_owned())
}

// Managed preferences were read by exact key name, so a misnamed key, or a `skills` dictionary,
// had no effect.
#[test]
fn a_misnamed_managed_preference_locks_its_section() {
    for (key, value) in [
        (
            "skills",
            PlistValue::Dict(vec![("extra".to_owned(), text("any"))]),
        ),
        (
            "skill",
            PlistValue::Dict(vec![("extra".to_owned(), text("any"))]),
        ),
        (
            "clauseo",
            PlistValue::Array(vec![PlistValue::Dict(vec![(
                "skills".to_owned(),
                PlistValue::Dict(Vec::new()),
            )])]),
        ),
        ("skil\u{200b}ls", PlistValue::Bool(true)),
    ] {
        let firm = managed(vec![("ui.theme", text("dark")), (key, value)]);
        let effective = Effective::load(&locations(), &users_own(), &[&firm]);
        assert_skills_misnamed(&effective, key);
        let reason = effective.lock_reason("skills.extra").unwrap();
        assert_eq!(
            reason.source,
            crate::SourceKind::ManagedPreferences,
            "{key:?}"
        );
        assert_eq!(reason.place, 2, "{key:?}");
        assert!(!effective.is_locked("tools.browser"), "{key:?}");
    }
    let firm = managed(vec![("skill.extra", text("curated"))]);
    let effective = Effective::load(&locations(), &users_own(), &[&firm]);
    assert_eq!(effective.settings().skills.extra, SkillsExtra::Off);
    assert!(effective.is_locked("skills.extra"));
    // A far-off name only warns.
    let firm = managed(vec![(
        "colour",
        PlistValue::Dict(vec![("theme".to_owned(), text("x"))]),
    )]);
    let effective = Effective::load(&locations(), &users_own(), &[&firm]);
    assert_eq!(effective.locked().count(), 0);
    assert_ne!(effective.warnings(), []);
}

// The shim asks for each listed name as well as the known keys.
#[test]
fn the_managed_preferences_shim_asks_for_every_listed_name() {
    let names = managed_prefs::names_to_ask(
        ["skills", "ui.theme", "Skill.extra", "skills"].map(str::to_owned),
    );
    let known = crate::leaf::KEYS.len() + 2;
    assert_eq!(names.len(), known + 2);
    assert_eq!(&names[known..], ["Skill.extra", "skills"]);
    assert_eq!(names.iter().filter(|name| *name == "ui.theme").count(), 1);
    assert!(names.contains(&"agents".to_owned()) && names.contains(&"plans".to_owned()));
}

// The shim takes the names to ask for from the managed preferences files, XML or binary.
#[test]
fn a_managed_preferences_file_gives_its_key_names() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>skills</key><dict><key>extra</key><string>off</string></dict>
<key>ui.theme</key><string>dark</string></dict></plist>"#;
    let mut binary = Vec::new();
    plist::Value::from_reader(std::io::Cursor::new(&xml[..]))
        .unwrap()
        .to_writer_binary(&mut binary)
        .unwrap();
    let paths = managed_prefs::file_paths("ai.clauseo.desktop", "lawyer").unwrap();
    assert_eq!(
        paths,
        [
            Path::new("/Library/Managed Preferences/ai.clauseo.desktop.plist"),
            Path::new("/Library/Managed Preferences/lawyer/ai.clauseo.desktop.plist"),
        ]
    );
    assert!(managed_prefs::file_paths("d", "../x").is_err());
    assert!(managed_prefs::file_paths("d", "").is_err());
    let files = MemFiles::default();
    files
        .files
        .borrow_mut()
        .insert(paths[0].clone(), xml.to_vec());
    files.files.borrow_mut().insert(paths[1].clone(), binary);
    let names = managed_prefs::file_names(&files, &paths).unwrap();
    assert_eq!(names, ["skills", "ui.theme", "skills", "ui.theme"]);
    // A missing file adds nothing.
    assert_eq!(
        managed_prefs::file_names(&MemFiles::default(), &paths).unwrap(),
        [""; 0]
    );
}

// The re-check of #450: a managed file that could not be read or parsed only lost its names,
// so a misnamed key in it went unseen.
#[test]
fn an_unreadable_managed_preferences_file_locks_every_security_key() {
    let paths = managed_prefs::file_paths("ai.clauseo.desktop", "lawyer").unwrap();
    // A stub that refuses every read with PermissionDenied, and one holding garbage.
    let denied = MemFiles {
        broken: paths.clone(),
        ..MemFiles::default()
    };
    let garbage = MemFiles::with(&paths[0], "not a plist \u{0}");
    let forced = |_: Vec<String>| vec![("ui.theme".to_owned(), text("dark"), true)];
    for files in [&denied, &garbage] {
        let read = managed_prefs::read_with(files, &paths, forced);
        assert!(matches!(read, PolicyRead::Unreadable(_)), "{read:?}");
        let firm = FakeSource {
            name: managed_prefs::SOURCE,
            read: || managed_prefs::read_with(files, &paths, forced),
        };
        let effective = Effective::load(&locations(), &users_own(), &[&firm]);
        for key in crate::leaf::SECURITY_KEYS {
            assert!(effective.is_locked(key), "{key}");
        }
        assert_eq!(effective.settings().skills.extra, SkillsExtra::Off);
        match effective.lock_reasons() {
            [reason] => assert_eq!(
                (reason.cause, reason.source),
                (
                    LockCause::SourceUnreadable,
                    crate::SourceKind::ManagedPreferences
                )
            ),
            other => panic!("{other:?}"),
        }
    }
    // A readable file with no misnamed key: the forced value applies, nothing else locks.
    let fine = MemFiles::with(
        &paths[0],
        r#"<plist version="1.0"><dict><key>ui.theme</key><string>dark</string></dict></plist>"#,
    );
    let read = managed_prefs::read_with(&fine, &paths, forced);
    assert!(matches!(&read, PolicyRead::Read(values) if values.keys().eq(["ui.theme".to_owned()])));
}

// The re-check of #450: a section glued to its key locked nothing.
#[test]
fn a_section_glued_to_its_key_locks() {
    for text in [
        r#"{"SkillExtra":"off"}"#,
        r#"{"skillextra":"off"}"#,
        r#"{"skill:extra":"off"}"#,
        r#"{"skill\\extra":"off"}"#,
    ] {
        let effective = Effective::load(&locations(), &users_own(), &[&policy("firm", text)]);
        assert_eq!(
            effective.settings().skills.extra,
            SkillsExtra::Off,
            "{text}"
        );
        let reason = effective.lock_reason("skills.extra").unwrap();
        assert_eq!(reason.cause, LockCause::MisnamedSection, "{text}");
        assert!(!effective.is_locked("tools.browser"), "{text}");
    }
    for text in [r#"{"ToolBrowser":false}"#, r#"{"tool:browser":false}"#] {
        let effective = Effective::load(&locations(), &users_own(), &[&policy("firm", text)]);
        assert!(!effective.settings().tools.browser, "{text}");
        assert!(effective.is_locked("tools.browser"), "{text}");
        assert!(!effective.is_locked("skills.extra"), "{text}");
    }
    let firm = registry_of(vec![("SkillExtra", RegValue::Sz("off".to_owned()))]);
    let effective = Effective::load(&locations(), &users_own(), &[&firm]);
    assert_eq!(effective.settings().skills.extra, SkillsExtra::Off);
    assert!(effective.is_locked("skills.extra"));
    let firm = managed(vec![("SkillExtra", text("off"))]);
    let effective = Effective::load(&locations(), &users_own(), &[&firm]);
    assert_eq!(effective.settings().skills.extra, SkillsExtra::Off);
    assert!(effective.is_locked("skills.extra"));
}

// The deliberate cost: a name that only starts like a section locks it.
#[test]
fn toolbar_and_skillful_lock_their_sections_deliberately() {
    let effective = Effective::load(
        &locations(),
        &users_own(),
        &[&policy("firm", r#"{"toolbar": true, "skillful": true}"#)],
    );
    assert!(effective.is_locked("tools.browser") && effective.is_locked("skills.extra"));
    assert!(!effective.is_locked("mcp.servers"));
}

// The re-check of #450: the user came from `USER`, which the lawyer sets; a failed lookup must
// lock rather than skip the user's file.
#[test]
fn a_failed_login_user_lookup_locks_every_security_key() {
    let forced = |_: Vec<String>| vec![("ui.theme".to_owned(), text("dark"), true)];
    let failed = || Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
    for looked_up in [|| Ok(None), failed, || Ok(Some("../elsewhere".to_owned()))] {
        let files = MemFiles::default();
        let firm = FakeSource {
            name: managed_prefs::SOURCE,
            read: || managed_prefs::read_for(&files, "ai.clauseo.desktop", looked_up(), forced),
        };
        let effective = Effective::load(&locations(), &users_own(), &[&firm]);
        for key in crate::leaf::SECURITY_KEYS {
            assert!(effective.is_locked(key), "{key}");
        }
        match effective.lock_reasons() {
            [reason] => assert_eq!(
                (reason.cause, reason.source),
                (
                    LockCause::SourceUnreadable,
                    crate::SourceKind::ManagedPreferences
                )
            ),
            other => panic!("{other:?}"),
        }
    }
    let user = managed_prefs::user_paths("ai.clauseo.desktop", Ok(Some("lawyer".to_owned())));
    assert_eq!(
        user.unwrap()[1],
        Path::new("/Library/Managed Preferences/lawyer/ai.clauseo.desktop.plist")
    );
}

/// Set in the child run of `the_login_user_comes_from_the_real_user_id_not_user` to the name
/// the parent looked up.
#[cfg(unix)]
const EXPECTED_USER: &str = "CLAUSEO_TEST_EXPECTED_USER";

// Runs this test again in a child with `USER=nobody`: the managed file it reads is still the
// real user id's.
#[cfg(unix)]
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "reruns this test binary with USER=nobody; nothing else is launched"
)]
fn the_login_user_comes_from_the_real_user_id_not_user() {
    // A CI container may run as a user id with no passwd entry: there is no name to compare,
    // and `a_failed_login_user_lookup_locks_every_security_key` covers that path.
    let Some(looked_up) = managed_prefs::login_user().unwrap() else {
        eprintln!(
            "skipped: the real user id {} has no passwd entry, so there is no login user to \
             compare; a_failed_login_user_lookup_locks_every_security_key covers this case",
            nix::unistd::getuid()
        );
        return;
    };
    if let Some(expected) = std::env::var_os(EXPECTED_USER) {
        assert_eq!(std::env::var("USER").as_deref(), Ok("nobody"));
        assert_eq!(looked_up, expected.to_string_lossy());
        let paths = managed_prefs::user_paths("d", managed_prefs::login_user()).unwrap();
        assert_eq!(
            paths[1],
            Path::new(managed_prefs::MANAGED_DIR)
                .join(&looked_up)
                .join("d.plist")
        );
        return;
    }
    assert_ne!(
        looked_up, "nobody",
        "run this test as a user other than nobody"
    );
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "effective::policy_gaps_tests::the_login_user_comes_from_the_real_user_id_not_user",
            "--nocapture",
        ])
        .env("USER", "nobody")
        .env(EXPECTED_USER, &looked_up)
        .status()
        .unwrap();
    assert!(status.success(), "{status}");
}
