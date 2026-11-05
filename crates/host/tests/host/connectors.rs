//! R24's connector commands at the host, before the host runs them: each is refused, none stores
//! a receipt (decision D2), and `AddApiKey`'s key reaches no refusal, receipt, log or file under
//! the data folder. And the tool switches on the wire against the settings crate's, which now
//! hold `tools.connectors` too.

use std::path::Path;

use clauseo_protocol::{
    ConnectorCommand, ErrorCode, HostCommand, MatterId, RedactedString, ToolsPatch,
    ToolsSettingsView,
};

use crate::support::{Dir, command_id, foreign_uuid};

/// A key that must never land anywhere but the keychain.
const KEY: &str = "ik_live_5c7e19ab3302f4d8e6a1b0c9d8e7f6a5";

fn add_key() -> HostCommand {
    HostCommand::Connector(ConnectorCommand::AddApiKey {
        id: "case-law".to_owned(),
        // As the window sends it: a plain JSON string.
        key: serde_json::from_value::<RedactedString>(serde_json::json!(KEY)).unwrap(),
    })
}

/// Every file under `dir` whose bytes hold `needle`.
fn files_holding(dir: &Path, needle: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        for entry in std::fs::read_dir(&next).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if std::fs::read(&path)
                .unwrap_or_default()
                .windows(needle.len())
                .any(|window| window == needle)
            {
                found.push(path.display().to_string());
            }
        }
    }
    found
}

#[test]
fn connector_commands_are_refused_with_no_receipt_and_the_key_lands_nowhere() {
    let dir = Dir::new("connector_commands_no_receipt", &[]);
    let host = dir.start(0);
    let id = command_id();
    let error = host.dispatch_as(id, add_key()).unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandInvalid, "{error}");
    assert!(!format!("{error:?}").contains(KEY), "{error:?}");
    // Today the host refuses every connector command, and a refusal stores no receipt, so the
    // same id with another body is refused the same way. That says nothing yet about D2's
    // no-receipt rule for commands that run; PR5 tests that.
    let again = host
        .dispatch_as(
            id,
            HostCommand::Connector(ConnectorCommand::SignOut {
                id: "case-law".to_owned(),
            }),
        )
        .unwrap_err();
    assert_eq!(again.code(), ErrorCode::CommandInvalid, "{again}");
    let matter = MatterId::new(foreign_uuid(9)).unwrap();
    for command in [
        ConnectorCommand::Check {},
        ConnectorCommand::Add {
            id: "case-law".into(),
        },
        ConnectorCommand::AddCredits {
            id: "case-law".into(),
            pack: "pack_200".into(),
        },
        ConnectorCommand::Remove {
            id: "case-law".into(),
        },
        ConnectorCommand::RefreshBalance {
            id: "case-law".into(),
        },
        ConnectorCommand::DismissCaseLawHint { matter },
    ] {
        let what = format!("{command:?}");
        let error = host.dispatch(HostCommand::Connector(command)).unwrap_err();
        assert_eq!(error.code(), ErrorCode::CommandInvalid, "{what}: {error}");
    }
    host.shutdown();
    let holding = files_holding(dir.data(), KEY.as_bytes());
    assert!(holding.is_empty(), "the key is in {holding:?}");
}

/// The tool switches as `clauseo-settings` has them and the wire has them: the same keys, the
/// same values, and the same patch JSON; but `tools.connectors`, which reaches the app as
/// `ConnectorsState::available` and `locked` instead.
#[test]
fn the_tool_switches_mirror_the_settings_crate() {
    let mut settings_tools =
        serde_json::to_value(clauseo_settings::ToolsSettings::default()).unwrap();
    assert_eq!(
        settings_tools.as_object_mut().unwrap().remove("connectors"),
        Some(true.into())
    );
    let wire_tools = serde_json::to_value(ToolsSettingsView {
        web_search: true,
        browser: cfg!(target_os = "macos"),
    })
    .unwrap();
    assert_eq!(settings_tools, wire_tools);
    let settings_patch = clauseo_settings::ToolsPatch {
        web_search: Some(false),
        browser: Some(true),
    };
    let wire_patch = ToolsPatch {
        web_search: Some(false),
        browser: Some(true),
    };
    assert_eq!(
        serde_json::to_value(settings_patch).unwrap(),
        serde_json::to_value(wire_patch).unwrap()
    );
}
