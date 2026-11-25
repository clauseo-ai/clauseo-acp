//! The add-on commands (`addons/change.rs`): their JSON, and the id's length check.

use clauseo_protocol::{
    AddonKind, AddonScope, HostCommand, MAX_ADDON_ID_BYTES, RemoveAddon, SetAddonOn, SetAddonScope,
};

use super::Validate;
use super::fixtures::{matter_id, round_trips};

#[test]
fn the_contracts_json_decodes() {
    let matter = matter_id(1);
    let scope: HostCommand = serde_json::from_str(&format!(
        r#"{{"set_addon_scope":{{"kind":"skill","id":"pdf","scope":{{"matter":"{matter}"}}}}}}"#
    ))
    .unwrap();
    assert_eq!(
        scope,
        HostCommand::SetAddonScope(SetAddonScope {
            kind: AddonKind::Skill,
            id: "pdf".to_owned(),
            scope: AddonScope::Matter(matter),
        })
    );
    let all: HostCommand =
        serde_json::from_str(r#"{"set_addon_scope":{"kind":"tool","id":"linear","scope":"all"}}"#)
            .unwrap();
    round_trips(&all);
    let on: HostCommand = serde_json::from_str(&format!(
        r#"{{"set_addon_on":{{"kind":"tool","id":"linear","matter":"{matter}","on":false}}}}"#
    ))
    .unwrap();
    assert!(matches!(
        on,
        HostCommand::SetAddonOn(SetAddonOn { on: false, .. })
    ));
    // Without a matter: every matter at once.
    let everywhere: HostCommand =
        serde_json::from_str(r#"{"set_addon_on":{"kind":"skill","id":"pdf","on":true}}"#).unwrap();
    assert!(matches!(
        everywhere,
        HostCommand::SetAddonOn(SetAddonOn {
            matter: None,
            on: true,
            ..
        })
    ));
    round_trips(&everywhere);
    assert!(everywhere.validate().is_ok());
    let remove: HostCommand =
        serde_json::from_str(r#"{"remove_addon":{"kind":"skill","id":"pdf"}}"#).unwrap();
    assert!(remove.validate().is_ok());
    // An unknown field or kind is refused, as in every command.
    let extra = r#"{"remove_addon":{"kind":"skill","id":"pdf","matter":null}}"#;
    assert!(serde_json::from_str::<HostCommand>(extra).is_err());
    let kind = r#"{"remove_addon":{"kind":"plugin","id":"pdf"}}"#;
    assert!(serde_json::from_str::<HostCommand>(kind).is_err());
}

#[test]
fn an_empty_or_long_id_is_refused() {
    let remove = |id: String| {
        HostCommand::RemoveAddon(RemoveAddon {
            kind: AddonKind::Tool,
            id,
        })
    };
    assert!(remove(String::new()).validate().is_err());
    assert!(
        remove("a".repeat(MAX_ADDON_ID_BYTES + 1))
            .validate()
            .is_err()
    );
    assert!(remove("a".repeat(MAX_ADDON_ID_BYTES)).validate().is_ok());
}
