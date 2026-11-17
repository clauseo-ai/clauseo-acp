//! The `env` read from Claude Code's settings sources: the files, their order, and what makes
//! the read unsure.

use std::path::{Path, PathBuf};

use std::ffi::OsStr;

use super::{ClaudeSources, SettingsEnv, plist_paths_for};
use crate::{Files as _, SystemFiles, own_signin_reaches_endpoint};

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
            global: Some(&config),
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

impl SettingsEnv {
    /// [`SettingsEnv::add_plists`] with no paths.
    fn add_plist_reads(&mut self, reads: impl IntoIterator<Item = Result<Option<Vec<u8>>, ()>>) {
        self.add_plists(reads.into_iter().map(|read| (None, read)));
    }
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
        global: None,
        user: true,
        os_policy: false,
    });
    assert_eq!(read, SettingsEnv::default());
}

/// The global config, then the user settings, then the policy tier: the remote settings over the
/// policy files and the drop-ins in name order, a name the remote settings set (in any case)
/// taking the remote value under the files' spelling. Only the checked names, a key's value only
/// as set or empty.
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
            pair("anthropic_base_url", "https://e.example"),
            pair("ANTHROPIC_BASE_URL", "https://e.example"),
            pair("ANTHROPIC_AUTH_TOKEN", "set"),
            pair("CLAUDE_CODE_USE_VERTEX", "0"),
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
    alone.add_registry_tiers(vec![Ok(None), Ok(Some(hkcu.to_owned()))], false);
    assert_eq!(
        pairs(&alone),
        [
            pair("ANTHROPIC_API_KEY", "set"),
            pair("ANTHROPIC_BASE_URL", "https://u.example")
        ]
    );
    let mut under_admin = SettingsEnv::default();
    under_admin.add_registry_tiers(
        vec![
            Ok(Some(r#"{"permissions": {}}"#.to_owned())),
            Ok(Some(hkcu.to_owned())),
        ],
        false,
    );
    assert_eq!(
        pairs(&under_admin),
        [pair("ANTHROPIC_BASE_URL", "https://u.example")]
    );
    // Another policy tier's content counts as HKLM's does; HKCU's default address is dropped.
    let default = r#"{"env": {"ANTHROPIC_BASE_URL": "https://api.anthropic.com"}}"#;
    let mut under_files = SettingsEnv::default();
    under_files.add_registry_tiers(vec![Ok(None), Ok(Some(default.to_owned()))], true);
    assert_eq!(under_files.pairs(), []);
    let mut unreadable = SettingsEnv::default();
    unreadable.add_registry_tiers(vec![Err(()), Ok(Some("  ".to_owned()))], false);
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

/// The plists read: the login user's, then the computer's, or the computer's alone without one.
#[test]
fn the_plists_are_the_login_user_s_then_the_computer_s() {
    let names = |user| -> Vec<PathBuf> { plist_paths_for(user) };
    assert_eq!(names(None).len(), 1);
    let dir = Path::new("/Library/Managed Preferences");
    let file = "com.anthropic.claudecode.plist";
    assert_eq!(
        names(Some("lawyer")),
        [dir.join("lawyer").join(file), dir.join(file)]
    );
    assert_eq!(names(Some("..")).len(), 1);
    assert!(names(None)[0].ends_with(Path::new("com.anthropic.claudecode.plist")));
}

/// A managed-preferences property list whose top-level dictionary holds `entries`.
fn plist(entries: &str) -> Vec<u8> {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>{entries}</dict></plist>"#
    )
    .into_bytes()
}

/// A JSON policy document's read.
fn policy_doc(text: &str) -> SettingsEnv {
    let mut read = SettingsEnv::default();
    read.add_json(Ok(Some(text.as_bytes().to_vec())), true);
    read
}

/// Whether a launch whose own env is `vars` would offer the sign-in under `settings`.
fn reaches(vars: &[(&str, &str)], settings: &SettingsEnv) -> bool {
    let env = vars
        .iter()
        .map(|(name, value)| (OsStr::new(name), OsStr::new(value)));
    own_signin_reaches_endpoint(env, settings)
}

/// A policy document naming a policy helper makes the read unsure and drops the policy tier's
/// own pairs, as the helper's output takes the tier's place: with no key the launch is refused.
/// The same key in the user's settings or the global config changes nothing.
#[test]
fn a_policy_helper_in_a_policy_document_makes_the_read_unsure() {
    let key = r#""env": {"ANTHROPIC_API_KEY": "k"}"#;
    let files = [
        "managed/managed-settings.json",
        "managed/managed-settings.d/10-a.json",
        "config/remote-settings.json",
    ];
    for (index, path) in files.iter().enumerate() {
        for (kind, helper) in [
            r#""policyHelper": {"command": "x"}"#,
            r#""policyHelpers": []"#,
        ]
        .iter()
        .enumerate()
        {
            let temp = Temp::new(&format!("helper-{index}-{kind}"));
            temp.put(path, &format!("{{{helper}, {key}}}"));
            temp.put(
                "managed/managed-settings.d/20-b.json",
                &format!("{{{key}}}"),
            );
            let read = temp.read(true);
            assert!(read.unsure(), "{path}: {helper}");
            assert_eq!(read.pairs(), [], "{path}: {helper}");
            assert!(reaches(&[], &read), "{path}: {helper}");
            assert!(!reaches(&[("ANTHROPIC_API_KEY", "stored")], &read));
        }
    }
    let mut plists = SettingsEnv::default();
    plists.add_plist_reads([Ok(Some(plist("<key>policyHelper</key><dict/>")))]);
    let mut hklm = SettingsEnv::default();
    hklm.add_registry_tiers(vec![Ok(Some(r#"{"policyHelpers": []}"#.to_owned()))], false);
    let mut hkcu = SettingsEnv::default();
    hkcu.add_registry_tiers(
        vec![Ok(None), Ok(Some(r#"{"policyHelper": 1}"#.to_owned()))],
        false,
    );
    for os in [plists, hklm, hkcu] {
        let files = policy_doc(&format!("{{{key}}}"));
        let read = SettingsEnv::policy([SettingsEnv::default(), os, files]);
        assert!(read.unsure() && read.pairs().is_empty(), "{read:?}");
        assert!(reaches(&[], &read));
    }
    for (index, path) in ["config/settings.json", "config/.claude.json"]
        .iter()
        .enumerate()
    {
        let temp = Temp::new(&format!("helper-user-{index}"));
        temp.put(path, &format!(r#"{{"policyHelper": {{}}, {key}}}"#));
        let read = temp.read(true);
        assert!(!read.unsure(), "{path}");
        assert_eq!(pairs(&read), [pair("ANTHROPIC_API_KEY", "set")], "{path}");
    }
}

/// The policy tiers join highest first, the remote settings, then the OS's, then the files: the
/// first to set a name in any case wins, and once a tier has set a key, token or provider switch,
/// a lower tier's that would turn the sign-in off is dropped.
#[test]
fn the_policy_tiers_join_highest_first() {
    let gateway = r#"{"env": {"ANTHROPIC_BASE_URL": "https://gateway.firm.example"}}"#;
    let default_plist = plist(
        "<key>env</key><dict><key>ANTHROPIC_BASE_URL</key>\
         <string>https://api.anthropic.com</string></dict>",
    );
    let mut mdm = SettingsEnv::default();
    mdm.add_plist_reads([Ok(Some(default_plist))]);
    let mut hklm = SettingsEnv::default();
    let default = r#"{"env": {"ANTHROPIC_BASE_URL": "https://api.anthropic.com"}}"#;
    hklm.add_registry_tiers(vec![Ok(Some(default.to_owned()))], false);
    for os in [mdm, hklm] {
        let read = SettingsEnv::policy([policy_doc(gateway), os, SettingsEnv::default()]);
        assert_eq!(
            pairs(&read),
            [pair("ANTHROPIC_BASE_URL", "https://gateway.firm.example")]
        );
        assert!(reaches(&[], &read));
    }
    let read = SettingsEnv::policy([
        policy_doc(r#"{"env": {"anthropic_base_url": "https://a.example"}}"#),
        policy_doc(
            r#"{"env": {"ANTHROPIC_AUTH_TOKEN": "", "ANTHROPIC_BASE_URL": "https://b.example"}}"#,
        ),
        policy_doc(r#"{"env": {"ANTHROPIC_API_KEY": "k", "CLAUDE_CODE_USE_BEDROCK": "1"}}"#),
    ]);
    assert_eq!(
        pairs(&read),
        [
            pair("anthropic_base_url", "https://a.example"),
            pair("ANTHROPIC_AUTH_TOKEN", ""),
            pair("ANTHROPIC_BASE_URL", "https://a.example"),
        ]
    );
    // A lower tier's key stays when no higher tier set one, and an empty one always stays.
    let read = SettingsEnv::policy([
        policy_doc(gateway),
        SettingsEnv::default(),
        policy_doc(r#"{"env": {"ANTHROPIC_API_KEY": "k"}}"#),
    ]);
    assert!(!reaches(&[], &read));
    let read = SettingsEnv::policy([
        policy_doc(r#"{"env": {"ANTHROPIC_AUTH_TOKEN": "t"}}"#),
        SettingsEnv::default(),
        policy_doc(r#"{"env": {"ANTHROPIC_API_KEY": ""}}"#),
    ]);
    assert_eq!(
        pairs(&read),
        [
            pair("ANTHROPIC_AUTH_TOKEN", "set"),
            pair("ANTHROPIC_API_KEY", "")
        ]
    );
    // On disk: the remote settings' gateway over a policy file's default address.
    let temp = Temp::new("tiers");
    temp.put("config/remote-settings.json", gateway);
    temp.put("managed/managed-settings.json", default);
    assert!(reaches(&[], &temp.read(true)));
}

/// The first property list with content is the OS tier; a later one is not read. One with no
/// content is passed over; after an unsure one, only pairs that can only refuse are kept.
#[test]
fn the_first_property_list_with_content_is_the_tier() {
    let key = plist("<key>env</key><dict><key>ANTHROPIC_API_KEY</key><string>k</string></dict>");
    let other = plist("<key>permissions</key><dict/>");
    let mut first = SettingsEnv::default();
    first.add_plist_reads([Ok(Some(other)), Ok(Some(key.clone()))]);
    assert_eq!(first.pairs(), []);
    let mut empty_first = SettingsEnv::default();
    empty_first.add_plist_reads([Ok(Some(plist(""))), Ok(None), Ok(Some(key.clone()))]);
    assert_eq!(pairs(&empty_first), [pair("ANTHROPIC_API_KEY", "set")]);
    let mut unsure_first = SettingsEnv::default();
    unsure_first.add_plist_reads([Err(()), Ok(Some(key))]);
    assert!(unsure_first.unsure());
    assert_eq!(unsure_first.pairs(), []);
}

/// Without a `CLAUDE_CONFIG_DIR`, the global config is the home folder's `.claude.json` and the
/// remote settings are `~/.claude`'s: the config and global folders differ.
#[test]
fn without_a_config_dir_the_home_folder_s_global_config_is_read() {
    let temp = Temp::new("home");
    temp.put(
        "home/.claude.json",
        r#"{"env": {"ANTHROPIC_BASE_URL": "https://a.example"}}"#,
    );
    temp.put(
        "home/.claude/remote-settings.json",
        r#"{"env": {"ANTHROPIC_AUTH_TOKEN": " "}}"#,
    );
    temp.put("home/.claude/.claude.json", "not read");
    let (home, config, managed) = (
        temp.0.join("home"),
        temp.0.join("home/.claude"),
        temp.0.join("m"),
    );
    let read = SettingsEnv::read(&ClaudeSources {
        managed: &managed,
        config: Some(&config),
        global: Some(&home),
        user: false,
        os_policy: false,
    });
    assert!(!read.unsure());
    assert_eq!(
        pairs(&read),
        [
            pair("ANTHROPIC_BASE_URL", "https://a.example"),
            pair("ANTHROPIC_AUTH_TOKEN", ""),
        ]
    );
}

/// A key or token of only white space is kept as empty, as Claude Code trims them; the
/// variables that name the API's address are kept.
#[test]
fn a_blank_key_is_kept_as_empty_and_the_address_variables_are_kept() {
    let temp = Temp::new("blank");
    temp.put(
        "config/settings.json",
        "{\"env\": {\"ANTHROPIC_API_KEY\": \" \", \"ANTHROPIC_AUTH_TOKEN\": \"\\t\", \
         \"CLAUDE_CODE_API_BASE_URL\": \"https://c.example\", \
         \"ANTHROPIC_UNIX_SOCKET\": \"s.sock\"}}",
    );
    let read = temp.read(true);
    // A document's names come in either order: sorted here.
    let mut kept = pairs(&read);
    kept.sort();
    assert_eq!(
        kept,
        [
            pair("ANTHROPIC_API_KEY", ""),
            pair("ANTHROPIC_AUTH_TOKEN", ""),
            pair("ANTHROPIC_UNIX_SOCKET", "s.sock"),
            pair("CLAUDE_CODE_API_BASE_URL", "https://c.example"),
        ]
    );
    assert!(reaches(&[("ANTHROPIC_API_KEY", "")], &read));
}

mod tiers;
mod unreadable;
