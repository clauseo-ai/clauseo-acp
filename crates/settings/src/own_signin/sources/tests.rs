//! The `env` read from Claude Code's settings sources: the files, their order, and what makes
//! the read unsure.

use std::path::{Path, PathBuf};

use super::{ClaudeSources, SettingsEnv, plist_paths_for};
use crate::{Files as _, SystemFiles};

/// A fresh folder under the temp folder, removed when dropped.
struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("clauseo-own-signin-{name}-{}", std::process::id()));
        remove(&dir);
        Self(dir)
    }

    /// Writes `text` at `path` under the folder, making its folders.
    fn put(&self, path: &str, text: &str) {
        SystemFiles
            .write_atomic(&self.0.join(path), text.as_bytes())
            .unwrap();
    }

    fn read(&self, user: bool) -> SettingsEnv {
        let (managed, config) = (self.0.join("managed"), self.0.join("config"));
        SettingsEnv::read(&ClaudeSources {
            managed: &managed,
            config: Some(&config),
            user,
            os_policy: false,
        })
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        remove(&self.0);
    }
}

/// Removes a test's folder, if it is there.
#[expect(
    clippy::disallowed_methods,
    reason = "a test's own temp folder, removed whole"
)]
fn remove(dir: &Path) {
    if let Err(error) = std::fs::remove_dir_all(dir) {
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{error}");
    }
}

fn pairs(env: &SettingsEnv) -> Vec<(String, String)> {
    env.pairs()
        .iter()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect()
}

fn pair(name: &str, value: &str) -> (String, String) {
    (name.to_owned(), value.to_owned())
}

/// Nothing there: nothing added, and sure.
#[test]
fn no_settings_files_add_nothing() {
    let temp = Temp::new("none");
    assert_eq!(temp.read(true), SettingsEnv::default());
    let missing = temp.0.join("not-there");
    let read = SettingsEnv::read(&ClaudeSources {
        managed: &missing,
        config: None,
        user: true,
        os_policy: false,
    });
    assert_eq!(read, SettingsEnv::default());
}

/// The global config, then the user settings, then the policy files, the drop-ins in name order
/// and the remote settings: only the checked names, a key's value only as set or empty.
#[test]
fn the_sources_apply_in_claude_code_s_order_the_policy_tier_last() {
    let temp = Temp::new("order");
    let env = |vars: &str| format!(r#"{{"env": {{{vars}}}, "other": 1}}"#);
    temp.put(
        "config/.claude.json",
        &env(r#""ANTHROPIC_API_KEY": "", "PATH": "x""#),
    );
    temp.put(
        "config/settings.json",
        &env(r#""ANTHROPIC_BASE_URL": "https://a.example""#),
    );
    temp.put(
        "managed/managed-settings.json",
        &env(r#""ANTHROPIC_BASE_URL": "https://b.example""#),
    );
    temp.put(
        "managed/managed-settings.d/20-c.json",
        &env(r#""CLAUDE_CODE_USE_VERTEX": 0"#),
    );
    temp.put(
        "managed/managed-settings.d/10-d.json",
        &env(r#""ANTHROPIC_AUTH_TOKEN": "secret""#),
    );
    temp.put("managed/managed-settings.d/.hidden.json", "not json");
    temp.put("managed/managed-settings.d/notes.txt", "not json");
    temp.put(
        "config/remote-settings.json",
        &env(r#""anthropic_base_url": "https://e.example""#),
    );
    let read = temp.read(true);
    assert!(!read.unsure());
    assert_eq!(
        pairs(&read),
        [
            pair("ANTHROPIC_API_KEY", ""),
            pair("ANTHROPIC_BASE_URL", "https://a.example"),
            pair("ANTHROPIC_BASE_URL", "https://b.example"),
            pair("ANTHROPIC_AUTH_TOKEN", "set"),
            pair("CLAUDE_CODE_USE_VERTEX", "0"),
            pair("anthropic_base_url", "https://e.example"),
        ]
    );
    // The limits helper's `settingSources: []` leaves out settings.json.
    let helper = pairs(&temp.read(false));
    assert!(
        !helper.contains(&pair("ANTHROPIC_BASE_URL", "https://a.example")),
        "{helper:?}"
    );
}

/// `.config.json` is the global config when present; `.claude.json` is then not read.
#[test]
fn the_legacy_global_config_comes_first_when_present() {
    let temp = Temp::new("legacy");
    temp.put(
        "config/.claude.json",
        r#"{"env": {"ANTHROPIC_BASE_URL": "https://a.example"}}"#,
    );
    temp.put(
        "config/.config.json",
        r#"{"env": {"ANTHROPIC_BASE_URL": "https://b.example"}}"#,
    );
    assert_eq!(
        pairs(&temp.read(true)),
        [pair("ANTHROPIC_BASE_URL", "https://b.example")]
    );
}

/// A file there that cannot be read or parsed, an `env` that is no object, or a checked value
/// of no text kind makes the read unsure; an unchecked one does not.
#[test]
fn an_unreadable_or_unparsable_source_makes_the_read_unsure() {
    let cases: [(&str, &str); 6] = [
        ("managed/managed-settings.json", "{ not json"),
        ("managed/managed-settings.d/a.json", "[1"),
        (
            "config/remote-settings.json",
            r#"{"env": "ANTHROPIC_BASE_URL=x"}"#,
        ),
        (
            "config/.claude.json",
            r#"{"env": {"ANTHROPIC_BASE_URL": ["x"]}}"#,
        ),
        (
            "config/settings.json",
            r#"{"env": {"ANTHROPIC_API_KEY": null}}"#,
        ),
        ("config/settings.json", "\u{feff}{ not json"),
    ];
    for (index, (path, text)) in cases.iter().enumerate() {
        let temp = Temp::new(&format!("unsure-{index}"));
        temp.put(path, text);
        assert!(temp.read(true).unsure(), "{path}: {text}");
    }
    // A folder where the file should be is there but cannot be read.
    let temp = Temp::new("unsure-folder");
    temp.put("managed/managed-settings.json/inside", "");
    assert!(temp.read(true).unsure());
    // A value Claude Code does not check, of any kind, changes nothing.
    let temp = Temp::new("sure");
    temp.put(
        "config/settings.json",
        "\u{feff}{\"env\": {\"OTHER\": [1], \"A\": null}}",
    );
    assert_eq!(
        temp.read(true),
        SettingsEnv {
            content: true,
            ..SettingsEnv::default()
        }
    );
}

/// HKCU's registry tier applies only when no other policy source has content: with one that
/// has, only its base URL is kept. An unreadable value is unsure; an empty one is nothing.
#[test]
fn the_registry_s_user_tier_counts_only_without_admin_content() {
    let hkcu = r#"{"env": {"ANTHROPIC_API_KEY": "k", "ANTHROPIC_BASE_URL": "https://u.example"}}"#;
    let mut alone = SettingsEnv::default();
    alone.add_registry_tiers(vec![Ok(None), Ok(Some(hkcu.to_owned()))]);
    assert_eq!(
        pairs(&alone),
        [
            pair("ANTHROPIC_API_KEY", "set"),
            pair("ANTHROPIC_BASE_URL", "https://u.example")
        ]
    );
    let mut under_admin = SettingsEnv::default();
    under_admin.add_registry_tiers(vec![
        Ok(Some(r#"{"permissions": {}}"#.to_owned())),
        Ok(Some(hkcu.to_owned())),
    ]);
    assert_eq!(
        pairs(&under_admin),
        [pair("ANTHROPIC_BASE_URL", "https://u.example")]
    );
    let mut unreadable = SettingsEnv::default();
    unreadable.add_registry_tiers(vec![Err(()), Ok(Some("  ".to_owned()))]);
    assert!(unreadable.unsure());
    assert_eq!(unreadable.pairs(), []);
}

/// A managed-preferences property list's top-level `env` dictionary; an empty file is nothing,
/// a file that is no property list or an `env` that is no dictionary is unsure.
#[test]
fn a_managed_preferences_plist_s_env_is_read() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>env</key><dict>
<key>ANTHROPIC_BASE_URL</key><string>https://p.example</string>
<key>CLAUDE_CODE_USE_BEDROCK</key><true/>
<key>OTHER</key><array/>
</dict></dict></plist>"#;
    let mut read = SettingsEnv::default();
    read.add_plist(Ok(Some(xml.to_vec())));
    assert!(!read.unsure());
    assert_eq!(
        pairs(&read),
        [
            pair("ANTHROPIC_BASE_URL", "https://p.example"),
            pair("CLAUDE_CODE_USE_BEDROCK", "true")
        ]
    );
    let mut empty = SettingsEnv::default();
    empty.add_plist(Ok(Some(Vec::new())));
    empty.add_plist(Ok(None));
    assert_eq!(empty, SettingsEnv::default());
    for bytes in [
        b"not a plist".to_vec(),
        br#"<plist version="1.0"><dict><key>env</key><string>x</string></dict></plist>"#.to_vec(),
    ] {
        let mut bad = SettingsEnv::default();
        bad.add_plist(Ok(Some(bytes)));
        assert!(bad.unsure());
    }
    let mut unreadable = SettingsEnv::default();
    unreadable.add_plist(Err(()));
    assert!(unreadable.unsure());
}

/// The plists read: the computer's and the login user's, or the computer's alone without one.
#[test]
fn the_plists_are_the_computer_s_and_the_login_user_s() {
    let names = |user| -> Vec<PathBuf> { plist_paths_for(user) };
    assert_eq!(names(None).len(), 1);
    assert_eq!(names(Some("lawyer")).len(), 2);
    assert_eq!(names(Some("..")).len(), 1);
    assert!(names(None)[0].ends_with(Path::new("com.anthropic.claudecode.plist")));
}
