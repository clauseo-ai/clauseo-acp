use super::Effective;
use crate::error::Error;
use crate::locations::Locations;
use crate::policy::{FilePolicy, PolicyRead, PolicySource, file};
use crate::schema::{AgentLaunch, Density, Theme};
use crate::testing::{FakeSource, MemFiles, abs, abs_json};
use crate::warning::WarningKind;
use std::path::Path;

fn locations() -> Locations {
    Locations::under_root(Path::new("/data"))
}

fn user(text: &str) -> MemFiles {
    MemFiles::with(locations().settings_file(), text)
}

/// A source whose `policy.json` is `text`.
fn policy(name: &'static str, text: impl Into<String>) -> FakeSource<impl Fn() -> PolicyRead> {
    let text: String = text.into();
    FakeSource {
        name,
        read: move || match file::parse(Path::new(name), text.as_bytes()) {
            Ok(values) => PolicyRead::Read(values),
            Err(error) => PolicyRead::Unreadable(error),
        },
    }
}

fn absent() -> FakeSource<impl Fn() -> PolicyRead> {
    FakeSource {
        name: "absent",
        read: || PolicyRead::Absent,
    }
}

#[test]
fn no_policy_reports_absent() {
    let effective = Effective::load(&locations(), &MemFiles::default(), &[&absent()]);
    assert!(effective.policy().is_absent());
    assert_eq!(effective.policy().applied(), Vec::<String>::new());
    assert_eq!(effective.policy().errors().len(), 0);
    assert_eq!(effective.settings(), &crate::Settings::default());
    assert_eq!(effective.locked().count(), 0);
    assert_eq!(effective.warnings(), []);
    assert_eq!(effective.data_dir(), Path::new("/data"));
    assert_eq!(effective.locations(), &locations());
    assert_eq!(
        effective.default_matters_dir(),
        Path::new("/data/Documents/Clauseo")
    );
}

#[test]
fn policy_wins_over_user() {
    let files = user(r#"{"ui": {"theme": "dark", "density": "compact"}}"#);
    let firm = policy("firm", r#"{"ui": {"theme": "light"}}"#);
    let effective = Effective::load(&locations(), &files, &[&firm]);
    assert_eq!(effective.settings().ui.theme, Theme::Light);
    assert_eq!(effective.settings().ui.density, Density::Compact);
    assert!(effective.is_locked("ui.theme"));
    assert!(!effective.is_locked("ui.density"));
    assert_eq!(effective.policy().applied(), ["firm"]);
    assert!(!effective.policy().is_absent());
}

#[test]
fn agent_policy_replaces_whole_entry() {
    let text = format!(
        r#"{{"agents": {{
            "claude": {{"command": "{a}", "args": ["--debug"], "env": {{"X": "1"}}}},
            "codex": {{"command": "{b}"}}
        }}}}"#,
        a = abs_json("user/claude"),
        b = abs_json("user/codex"),
    );
    let files = user(&text);
    let firm = policy(
        "firm",
        format!(
            r#"{{"agents": {{"claude": {{"command": "{}"}}}}}}"#,
            abs_json("firm/claude")
        ),
    );
    let effective = Effective::load(&locations(), &files, &[&firm]);
    assert_eq!(
        effective.agent("claude"),
        Some(&AgentLaunch {
            command: abs("firm/claude").display().to_string(),
            args: Vec::new(),
            env: std::collections::BTreeMap::new(),
        })
    );
    assert_eq!(
        effective.agent("codex").map(|codex| codex.command.clone()),
        Some(abs("user/codex").display().to_string())
    );
    assert!(effective.is_locked("agents.claude"));
    assert!(!effective.is_locked("agents.codex"));
}

#[test]
fn locked_keys_listed() {
    let firm = policy(
        "firm",
        r#"{"ui": {"theme": "dark", "font_size": 18, "colour": "red"}, "matters": {}}"#,
    );
    let effective = Effective::load(&locations(), &MemFiles::default(), &[&firm]);
    assert_eq!(
        effective.locked().collect::<Vec<_>>(),
        ["ui.font_size", "ui.theme"]
    );
    assert!(effective.is_locked("ui.theme"));
    assert!(!effective.is_locked("ui.colour"));
    // The policy's warnings come with the user's.
    assert_eq!(effective.warnings().len(), 1);
    assert!(matches!(
        &effective.warnings()[0].kind,
        WarningKind::UnknownKey { key } if key == "ui.colour"
    ));
}

#[test]
fn unreadable_policy_is_reported_and_start_continues() {
    let broken = FakeSource {
        name: "registry",
        read: || {
            PolicyRead::Unreadable(Error::Registry {
                key: "SOFTWARE".to_owned(),
                code: 5,
            })
        },
    };
    let broken_file = policy("policy.json", "{\"ui\": ");
    let firm = policy("firm", r#"{"ui": {"theme": "dark"}}"#);
    let sources: [&dyn PolicySource; 3] = [&broken, &broken_file, &firm];
    let effective = Effective::load(&locations(), &MemFiles::default(), &sources);
    assert_eq!(effective.settings().ui.theme, Theme::Dark);
    assert_eq!(effective.policy().applied(), ["firm"]);
    let errors: Vec<_> = effective
        .policy()
        .errors()
        .iter()
        .map(|error| (error.source.as_str(), error.error.to_string()))
        .collect();
    assert_eq!(
        errors,
        [
            (
                "registry",
                "registry key HKLM\\SOFTWARE: Win32 error 5".to_owned()
            ),
            (
                "policy.json",
                "policy.json:1:7: EOF while parsing a value".to_owned()
            ),
        ]
    );
}

#[test]
fn a_source_that_only_fails_leaves_the_policy_absent() {
    let broken = policy("policy.json", "nope");
    let effective = Effective::load(&locations(), &MemFiles::default(), &[&broken]);
    assert!(effective.policy().is_absent());
    assert_eq!(effective.policy().errors().len(), 1);
}

#[test]
fn an_earlier_source_wins_key_by_key() {
    let registry = policy("registry", r#"{"ui": {"theme": "dark"}}"#);
    let file = policy(
        "file",
        r#"{"ui": {"theme": "light", "density": "compact"}}"#,
    );
    let effective = Effective::load(&locations(), &MemFiles::default(), &[&registry, &file]);
    assert_eq!(effective.settings().ui.theme, Theme::Dark);
    assert_eq!(effective.settings().ui.density, Density::Compact);
    assert_eq!(effective.policy().applied(), ["registry", "file"]);
}

#[test]
fn agent_launch_from_user_settings() {
    let text = format!(
        r#"{{"agents": {{"fake": {{"command": "{}", "args": ["--scenario", "basic"]}}}}}}"#,
        abs_json("bin/fake_agent")
    );
    let effective = Effective::load(&locations(), &user(&text), &[]);
    let fake = effective.agent("fake").unwrap();
    assert_eq!(fake.command, abs("bin/fake_agent").display().to_string());
    assert_eq!(fake.args, ["--scenario", "basic"]);
    assert_eq!(effective.agent("other"), None);
    assert_eq!(effective.warnings(), []);
}

#[test]
fn relative_agent_command_is_a_warning() {
    let files = user(r#"{"agents": {"fake": {"command": "fake_agent"}}}"#);
    let effective = Effective::load(&locations(), &files, &[]);
    assert_eq!(effective.agent("fake"), None);
    assert_eq!(effective.warnings().len(), 1);
    let warning = &effective.warnings()[0];
    assert_eq!(
        warning.kind,
        WarningKind::RelativeAgentCommand {
            id: "fake".to_owned(),
            command: "fake_agent".to_owned(),
        }
    );
    assert_eq!(
        warning.to_string(),
        format!(
            "{}:1:21: agent fake left out: its command fake_agent is not an absolute path",
            locations().settings_file().display()
        )
    );
}

#[test]
fn the_default_location_setting_moves_new_matters() {
    let text = format!(
        r#"{{"matters": {{"default_location": "{}"}}}}"#,
        abs_json("cases")
    );
    let effective = Effective::load(&locations(), &user(&text), &[]);
    assert_eq!(effective.default_matters_dir(), abs("cases"));
}

#[test]
fn a_file_policy_reads_through_files() {
    let path = abs("etc/clauseo/policy.json");
    let files = MemFiles::with(&path, r#"{"ui": {"theme": "dark"}}"#);
    let source = FilePolicy::new(path.clone(), files);
    assert_eq!(source.name(), path.display().to_string());
    let effective = Effective::load(&locations(), &MemFiles::default(), &[&source]);
    assert_eq!(effective.settings().ui.theme, Theme::Dark);
}
